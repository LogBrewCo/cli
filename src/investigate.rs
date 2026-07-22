//! Server-directed, read-only issue investigation.

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{CliEnvironment, RuntimeError, encode_component, path_with_query};

/// Maximum accepted body for either investigation read.
const RESPONSE_LIMIT: usize = 256 * 1024;

/// Maximum accepted scope length used to construct a follow-up request.
const SCOPE_LIMIT: usize = 256;

/// Validated issue scope retained across the directed follow-up.
#[derive(Debug)]
struct IssueScope<'a> {
    /// Project ownership scope required by both routes.
    project_id: &'a str,
    /// Participating service required by the related-log route.
    service_name: &'a str,
    /// Release scope sent when nonblank for traces and required for logs.
    release: &'a str,
    /// Environment scope sent when nonblank for traces and required for logs.
    environment: &'a str,
    /// Exact lower time bound for related logs.
    first_seen_at: &'a str,
}

/// One of the two public server-directed investigation routes.
#[derive(Debug)]
enum InvestigationRoute<'a> {
    /// Fetch one trace summary.
    Trace {
        /// Exact trace identifier returned by the issue.
        trace_id: &'a str,
    },
    /// Fetch one bounded page of related logs.
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

/// Duplicate-aware exact issue-detail shape.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueShape {
    #[serde(rename = "id")]
    _id: serde_json::Value,
    #[serde(rename = "project_id")]
    _project_id: serde_json::Value,
    #[serde(rename = "fingerprint")]
    _fingerprint: serde_json::Value,
    #[serde(rename = "severity")]
    _severity: serde_json::Value,
    #[serde(rename = "title")]
    _title: serde_json::Value,
    #[serde(rename = "message")]
    _message: serde_json::Value,
    #[serde(default, rename = "stack_trace")]
    _stack_trace: Option<serde_json::Value>,
    #[serde(rename = "attributes")]
    _attributes: serde_json::Value,
    #[serde(rename = "environment")]
    _environment: serde_json::Value,
    #[serde(rename = "release")]
    _release: serde_json::Value,
    #[serde(rename = "service_name")]
    _service_name: serde_json::Value,
    #[serde(default, rename = "trace_id")]
    _trace_id: Option<serde_json::Value>,
    #[serde(default, rename = "symbolication")]
    _symbolication: Option<serde_json::Value>,
    #[serde(rename = "next_action")]
    _next_action: ActionShape,
    #[serde(rename = "status")]
    _status: serde_json::Value,
    #[serde(rename = "occurrence_count")]
    _occurrence_count: serde_json::Value,
    #[serde(rename = "first_seen_at")]
    _first_seen_at: serde_json::Value,
    #[serde(rename = "last_seen_at")]
    _last_seen_at: serde_json::Value,
}

/// Duplicate-aware exact action object.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionShape {
    #[serde(rename = "code")]
    _code: serde_json::Value,
    #[serde(rename = "target")]
    _target: serde_json::Value,
}

/// Duplicate-aware exact trace-summary shape.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceSummaryShape {
    #[serde(rename = "trace_id")]
    _trace_id: serde_json::Value,
    #[serde(rename = "span_count")]
    _span_count: serde_json::Value,
    #[serde(rename = "error_span_count")]
    _error_span_count: serde_json::Value,
    #[serde(rename = "service_count")]
    _service_count: serde_json::Value,
    #[serde(rename = "project_count")]
    _project_count: serde_json::Value,
    #[serde(rename = "started_at")]
    _started_at: serde_json::Value,
    #[serde(rename = "duration_ms")]
    _duration_ms: serde_json::Value,
    #[serde(rename = "root_span")]
    _root_span: serde_json::Value,
    #[serde(rename = "slowest_child_span")]
    _slowest_child_span: serde_json::Value,
    #[serde(rename = "slowest_path")]
    _slowest_path: serde_json::Value,
    #[serde(rename = "error_spans")]
    _error_spans: serde_json::Value,
    #[serde(rename = "services")]
    _services: serde_json::Value,
    #[serde(rename = "releases")]
    _releases: serde_json::Value,
    #[serde(rename = "environments")]
    _environments: serde_json::Value,
}

/// Duplicate-aware supported related-log response shapes.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum LogsShape {
    Array(Vec<serde_json::Value>),
    Envelope(LogsEnvelopeShape),
}

/// Duplicate-aware exact cursor log envelope.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LogsEnvelopeShape {
    /// Bounded first-page log rows.
    #[serde(rename = "logs")]
    logs: Vec<serde_json::Value>,
    #[serde(rename = "next_cursor")]
    _next_cursor: Option<CursorShape>,
}

