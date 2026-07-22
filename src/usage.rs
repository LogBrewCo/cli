//! Strict authenticated account-usage reads.

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{CliEnvironment, RuntimeError};

/// Maximum accepted account-usage response body.
const RESPONSE_LIMIT: usize = 256 * 1024;

/// Validated fields needed for bounded human rendering.
#[derive(Debug, Clone, Copy)]
struct UsageView<'a> {
    /// Current plan tier.
    tier: &'a str,
    /// Current plan status.
    plan_status: &'a str,
    /// Overall usage state.
    state: &'a str,
    /// Inclusive period start.
    period_start: &'a str,
    /// Exclusive period end.
    period_end: &'a str,
    /// Next reset timestamp.
    reset_at: &'a str,
    /// Used event count.
    events: u64,
    /// Configured event limit.
    event_limit: Option<u64>,
    /// Used byte count.
    bytes: u64,
    /// Configured byte limit.
    byte_limit: Option<u64>,
    /// Used project count.
    projects: u64,
    /// Configured project limit.
    project_limit: Option<u64>,
    /// Resource currently driving the usage state.
    limit: Option<&'a str>,
    /// Validated server-directed action code.
    action_code: &'a str,
}

/// Nullable account limits needed for human totals.
#[derive(Debug, Clone, Copy)]
struct UsageLimits {
    /// Configured event limit.
    events: Option<u64>,
    /// Configured byte limit.
    bytes: Option<u64>,
    /// Configured project limit.
    projects: Option<u64>,
}

/// Duplicate-aware exact top-level usage shape.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageShape {
    #[serde(rename = "period_start")]
    _period_start: serde_json::Value,
    #[serde(rename = "period_end")]
    _period_end: serde_json::Value,
    #[serde(rename = "reset_at")]
    _reset_at: serde_json::Value,
    #[serde(rename = "plan")]
    _plan: PlanShape,
    #[serde(rename = "limits")]
    _limits: LimitsShape,
    #[serde(rename = "usage")]
    _usage: TotalsShape,
    #[serde(rename = "state")]
    _state: serde_json::Value,
    #[serde(rename = "warning_threshold")]
    _warning_threshold: serde_json::Value,
    #[serde(rename = "percent_used")]
    _percent_used: serde_json::Value,
    #[serde(rename = "warning")]
    _warning: serde_json::Value,
    #[serde(rename = "blocked")]
    _blocked: serde_json::Value,
    #[serde(rename = "limit")]
    _limit: serde_json::Value,
    #[serde(rename = "next")]
    _next: serde_json::Value,
    #[serde(rename = "next_action")]
    _next_action: UsageActionShape,
    #[serde(rename = "by_project")]
    _by_project: Vec<ProjectUsageShape>,
    #[serde(rename = "by_stream")]
    _by_stream: Vec<StreamUsageShape>,
}

/// Duplicate-aware plan object.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanShape {
    #[serde(rename = "tier")]
    _tier: serde_json::Value,
    #[serde(rename = "status")]
    _status: serde_json::Value,
}

/// Duplicate-aware limits object.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsShape {
    #[serde(rename = "events")]
    _events: serde_json::Value,
    #[serde(rename = "bytes")]
    _bytes: serde_json::Value,
    #[serde(rename = "projects")]
    _projects: serde_json::Value,
    #[serde(rename = "retention_days")]
    _retention_days: serde_json::Value,
}

/// Duplicate-aware totals object.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TotalsShape {
    #[serde(rename = "events")]
    _events: serde_json::Value,
    #[serde(rename = "bytes")]
    _bytes: serde_json::Value,
    #[serde(rename = "projects")]
    _projects: serde_json::Value,
}

/// Duplicate-aware usage action object.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageActionShape {
    #[serde(rename = "code")]
    _code: serde_json::Value,
    #[serde(rename = "target")]
    _target: serde_json::Value,
    #[serde(rename = "state")]
    _state: serde_json::Value,
    #[serde(rename = "limit")]
    _limit: serde_json::Value,
    #[serde(rename = "reset_at")]
    _reset_at: serde_json::Value,
}

/// Duplicate-aware project breakdown row.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectUsageShape {
    #[serde(rename = "project_id")]
    _project_id: serde_json::Value,
    #[serde(rename = "events")]
    _events: serde_json::Value,
    #[serde(rename = "bytes")]
    _bytes: serde_json::Value,
}

