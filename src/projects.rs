//! Strict authenticated project catalog reads.

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{CliEnvironment, RuntimeError};

/// Maximum accepted project catalog response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Maximum project rows shown in human output.
const HUMAN_ROW_LIMIT: usize = 100;

/// Duplicate-aware exact project row shape.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectShape {
    #[serde(rename = "id")]
    _id: serde_json::Value,
    #[serde(rename = "name")]
    _name: serde_json::Value,
    #[serde(rename = "provider_project_id")]
    _provider_project_id: serde_json::Value,
    #[serde(rename = "provider_project_slug")]
    _provider_project_slug: serde_json::Value,
    #[serde(rename = "provider")]
    _provider: serde_json::Value,
    #[serde(rename = "is_active")]
    _is_active: serde_json::Value,
    #[serde(rename = "language")]
    _language: serde_json::Value,
    #[serde(rename = "setup_status")]
    _setup_status: serde_json::Value,
    #[serde(rename = "setup_started_at")]
    _setup_started_at: serde_json::Value,
    #[serde(rename = "first_telemetry_seen_at")]
    _first_telemetry_seen_at: serde_json::Value,
    #[serde(rename = "last_seen_at")]
    _last_seen_at: serde_json::Value,
    #[serde(rename = "last_release")]
    _last_release: serde_json::Value,
    #[serde(rename = "last_environment")]
    _last_environment: serde_json::Value,
    #[serde(rename = "created_at")]
    _created_at: serde_json::Value,
}

/// Duplicate-aware standard API error envelope.
#[expect(
    clippy::missing_docs_in_private_items,
    reason = "field names intentionally mirror the validated public JSON contract"
)]
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorShape {
    #[serde(rename = "error")]
    _error: serde_json::Value,
    #[serde(rename = "code")]
    _code: serde_json::Value,
    #[serde(rename = "next")]
    _next: serde_json::Value,
    #[serde(rename = "next_action")]
    _next_action: ErrorActionShape,
}

/// Duplicate-aware standard API error action.
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

/// Human-rendered fields from one fully validated project row.
struct ProjectView<'a> {
    /// Account-owned project UUID.
    id: &'a str,
    /// Display-safe project name.
    name: &'a str,
    /// Canonical setup lifecycle state.
    setup_status: &'a str,
    /// Most recent cross-stream telemetry timestamp.
    last_seen_at: Option<&'a str>,
}

/// Executes one strict authenticated project catalog read.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let origin =
        crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport_error)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| transport_error())?;
    let url = format!("{origin}/api/projects");
    let response = send_authenticated_with_refresh(&client, env, |client, credential| {
        client.get(url.as_str()).bearer_auth(credential.token())
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    let body = crate::http::bounded_body(response, RESPONSE_LIMIT)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => invalid_response(),
            crate::http::BodyError::Transport => transport_error(),
        })?;

    if status != 200 {
        return Err(validate_error(status, body.as_str(), &credential)?);
    }
    let _shape =
        serde_json::from_str::<Vec<ProjectShape>>(body.as_str()).map_err(|_| invalid_response())?;
    let value =
        serde_json::from_str::<serde_json::Value>(body.as_str()).map_err(|_| invalid_response())?;
    let projects = validate_catalog(&value)?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        write_human(projects.as_slice(), output)?;
    }
    Ok(())
}

/// Converts request failures into fixed, host-free project recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::Api { .. }
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::ExplainResponseInvalid
        | RuntimeError::AnalyticsOverviewResponseInvalid
        | RuntimeError::AnalyticsPropertiesResponseInvalid
        | RuntimeError::AnalyticsResponseInvalid
        | RuntimeError::AnalyticsFunnelResponseInvalid
        | RuntimeError::AnalyticsRetentionResponseInvalid
        | RuntimeError::AnalyticsLifecycleResponseInvalid
        | RuntimeError::AnalyticsSegmentResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_error(),
    }
}

/// Validates every project before any JSON or human output is emitted.
fn validate_catalog(value: &serde_json::Value) -> Result<Vec<ProjectView<'_>>, RuntimeError> {
    let rows = value.as_array().ok_or_else(invalid_response)?;
    rows.iter().map(validate_project).collect()
}

/// Validates the exact active-project row contract.
fn validate_project(value: &serde_json::Value) -> Result<ProjectView<'_>, RuntimeError> {
    let object = value.as_object().ok_or_else(invalid_response)?;
    let id = safe_string(object.get("id"), 64, false)?;
    if !crate::ids::is_uuid(id) {
        return Err(invalid_response());
    }
    let name = safe_string(object.get("name"), 120, false)?;
    let _provider_project_id = safe_string(object.get("provider_project_id"), 256, false)?;
    let _provider_project_slug = nullable_string(object.get("provider_project_slug"), 256)?;
    let _provider = safe_string(object.get("provider"), 64, false)?;
    if object.get("is_active").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(invalid_response());
    }
    let _language = nullable_string(object.get("language"), 64)?;
    let setup_status = enum_string(
        object.get("setup_status"),
        &[
            "created",
            "setup_started",
            "sdk_seen",
            "first_telemetry_seen",
            "active",
        ],
    )?;
    let _setup_started_at = nullable_timestamp(object.get("setup_started_at"))?;
    let _first_telemetry_seen_at = nullable_timestamp(object.get("first_telemetry_seen_at"))?;
    let last_seen_at = nullable_timestamp(object.get("last_seen_at"))?;
    let _last_release = nullable_string(object.get("last_release"), 256)?;
    let _last_environment = nullable_string(object.get("last_environment"), 256)?;
    let _created_at = timestamp(object.get("created_at"))?;
    Ok(ProjectView {
        id,
        name,
        setup_status,
        last_seen_at,
    })
}