/// Duplicate-aware exact nonterminal cursor.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorShape {
    #[serde(rename = "time")]
    _time: serde_json::Value,
    #[serde(rename = "id")]
    _id: serde_json::Value,
}

/// Executes one issue-directed investigation without mutating server state.
pub async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    issue_id: &str,
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
    let issue_path = format!("/api/telemetry/issues/{}", encode_component(issue_id));
    let issue_body = get_body(&client, env, origin.as_str(), issue_path.as_str()).await?;
    let _issue_shape =
        serde_json::from_str::<IssueShape>(issue_body.as_str()).map_err(|_| invalid_response())?;
    let issue = serde_json::from_str::<serde_json::Value>(issue_body.as_str())
        .map_err(|_| invalid_response())?;
    let (scope, route) = validate_issue(&issue, issue_id)?;
    let follow_path = follow_path(&scope, &route);
    let result_body = get_body(&client, env, origin.as_str(), follow_path.as_str()).await?;
    let result = validate_result(result_body.as_str(), &scope, &route)?;

    if json {
        let next_action = issue
            .get("next_action")
            .cloned()
            .ok_or_else(invalid_response)?;
        let (trace_summary, related_logs) = match route {
            InvestigationRoute::Trace { .. } => (result, serde_json::Value::Null),
            InvestigationRoute::RelatedLogs => (serde_json::Value::Null, result),
        };
        let body = serde_json::json!({
            "issue": issue,
            "next_action": next_action,
            "trace_summary": trace_summary,
            "related_logs": related_logs,
        });
        writeln!(output, "{body}")?;
    } else {
        write_human(output, &route, &result)?;
    }
    Ok(())
}

/// Sends one authenticated GET and returns a bounded success body.
async fn get_body(
    client: &reqwest::Client,
    env: &CliEnvironment,
    origin: &str,
    path: &str,
) -> Result<String, RuntimeError> {
    let url = format!("{origin}{path}");
    let response = send_authenticated_with_refresh(client, env, |client, credential| {
        client.get(url.as_str()).bearer_auth(credential.token())
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(safe_api_error(status, &credential));
    }
    bounded_body(response).await
}

/// Reads a response incrementally without retaining oversized data.
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

/// Converts transport and refresh failures into fixed, path-free recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Api {
            status,
            auth_source,
            auth_label,
            ..
        } => fixed_api_error(status, auth_source, auth_label),
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_error(),
    }
}

/// Validates the issue identity, exact shape, scope, and directed action pair.
fn validate_issue<'a>(
    issue: &'a serde_json::Value,
    requested_issue_id: &str,
) -> Result<(IssueScope<'a>, InvestigationRoute<'a>), RuntimeError> {
    let object = issue.as_object().ok_or_else(invalid_response)?;
    let id = required_raw_string(object, "id")?;
    if id != requested_issue_id || !is_canonical_uuid(id) {
        return Err(invalid_response());
    }
    let project_id = required_scope(object, "project_id", false)?;
    if !is_canonical_uuid(project_id) {
        return Err(invalid_response());
    }
    let _fingerprint = required_raw_string(object, "fingerprint")?;
    let _severity = required_scope(object, "severity", false)?;
    let _title = required_raw_string(object, "title")?;
    let _message = required_raw_string(object, "message")?;
    validate_optional_string(object, "stack_trace")?;
    if !object
        .get("attributes")
        .is_some_and(serde_json::Value::is_object)
    {
        return Err(invalid_response());
    }
    validate_optional_object(object, "symbolication")?;
    let service_name = required_scope(object, "service_name", true)?;
    let release = required_scope(object, "release", true)?;
    let environment = required_scope(object, "environment", true)?;
    let first_seen_at = required_timestamp(object, "first_seen_at")?;
    let _last_seen_at = required_timestamp(object, "last_seen_at")?;
    let _occurrence_count = required_count(object, "occurrence_count")?;
    if !matches!(
        required_scope(object, "status", false)?,
        "unresolved" | "resolved" | "ignored"
    ) {
        return Err(invalid_response());
    }
    let trace_id = optional_string(object, "trace_id")?;
    let action = object
        .get("next_action")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid_response)?;
    let code = required_scope(action, "code", false)?;
    let target = required_scope(action, "target", false)?;
    let route = match (code, target) {
        ("inspect_trace", "trace_summary") => {
            let trace_id = trace_id
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(invalid_response)?;
            if !safe_scope(trace_id) || matches!(trace_id, "." | "..") {
                return Err(invalid_response());
            }
            InvestigationRoute::Trace { trace_id }
        }
        ("inspect_related_logs", "telemetry_logs") => {
            if trace_id.is_some_and(|value| !value.trim().is_empty())
                || [service_name, release, environment]
                    .iter()
                    .any(|value| value.trim().is_empty())
            {
                return Err(invalid_response());
            }
            InvestigationRoute::RelatedLogs
        }
        _ => return Err(invalid_response()),
    };
    Ok((
        IssueScope {
            project_id,
            service_name,
            release,
            environment,
            first_seen_at,
        },
        route,
    ))
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
                ("release", nonblank(scope.release)),
                ("environment", nonblank(scope.environment)),
            ],
        ),
        InvestigationRoute::RelatedLogs => path_with_query(
            "/api/logs",
            &[
                ("project_id", Some(scope.project_id)),
                ("service_name", Some(scope.service_name)),
                ("release", Some(scope.release)),
                ("environment", Some(scope.environment)),
                ("since", Some(scope.first_seen_at)),
                ("limit", Some("25")),
            ],
        ),
    }
}