/// Duplicate-aware stream breakdown row.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamUsageShape {
    #[serde(rename = "kind")]
    _kind: serde_json::Value,
    #[serde(rename = "events")]
    _events: serde_json::Value,
    #[serde(rename = "bytes")]
    _bytes: serde_json::Value,
}

/// Duplicate-aware standard error envelope.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageErrorShape {
    #[serde(rename = "error")]
    _error: serde_json::Value,
    #[serde(rename = "code")]
    _code: serde_json::Value,
    #[serde(rename = "next")]
    _next: serde_json::Value,
    #[serde(rename = "next_action")]
    _next_action: ErrorActionShape,
}

/// Duplicate-aware standard error action.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorActionShape {
    #[serde(rename = "code")]
    _code: serde_json::Value,
    #[serde(rename = "target")]
    _target: serde_json::Value,
}

/// Executes the exact account-usage read and writes JSON or bounded human output.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let origin = normalized_origin(env.base_url.as_str())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| transport_error())?;
    let url = format!("{origin}/api/account/usage");
    let response = send_authenticated_with_refresh(&client, env, |client, credential| {
        client.get(url.as_str()).bearer_auth(credential.token())
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    let body = bounded_body(response).await?;

    if status != 200 {
        return Err(validate_error(status, body.as_str(), &credential)?);
    }
    let _shape =
        serde_json::from_str::<UsageShape>(body.as_str()).map_err(|_| invalid_response())?;
    let value =
        serde_json::from_str::<serde_json::Value>(body.as_str()).map_err(|_| invalid_response())?;
    let view = validate_success(&value)?;
    if json {
        writeln!(output, "{value}")?;
    } else {
        write_human(&view, output)?;
    }
    Ok(())
}

/// Reads a response incrementally without retaining an oversized body.
async fn bounded_body(mut response: reqwest::Response) -> Result<String, RuntimeError> {
    if response.content_length().is_some_and(|length| {
        usize::try_from(length).map_or(true, |length| length > RESPONSE_LIMIT)
    }) {
        return Err(invalid_response());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| transport_error())? {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(invalid_response());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| invalid_response())
}

/// Converts auth and transport failures into fixed, path-free usage recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::Api { .. }
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_error(),
    }
}

/// Validates the complete success contract and returns only human-rendered fields.
fn validate_success(value: &serde_json::Value) -> Result<UsageView<'_>, RuntimeError> {
    let object = exact_object(
        value,
        &[
            "period_start",
            "period_end",
            "reset_at",
            "plan",
            "limits",
            "usage",
            "state",
            "warning_threshold",
            "percent_used",
            "warning",
            "blocked",
            "limit",
            "next",
            "next_action",
            "by_project",
            "by_stream",
        ],
    )?;
    let period_start = timestamp(object.get("period_start"))?;
    let period_end = timestamp(object.get("period_end"))?;
    let reset_at = timestamp(object.get("reset_at"))?;
    let plan = validate_plan(object.get("plan"))?;
    let limits = validate_limits(object.get("limits"))?;
    let usage = validate_totals(object.get("usage"))?;
    let state = enum_string(object.get("state"), &["ok", "warning", "blocked"])?;
    let _warning_threshold = finite_number(object.get("warning_threshold"))?;
    let percent_used = finite_number(object.get("percent_used"))?;
    if !(0.0..=100.0).contains(&percent_used) {
        return Err(invalid_response());
    }
    let warning = boolean(object.get("warning"))?;
    let blocked = boolean(object.get("blocked"))?;
    let limit = nullable_enum_string(object.get("limit"), &["events", "bytes", "projects"])?;
    let valid_state = matches!(
        (state, warning, blocked, limit),
        ("ok", false, false, None)
            | ("warning", true, false, Some(_))
            | ("blocked", true, true, Some(_))
    );
    if !valid_state {
        return Err(invalid_response());
    }
    let _next = safe_string(object.get("next"), 512)?;
    let action_code = validate_action(object.get("next_action"), state, limit, reset_at)?;
    validate_projects(object.get("by_project"))?;
    validate_streams(object.get("by_stream"))?;

    Ok(UsageView {
        tier: plan.0,
        plan_status: plan.1,
        state,
        period_start,
        period_end,
        reset_at,
        events: usage.0,
        event_limit: limits.events,
        bytes: usage.1,
        byte_limit: limits.bytes,
        projects: usage.2,
        project_limit: limits.projects,
        limit,
        action_code,
    })
}

