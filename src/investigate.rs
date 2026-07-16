//! Server-directed, read-only issue investigation.

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{CliEnvironment, RuntimeError, encode_component, path_with_query};

/// Maximum scope characters rendered for one human label.
const HUMAN_SCOPE_LIMIT: usize = 120;

/// Validated issue scope retained across the directed follow-up.
#[derive(Debug)]
struct IssueScope<'a> {
    /// Project ownership scope required by both routes.
    project_id: &'a str,
    /// Optional participating service scope.
    service_name: Option<&'a str>,
    /// Optional release scope.
    release: Option<&'a str>,
    /// Optional environment scope.
    environment: Option<&'a str>,
    /// Exact lower time bound for related logs.
    first_seen_at: &'a str,
    /// Issue context retained for output only.
    last_seen_at: &'a str,
}

/// One of the two public server-directed investigation routes.
#[derive(Debug)]
enum InvestigationRoute<'a> {
    /// Fetch a bounded trace summary.
    Trace {
        /// Exact trace identifier returned by the issue.
        trace_id: &'a str,
    },
    /// Fetch logs related by issue scope and first-seen time.
    RelatedLogs,
}

impl InvestigationRoute<'_> {
    /// Stable backend action code.
    const fn code(&self) -> &'static str {
        match self {
            Self::Trace { .. } => "inspect_trace",
            Self::RelatedLogs => "inspect_related_logs",
        }
    }

    /// Stable backend action target.
    const fn target(&self) -> &'static str {
        match self {
            Self::Trace { .. } => "trace_summary",
            Self::RelatedLogs => "telemetry_logs",
        }
    }
}

/// Executes one issue-directed investigation without mutating server state.
pub async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    issue_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| transport_error())?;
    let issue_path = format!("/api/telemetry/issues/{}", encode_component(issue_id));
    let issue = get_json(&client, env, issue_path.as_str()).await?;
    let (scope, route) = validate_issue(&issue, issue_id)?;
    let follow_path = follow_path(&scope, &route);
    let result = get_json(&client, env, follow_path.as_str()).await?;
    validate_result(&result, &scope, &route)?;

    if json {
        let body = serde_json::json!({
            "issue": issue,
            "investigation": {
                "code": route.code(),
                "target": route.target(),
                "result": result,
            }
        });
        writeln!(output, "{body}")?;
    } else {
        write_human(output, issue_id, &scope, &route, &result)?;
    }
    Ok(())
}

/// Sends one authenticated GET and discards unsafe upstream failure text.
async fn get_json(
    client: &reqwest::Client,
    env: &CliEnvironment,
    path: &str,
) -> Result<serde_json::Value, RuntimeError> {
    let url = format!("{}{}", env.base_url.trim_end_matches('/'), path);
    let response = send_authenticated_with_refresh(client, env, |client, credential| {
        client.get(url.as_str()).bearer_auth(credential.token())
    })
    .await;
    let (response, credential) = match response {
        Ok(response) => response,
        Err(RuntimeError::Http(_)) => return Err(transport_error()),
        Err(error) => return Err(error),
    };
    let status = response.status();
    let body = response.text().await.map_err(|_| transport_error())?;
    if !status.is_success() {
        return Err(safe_api_error(status.as_u16(), &credential));
    }
    serde_json::from_str(body.as_str()).map_err(|_| invalid_response())
}

/// Validates the issue identity, scope, and exact directed action pair.
fn validate_issue<'a>(
    issue: &'a serde_json::Value,
    requested_issue_id: &str,
) -> Result<(IssueScope<'a>, InvestigationRoute<'a>), RuntimeError> {
    let object = issue.as_object().ok_or_else(invalid_response)?;
    if required_string(object, "id")? != requested_issue_id {
        return Err(invalid_response());
    }
    let project_id = required_string(object, "project_id")?;
    if !crate::ids::is_uuid(project_id) {
        return Err(invalid_response());
    }
    let scope = IssueScope {
        project_id,
        service_name: optional_scope(object, "service_name")?,
        release: optional_scope(object, "release")?,
        environment: optional_scope(object, "environment")?,
        first_seen_at: required_timestamp(object, "first_seen_at")?,
        last_seen_at: required_timestamp(object, "last_seen_at")?,
    };
    let action = object
        .get("next_action")
        .and_then(serde_json::Value::as_object)
        .filter(|action| action.len() == 2)
        .ok_or_else(invalid_response)?;
    let code = required_string(action, "code")?;
    let target = required_string(action, "target")?;
    let route = match (code, target) {
        ("inspect_trace", "trace_summary") => {
            let trace_id = required_string(object, "trace_id")?;
            if trace_id.trim().is_empty()
                || trace_id.chars().any(char::is_control)
                || matches!(trace_id, "." | "..")
            {
                return Err(invalid_response());
            }
            InvestigationRoute::Trace { trace_id }
        }
        ("inspect_related_logs", "telemetry_logs") => InvestigationRoute::RelatedLogs,
        _ => return Err(invalid_response()),
    };
    Ok((scope, route))
}