/// Parses and validates the exact response shape for one directed result.
fn validate_result(
    body: &str,
    scope: &IssueScope<'_>,
    route: &InvestigationRoute<'_>,
) -> Result<serde_json::Value, RuntimeError> {
    match route {
        InvestigationRoute::Trace { trace_id } => {
            let _shape =
                serde_json::from_str::<TraceSummaryShape>(body).map_err(|_| invalid_response())?;
            let value =
                serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_response())?;
            validate_trace_summary(&value, trace_id)?;
            Ok(value)
        }
        InvestigationRoute::RelatedLogs => {
            let shape = serde_json::from_str::<LogsShape>(body).map_err(|_| invalid_response())?;
            let value =
                serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_response())?;
            validate_logs_shape(&shape, &value, scope)?;
            Ok(value)
        }
    }
}

/// Validates trace identity and the public scalar/container types used by humans.
fn validate_trace_summary(
    value: &serde_json::Value,
    expected_trace_id: &str,
) -> Result<(), RuntimeError> {
    let object = value.as_object().ok_or_else(invalid_response)?;
    if required_raw_string(object, "trace_id")? != expected_trace_id {
        return Err(invalid_response());
    }
    for key in [
        "span_count",
        "error_span_count",
        "service_count",
        "project_count",
        "duration_ms",
    ] {
        let _count = required_count(object, key)?;
    }
    let _started_at = required_timestamp(object, "started_at")?;
    Ok(())
}

/// Validates one bounded bare-array or exact cursor-envelope log response.
fn validate_logs_shape(
    shape: &LogsShape,
    value: &serde_json::Value,
    scope: &IssueScope<'_>,
) -> Result<(), RuntimeError> {
    let rows = match shape {
        LogsShape::Array(rows) => rows.as_slice(),
        LogsShape::Envelope(envelope) => envelope.logs.as_slice(),
    };
    if rows.len() > 25 || !rows.iter().all(|row| valid_log_row(row, scope)) {
        return Err(invalid_response());
    }
    if let Some(envelope) = value.as_object() {
        let cursor = envelope.get("next_cursor").ok_or_else(invalid_response)?;
        if !valid_cursor(cursor) {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Binds one returned log row to the directed incident scope.
fn valid_log_row(value: &serde_json::Value, scope: &IssueScope<'_>) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if required_raw_string(object, "service_name").ok() != Some(scope.service_name)
        || required_raw_string(object, "release").ok() != Some(scope.release)
        || required_raw_string(object, "environment").ok() != Some(scope.environment)
        || required_timestamp(object, "timestamp").is_err()
    {
        return false;
    }
    match object.get("project_id") {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(project_id)) => project_id == scope.project_id,
        Some(_) => false,
    }
}

/// Percent-encodes one trace path segment, including URL dot segments.
fn encode_trace_segment(trace_id: &str) -> String {
    encode_component(trace_id).replace('.', "%2E")
}

/// Writes a bounded summary without echoing identifiers or telemetry payload text.
fn write_human<W: std::io::Write>(
    output: &mut W,
    route: &InvestigationRoute<'_>,
    result: &serde_json::Value,
) -> Result<(), RuntimeError> {
    writeln!(output, "Issue investigation")?;
    writeln!(output, "Route: {} -> {}", route.code(), route.target())?;
    match route {
        InvestigationRoute::Trace { .. } => {
            let object = result.as_object().ok_or_else(invalid_response)?;
            writeln!(
                output,
                "Trace summary: spans={} errors={} services={} duration={}ms",
                required_count(object, "span_count")?,
                required_count(object, "error_span_count")?,
                required_count(object, "service_count")?,
                required_count(object, "duration_ms")?,
            )?;
            writeln!(
                output,
                "Next: rerun this command with --json to inspect full public trace fields."
            )?;
        }
        InvestigationRoute::RelatedLogs => {
            writeln!(
                output,
                "Related logs: {} (first page)",
                log_count(result).ok_or_else(invalid_response)?
            )?;
            writeln!(
                output,
                "Next: rerun this command with --json to inspect full public log fields."
            )?;
        }
    }
    Ok(())
}

/// Returns a nonblank string as an optional query parameter.
fn nonblank(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

/// Returns the validated row count from either supported log response shape.
fn log_count(value: &serde_json::Value) -> Option<usize> {
    value
        .as_array()
        .map(Vec::len)
        .or_else(|| value.as_object()?.get("logs")?.as_array().map(Vec::len))
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
    crate::render::is_rfc3339_utc(time) && is_canonical_uuid(id)
}

/// Extracts one required response string without rendering it.
fn required_raw_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, RuntimeError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid_response)
}

/// Extracts one bounded, control-safe scope string.
fn required_scope<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
    allow_blank: bool,
) -> Result<&'a str, RuntimeError> {
    required_raw_string(object, key).and_then(|value| {
        (safe_scope(value) && (allow_blank || !value.trim().is_empty()))
            .then_some(value)
            .ok_or_else(invalid_response)
    })
}