/// Validates plan tier and lifecycle status.
fn validate_plan(value: Option<&serde_json::Value>) -> Result<(&str, &str), RuntimeError> {
    let object = exact_object(required(value)?, &["tier", "status"])?;
    Ok((
        safe_string(object.get("tier"), 64)?,
        enum_string(
            object.get("status"),
            &["free", "trial", "active", "past_due", "disabled"],
        )?,
    ))
}

/// Validates nullable configured account limits.
fn validate_limits(value: Option<&serde_json::Value>) -> Result<UsageLimits, RuntimeError> {
    let object = exact_object(
        required(value)?,
        &["events", "bytes", "projects", "retention_days"],
    )?;
    let events = nullable_u64(object.get("events"))?;
    let bytes = nullable_u64(object.get("bytes"))?;
    let projects = nullable_u64(object.get("projects"))?;
    let _retention_days = nullable_u64(object.get("retention_days"))?;
    Ok(UsageLimits {
        events,
        bytes,
        projects,
    })
}

/// Validates non-negative account usage totals.
fn validate_totals(value: Option<&serde_json::Value>) -> Result<(u64, u64, u64), RuntimeError> {
    let object = exact_object(required(value)?, &["events", "bytes", "projects"])?;
    Ok((
        unsigned(object.get("events"))?,
        unsigned(object.get("bytes"))?,
        unsigned(object.get("projects"))?,
    ))
}

/// Validates the exact nested action and its parent-field bindings.
fn validate_action<'a>(
    value: Option<&'a serde_json::Value>,
    state: &str,
    limit: Option<&str>,
    reset_at: &str,
) -> Result<&'a str, RuntimeError> {
    let object = exact_object(
        required(value)?,
        &["code", "target", "state", "limit", "reset_at"],
    )?;
    let code = enum_string(
        object.get("code"),
        &[
            "continue_sending_telemetry",
            "reduce_usage_or_upgrade",
            "archive_project_or_upgrade",
            "wait_until_reset_or_upgrade",
            "check_usage_limits",
        ],
    )?;
    let target = enum_string(
        object.get("target"),
        &["telemetry_ingest", "account_usage", "projects", "pricing"],
    )?;
    let action_state = enum_string(object.get("state"), &["ok", "warning", "blocked"])?;
    let action_limit = nullable_enum_string(object.get("limit"), &["events", "bytes", "projects"])?;
    let action_reset = timestamp(object.get("reset_at"))?;
    let valid_pair = matches!(
        (code, target, state, limit),
        ("continue_sending_telemetry", "telemetry_ingest", "ok", None)
            | ("check_usage_limits", "account_usage", "ok", None)
            | (
                "reduce_usage_or_upgrade",
                "account_usage",
                "warning",
                Some("events" | "bytes")
            )
            | (
                "archive_project_or_upgrade",
                "projects",
                "warning",
                Some("projects")
            )
            | ("wait_until_reset_or_upgrade", "pricing", "blocked", Some(_))
    );
    if !valid_pair || action_state != state || action_limit != limit || action_reset != reset_at {
        return Err(invalid_response());
    }
    Ok(code)
}

/// Validates all per-project breakdown rows without retaining identifiers.
fn validate_projects(value: Option<&serde_json::Value>) -> Result<(), RuntimeError> {
    let rows = required(value)?.as_array().ok_or_else(invalid_response)?;
    for row in rows {
        let object = exact_object(row, &["project_id", "events", "bytes"])?;
        let project_id = safe_string(object.get("project_id"), 64)?;
        if !crate::ids::is_uuid(project_id) {
            return Err(invalid_response());
        }
        let _events = unsigned(object.get("events"))?;
        let _bytes = unsigned(object.get("bytes"))?;
    }
    Ok(())
}

/// Validates all per-stream breakdown rows without retaining their values.
fn validate_streams(value: Option<&serde_json::Value>) -> Result<(), RuntimeError> {
    let rows = required(value)?.as_array().ok_or_else(invalid_response)?;
    for row in rows {
        let object = exact_object(row, &["kind", "events", "bytes"])?;
        let _kind = safe_string(object.get("kind"), 64)?;
        let _events = unsigned(object.get("events"))?;
        let _bytes = unsigned(object.get("bytes"))?;
    }
    Ok(())
}