/// Builds the exact follow-up path and canonical scope order.
fn follow_path(scope: &IssueScope<'_>, route: &InvestigationRoute<'_>) -> String {
    match route {
        InvestigationRoute::Trace { trace_id } => path_with_query(
            format!(
                "/api/telemetry/traces/{}/summary",
                encode_trace_segment(trace_id)
            )
            .as_str(),
            &[
                ("project_id", Some(scope.project_id)),
                ("release", scope.release),
                ("environment", scope.environment),
            ],
        ),
        InvestigationRoute::RelatedLogs => path_with_query(
            "/api/logs",
            &[
                ("project_id", Some(scope.project_id)),
                ("service_name", scope.service_name),
                ("release", scope.release),
                ("environment", scope.environment),
                ("since", Some(scope.first_seen_at)),
            ],
        ),
    }
}

/// Validates enough of a successful follow-up to render it safely.
fn validate_result(
    result: &serde_json::Value,
    scope: &IssueScope<'_>,
    route: &InvestigationRoute<'_>,
) -> Result<(), RuntimeError> {
    match route {
        InvestigationRoute::Trace { trace_id } => {
            let object = result.as_object().ok_or_else(invalid_response)?;
            if required_string(object, "trace_id")? != *trace_id {
                return Err(invalid_response());
            }
            let project_ids = object
                .get("project_ids")
                .and_then(serde_json::Value::as_array)
                .filter(|project_ids| {
                    !project_ids.is_empty()
                        && project_ids.iter().all(serde_json::Value::is_string)
                        && project_ids
                            .iter()
                            .any(|project_id| project_id.as_str() == Some(scope.project_id))
                })
                .ok_or_else(invalid_response)?;
            let _project_count = project_ids.len();
            let _span_count = required_count(object, "span_count")?;
            let _error_span_count = required_count(object, "error_span_count")?;
            let _service_count = required_count(object, "service_count")?;
            let _duration_ms = required_count(object, "duration_ms")?;
            let _started_at = required_timestamp(object, "started_at")?;
        }
        InvestigationRoute::RelatedLogs => {
            if !logs(result).is_some_and(|logs| logs.iter().all(serde_json::Value::is_object)) {
                return Err(invalid_response());
            }
        }
    }
    Ok(())
}

/// Percent-encodes one trace path segment, including URL dot segments.
fn encode_trace_segment(trace_id: &str) -> String {
    encode_component(trace_id).replace('.', "%2E")
}

/// Writes a bounded summary without echoing issue or telemetry payload text.
fn write_human<W: std::io::Write>(
    output: &mut W,
    issue_id: &str,
    scope: &IssueScope<'_>,
    route: &InvestigationRoute<'_>,
    result: &serde_json::Value,
) -> Result<(), RuntimeError> {
    writeln!(output, "Issue {issue_id} investigation")?;
    writeln!(output, "Action: {} -> {}", route.code(), route.target())?;
    write!(output, "Scope: project={}", scope.project_id)?;
    write_optional_scope(output, "service", scope.service_name)?;
    write_optional_scope(output, "release", scope.release)?;
    write_optional_scope(output, "environment", scope.environment)?;
    writeln!(
        output,
        " first_seen={} last_seen={}",
        scope.first_seen_at, scope.last_seen_at
    )?;
    match route {
        InvestigationRoute::Trace { .. } => {
            let object = result.as_object().ok_or_else(invalid_response)?;
            writeln!(
                output,
                "Trace summary: spans={} errors={} services={} duration={}ms started={}",
                required_count(object, "span_count")?,
                required_count(object, "error_span_count")?,
                required_count(object, "service_count")?,
                required_count(object, "duration_ms")?,
                required_timestamp(object, "started_at")?
            )?;
            writeln!(
                output,
                "Next: inspect the JSON result for full public trace fields."
            )?;
        }
        InvestigationRoute::RelatedLogs => {
            writeln!(
                output,
                "Related logs: {}",
                logs(result).ok_or_else(invalid_response)?.len()
            )?;
            writeln!(
                output,
                "Next: inspect the JSON result for full public log fields."
            )?;
        }
    }
    Ok(())
}