/// Extracts one optional string, accepting omission or null.
fn optional_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match object.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.as_str())),
        Some(_) => Err(invalid_response()),
    }
}

/// Validates one optional string field without retaining it.
fn validate_optional_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<(), RuntimeError> {
    optional_string(object, key).map(|_| ())
}

/// Validates one optional object field without retaining it.
fn validate_optional_object(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<(), RuntimeError> {
    match object.get(key) {
        None | Some(serde_json::Value::Null | serde_json::Value::Object(_)) => Ok(()),
        Some(_) => Err(invalid_response()),
    }
}

/// Extracts one required UTC RFC3339 timestamp.
fn required_timestamp<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, RuntimeError> {
    let value = required_raw_string(object, key)?;
    crate::render::is_rfc3339_utc(value)
        .then_some(value)
        .ok_or_else(invalid_response)
}

/// Extracts one required unsigned integer count.
fn required_count(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<u64, RuntimeError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(invalid_response)
}

/// Returns whether a response UUID uses the canonical lowercase dashed form.
fn is_canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23)
                    && (byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
}

/// Returns whether a scope can be safely sent as a single query value.
fn safe_scope(value: &str) -> bool {
    value.chars().count() <= SCOPE_LIMIT && !has_unsafe_text(value)
}

/// Rejects controls and display-direction characters in request scope.
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
    fixed_api_error(status, credential.source(), credential.label())
}

/// Builds one fixed API error without retaining an upstream response body.
fn fixed_api_error(
    status: u16,
    auth_source: &'static str,
    auth_label: &'static str,
) -> RuntimeError {
    let (error, code, next, action_code, action_target) = match status {
        400 | 422 => (
            "investigation request rejected",
            "validation_failed",
            "retry the issue investigation; if it repeats, report the public response contract",
            "fix_request",
            "request",
        ),
        401 => (
            "authentication required",
            "unauthorized",
            "run logbrew login",
            "sign_in",
            "auth",
        ),
        403 => (
            "investigation request forbidden",
            "forbidden",
            "confirm account access and retry the issue investigation",
            "check_access",
            "auth",
        ),
        404 => (
            "investigation data not found",
            "not_found",
            "refresh the issue id and retry the investigation",
            "check_resource",
            "resource",
        ),
        405 => (
            "investigation method is not supported",
            "method_not_allowed",
            "retry the read-only issue investigation",
            "use_supported_method",
            "api_method",
        ),
        429 => (
            "investigation request rate limited",
            "rate_limited",
            "retry the same issue investigation later",
            "retry_later",
            "issue",
        ),
        500..=599 => (
            "investigation service unavailable",
            "service_unavailable",
            "retry the same issue investigation later",
            "retry_later",
            "issue",
        ),
        _ => (
            "investigation request failed",
            "request_failed",
            "check account access and retry the issue investigation",
            "retry_investigation",
            "issue",
        ),
    };
    RuntimeError::Api {
        status,
        body: serde_json::json!({
            "error": error,
            "code": code,
            "next": next,
            "next_action": {"code": action_code, "target": action_target}
        })
        .to_string(),
        auth_source,
        auth_label,
    }
}