/// Validates one typed error envelope, then replaces it with fixed local guidance.
fn validate_error(
    status: u16,
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    let _shape = serde_json::from_str::<UsageErrorShape>(body).map_err(|_| invalid_response())?;
    let value = serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_response())?;
    let object = exact_object(&value, &["error", "code", "next", "next_action"])?;
    let _error = safe_string(object.get("error"), 256)?;
    let _next = safe_string(object.get("next"), 512)?;
    let code = safe_string(object.get("code"), 64)?;
    let action = exact_object(required(object.get("next_action"))?, &["code", "target"])?;
    let action_code = safe_string(action.get("code"), 64)?;
    let action_target = safe_string(action.get("target"), 64)?;
    let typed = match status {
        401 => code == "unauthorized" && action_code == "sign_in" && action_target == "auth",
        405 => {
            code == "method_not_allowed"
                && action_code == "use_supported_method"
                && action_target == "api_method"
        }
        429 => code == "rate_limited" && action_code == "retry" && action_target == "request",
        500..=599 => {
            matches!(code, "internal_error" | "storage_error" | "json_error")
                && action_code == "retry"
                && action_target == "request"
        }
        _ => false,
    };
    if !typed {
        return Err(invalid_response());
    }
    Ok(RuntimeError::Api {
        status,
        body: safe_error_body(status),
        auth_source: credential.source(),
        auth_label: credential.label(),
    })
}

/// Returns a fixed synthetic body for status-derived recovery.
fn safe_error_body(status: u16) -> String {
    let value = match status {
        401 => serde_json::json!({
            "error": "account authentication is invalid",
            "code": "unauthorized",
            "next": "run logbrew login",
            "next_action": {"code": "sign_in", "target": "auth"}
        }),
        405 => serde_json::json!({
            "error": "usage method is not supported",
            "code": "method_not_allowed",
            "next": "retry logbrew usage with the supported GET request",
            "next_action": {"code": "use_supported_method", "target": "api_method"}
        }),
        429 => serde_json::json!({
            "error": "usage request is rate limited",
            "code": "rate_limited",
            "next": "retry logbrew usage later",
            "next_action": {"code": "retry", "target": "request"}
        }),
        500..=599 => serde_json::json!({
            "error": "usage service is unavailable",
            "code": "server_error",
            "next": "retry logbrew usage later",
            "next_action": {"code": "retry", "target": "request"}
        }),
        _ => serde_json::json!({
            "error": "usage request failed",
            "code": "usage_request_failed",
            "next": "check account access and retry logbrew usage",
            "next_action": {"code": "retry", "target": "request"}
        }),
    };
    value.to_string()
}

/// Writes the fixed scan-oriented account-usage summary.
fn write_human<W: std::io::Write>(
    view: &UsageView<'_>,
    output: &mut W,
) -> Result<(), std::io::Error> {
    writeln!(output, "LogBrew account usage")?;
    writeln!(output, "Plan: {} ({})", view.tier, view.plan_status)?;
    writeln!(output, "State: {}", view.state)?;
    writeln!(
        output,
        "Period: {} to {} | Reset: {}",
        view.period_start, view.period_end, view.reset_at
    )?;
    writeln!(
        output,
        "Events: {} / {}",
        view.events,
        limit_text(view.event_limit)
    )?;
    writeln!(
        output,
        "Bytes: {} / {}",
        view.bytes,
        limit_text(view.byte_limit)
    )?;
    writeln!(
        output,
        "Projects: {} / {}",
        view.projects,
        limit_text(view.project_limit)
    )?;
    writeln!(output, "Driving limit: {}", view.limit.unwrap_or("none"))?;
    writeln!(output, "Next: {}", action_next(view.action_code))
}

/// Formats a nullable configured limit without introducing local calculations.
fn limit_text(limit: Option<u64>) -> String {
    limit.map_or_else(|| String::from("unlimited"), |value| value.to_string())
}

/// Maps a validated server action to one bounded CLI-owned next step.
fn action_next(code: &str) -> &'static str {
    match code {
        "continue_sending_telemetry" => "continue sending telemetry",
        "reduce_usage_or_upgrade" => "reduce telemetry usage or review account options",
        "archive_project_or_upgrade" => "archive an unused project or review account options",
        "wait_until_reset_or_upgrade" => "wait until usage resets or review account options",
        "check_usage_limits" => "inspect account usage limits",
        _ => "retry logbrew usage",
    }
}

/// Returns one exact object or a fixed invalid-response error.
fn exact_object<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, serde_json::Value>, RuntimeError> {
    let object = value.as_object().ok_or_else(invalid_response)?;
    if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
        return Err(invalid_response());
    }
    Ok(object)
}

/// Returns one required JSON value.
fn required(value: Option<&serde_json::Value>) -> Result<&serde_json::Value, RuntimeError> {
    value.ok_or_else(invalid_response)
}