/// Writes one optional scope label with a bounded human value.
fn write_optional_scope<W: std::io::Write>(
    output: &mut W,
    label: &str,
    value: Option<&str>,
) -> Result<(), std::io::Error> {
    if let Some(value) = value {
        write!(output, " {label}={}", bounded_scope(value))?;
    }
    Ok(())
}

/// Bounds one already control-safe scope value for human output.
fn bounded_scope(value: &str) -> String {
    let mut chars = value.chars();
    let mut output = chars.by_ref().take(HUMAN_SCOPE_LIMIT).collect::<String>();
    if chars.next().is_some() {
        output.push_str("...");
    }
    output
}

/// Returns the log items from either supported public response shape.
fn logs(value: &serde_json::Value) -> Option<&[serde_json::Value]> {
    if let Some(logs) = value.as_array() {
        return Some(logs.as_slice());
    }
    let object = value.as_object()?;
    let logs = object.get("logs")?.as_array()?.as_slice();
    match (object.len(), object.get("next_cursor")) {
        (1, None) => Some(logs),
        (2, Some(cursor)) if valid_cursor(cursor) => Some(logs),
        _ => None,
    }
}

/// Validates a terminal or complete public cursor object.
fn valid_cursor(value: &serde_json::Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(object) = value.as_object().filter(|object| object.len() == 2) else {
        return false;
    };
    let Some(time) = object.get("time").and_then(serde_json::Value::as_str) else {
        return false;
    };
    let Some(id) = object.get("id").and_then(serde_json::Value::as_str) else {
        return false;
    };
    crate::render::is_rfc3339_utc(time) && crate::ids::is_uuid(id)
}

/// Extracts one required nonblank, control-safe response string.
fn required_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, RuntimeError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty() && !has_unsafe_text(value))
        .ok_or_else(invalid_response)
}

/// Extracts one optional scope value, rejecting malformed or blank values.
fn optional_scope<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match object.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value))
            if !value.trim().is_empty() && !has_unsafe_text(value) =>
        {
            Ok(Some(value.as_str()))
        }
        Some(_) => Err(invalid_response()),
    }
}

fn has_unsafe_text(value: &str) -> bool {
    value.chars().any(|character| {
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

/// Extracts one required UTC RFC3339 timestamp.
fn required_timestamp<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, RuntimeError> {
    let value = required_string(object, key)?;
    if crate::render::is_rfc3339_utc(value) {
        Ok(value)
    } else {
        Err(invalid_response())
    }
}

/// Extracts one required non-negative integer count.
fn required_count(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<u64, RuntimeError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(invalid_response)
}

/// Returns a fixed path-free transport failure.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "issue investigation request could not be completed",
        next: "check network connectivity and retry the same issue investigation",
    }
}

/// Returns a fixed contract failure without reflecting server fields.
const fn invalid_response() -> RuntimeError {
    RuntimeError::InvestigationResponseInvalid
}

/// Converts a failed API status into fixed, value-safe guidance.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    let (error, code, next) = match status {
        401 => (
            "authentication required",
            "unauthorized",
            "run logbrew login",
        ),
        403 => (
            "investigation request forbidden",
            "forbidden",
            "confirm account access and retry the issue investigation",
        ),
        404 => (
            "investigation data not found",
            "not_found",
            "refresh the issue id and retry the investigation",
        ),
        400 | 422 => (
            "investigation request rejected",
            "validation_failed",
            "retry the issue investigation; if it repeats, report the public response contract",
        ),
        429 => (
            "investigation request rate limited",
            "rate_limited",
            "retry the same issue investigation later",
        ),
        500..=599 => (
            "investigation service unavailable",
            "service_unavailable",
            "retry the same issue investigation later",
        ),
        _ => (
            "investigation request failed",
            "request_failed",
            "check account access and retry the issue investigation",
        ),
    };
    RuntimeError::Api {
        status,
        body: serde_json::json!({
            "error": error,
            "code": code,
            "next": next,
            "next_action": {"code": "retry_investigation", "target": "issue"}
        })
        .to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}