/// Validates a duplicate-aware standard error before replacing its text.
fn validate_error(
    status: u16,
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    let _shape = serde_json::from_str::<ErrorShape>(body).map_err(|_| invalid_response())?;
    let value = serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_response())?;
    let object = value.as_object().ok_or_else(invalid_response)?;
    let _error = safe_string(object.get("error"), 512, false)?;
    let code = safe_string(object.get("code"), 64, false)?;
    let _next = safe_string(object.get("next"), 512, false)?;
    let action = object
        .get("next_action")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid_response)?;
    let action_code = safe_string(action.get("code"), 64, false)?;
    let action_target = safe_string(action.get("target"), 64, false)?;
    let typed = match status {
        401 => code == "unauthorized" && action_code == "sign_in" && action_target == "auth",
        405 => {
            code == "method_not_allowed"
                && action_code == "use_supported_method"
                && action_target == "api_method"
        }
        500..=599 => {
            matches!(code, "storage_error" | "json_error" | "internal_error")
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

/// Writes a bounded project catalog without unbounded backend fields.
fn write_human<W: std::io::Write>(
    projects: &[ProjectView<'_>],
    output: &mut W,
) -> Result<(), std::io::Error> {
    writeln!(output, "Projects ({})", projects.len())?;
    if projects.is_empty() {
        writeln!(output, "No active projects found.")?;
        writeln!(
            output,
            "Next: create a project with logbrew projects create."
        )
    } else {
        for project in projects.iter().take(HUMAN_ROW_LIMIT) {
            writeln!(
                output,
                "- {} id={} setup={} last_seen={}",
                project.name,
                project.id,
                project.setup_status,
                project.last_seen_at.unwrap_or("never")
            )?;
        }
        if projects.len() > HUMAN_ROW_LIMIT {
            writeln!(
                output,
                "{} additional projects omitted.",
                projects.len() - HUMAN_ROW_LIMIT
            )?;
        }
        Ok(())
    }
}

/// Returns one required bounded control-free string.
fn safe_string(
    value: Option<&serde_json::Value>,
    limit: usize,
    allow_empty: bool,
) -> Result<&str, RuntimeError> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() <= limit)
        .filter(|value| allow_empty || !value.trim().is_empty())
        .filter(|value| crate::http::display_safe(value, usize::MAX))
        .ok_or_else(invalid_response)
}

/// Returns one nullable bounded control-free string.
fn nullable_string(
    value: Option<&serde_json::Value>,
    limit: usize,
) -> Result<Option<&str>, RuntimeError> {
    match value {
        Some(serde_json::Value::Null) => Ok(None),
        Some(value) => safe_string(Some(value), limit, true).map(Some),
        None => Err(invalid_response()),
    }
}

/// Returns one value from a closed string vocabulary.
fn enum_string<'a>(
    value: Option<&'a serde_json::Value>,
    allowed: &[&str],
) -> Result<&'a str, RuntimeError> {
    let value = safe_string(value, 64, false)?;
    allowed
        .contains(&value)
        .then_some(value)
        .ok_or_else(invalid_response)
}

/// Returns one valid RFC3339 timestamp.
fn timestamp(value: Option<&serde_json::Value>) -> Result<&str, RuntimeError> {
    let value = safe_string(value, 64, false)?;
    crate::time::parse_rfc3339(value)
        .is_some()
        .then_some(value)
        .ok_or_else(invalid_response)
}

/// Returns one required nullable RFC3339 timestamp.
fn nullable_timestamp(value: Option<&serde_json::Value>) -> Result<Option<&str>, RuntimeError> {
    match value {
        Some(serde_json::Value::Null) => Ok(None),
        Some(value) => timestamp(Some(value)).map(Some),
        None => Err(invalid_response()),
    }
}

/// Returns fixed project-catalog recovery for a malformed success or error.
const fn invalid_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project catalog response was invalid",
        next: "retry logbrew projects; if it repeats, report the public response contract",
    }
}

/// Returns fixed project-catalog network recovery without a URL.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project catalog request could not be completed",
        next: "check network connectivity and retry logbrew projects",
    }
}

/// Returns a synthetic, allowlisted API error body.
fn safe_error_body(status: u16) -> String {
    let value = match status {
        401 => serde_json::json!({
            "error": "account authentication is invalid",
            "code": "unauthorized",
            "next": "run logbrew login",
            "next_action": {"code": "sign_in", "target": "auth"}
        }),
        405 => serde_json::json!({
            "error": "project catalog method is not supported",
            "code": "method_not_allowed",
            "next": "retry logbrew projects with the supported GET request",
            "next_action": {"code": "use_supported_method", "target": "api_method"}
        }),
        _ => serde_json::json!({
            "error": "project catalog is unavailable",
            "code": "server_error",
            "next": "retry logbrew projects later",
            "next_action": {"code": "retry", "target": "request"}
        }),
    };
    value.to_string()
}