/// Returns one bounded display-safe string.
fn safe_string(value: Option<&serde_json::Value>, limit: usize) -> Result<&str, RuntimeError> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && safe_text(value, limit))
        .ok_or_else(invalid_response)
}

/// Returns one string from an exact public vocabulary.
fn enum_string<'a>(
    value: Option<&'a serde_json::Value>,
    allowed: &[&str],
) -> Result<&'a str, RuntimeError> {
    let value = safe_string(value, 64)?;
    allowed
        .contains(&value)
        .then_some(value)
        .ok_or_else(invalid_response)
}

/// Returns one nullable string from an exact public vocabulary.
fn nullable_enum_string<'a>(
    value: Option<&'a serde_json::Value>,
    allowed: &[&str],
) -> Result<Option<&'a str>, RuntimeError> {
    let value = required(value)?;
    if value.is_null() {
        Ok(None)
    } else {
        enum_string(Some(value), allowed).map(Some)
    }
}

/// Returns one unsigned whole number.
fn unsigned(value: Option<&serde_json::Value>) -> Result<u64, RuntimeError> {
    value
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(invalid_response)
}

/// Returns one nullable unsigned whole number.
fn nullable_u64(value: Option<&serde_json::Value>) -> Result<Option<u64>, RuntimeError> {
    let value = required(value)?;
    if value.is_null() {
        Ok(None)
    } else {
        value.as_u64().map(Some).ok_or_else(invalid_response)
    }
}

/// Returns one JSON boolean.
fn boolean(value: Option<&serde_json::Value>) -> Result<bool, RuntimeError> {
    value
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(invalid_response)
}

/// Returns one finite JSON number.
fn finite_number(value: Option<&serde_json::Value>) -> Result<f64, RuntimeError> {
    value
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(invalid_response)
}

/// Returns one valid RFC3339 timestamp.
fn timestamp(value: Option<&serde_json::Value>) -> Result<&str, RuntimeError> {
    let value = safe_string(value, 64)?;
    is_rfc3339(value)
        .then_some(value)
        .ok_or_else(invalid_response)
}

/// Rejects controls and display-direction characters in server text.
fn safe_text(value: &str, limit: usize) -> bool {
    value.chars().count() <= limit
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '\u{061c}'
                        | '\u{200b}'..='\u{200f}'
                        | '\u{2028}'..='\u{202e}'
                        | '\u{2060}'..='\u{206f}'
                        | '\u{feff}'
                        | '\u{fff9}'..='\u{fffb}'
                )
        })
}

/// Validates RFC3339 calendar, time, fraction, and offset syntax.
fn is_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let Some(year) = bytes.get(0..4).and_then(digits_u32) else {
        return false;
    };
    let Some(month) = bytes.get(5..7).and_then(digits_u32) else {
        return false;
    };
    let Some(day) = bytes.get(8..10).and_then(digits_u32) else {
        return false;
    };
    let Some(hour) = bytes.get(11..13).and_then(digits_u32) else {
        return false;
    };
    let Some(minute) = bytes.get(14..16).and_then(digits_u32) else {
        return false;
    };
    let Some(second) = bytes.get(17..19).and_then(digits_u32) else {
        return false;
    };
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return false;
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    match bytes.get(index) {
        Some(b'Z') => index + 1 == bytes.len(),
        Some(b'+' | b'-') => {
            let Some(offset_hour) = bytes.get(index + 1..index + 3).and_then(digits_u32) else {
                return false;
            };
            let Some(offset_minute) = bytes.get(index + 4..index + 6).and_then(digits_u32) else {
                return false;
            };
            bytes.get(index + 3) == Some(&b':')
                && index + 6 == bytes.len()
                && offset_hour <= 23
                && offset_minute <= 59
        }
        Some(_) | None => false,
    }
}

/// Parses an ASCII digit slice as an unsigned integer.
fn digits_u32(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(*byte - b'0'))
    })
}

/// Returns the number of days in one Gregorian month.
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Validates the configured API origin without retaining it in errors.
fn normalized_origin(base_url: &str) -> Result<String, RuntimeError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| transport_error())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(transport_error());
    }
    url.set_path("");
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// Fixed recovery for malformed or oversized account-usage responses.
const fn invalid_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "usage response is invalid",
        next: "retry logbrew usage; if it repeats, report the public response contract",
    }
}

/// Fixed recovery for local HTTP transport failure.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "usage request could not be completed",
        next: "check network connectivity and retry logbrew usage",
    }
}
