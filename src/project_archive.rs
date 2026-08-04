//! Strict, explicitly confirmed project archival.

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::ids::is_uuid;
use crate::{CliEnvironment, CliError, RuntimeError};
use serde::{Deserialize, Serialize};

/// Maximum accepted archive response body.
const RESPONSE_LIMIT: usize = 64 * 1024;

/// Exact successful machine-readable archive result.
#[derive(Serialize)]
struct ArchiveSuccess<'a> {
    /// Stable success signal.
    ok: bool,
    /// Canonical archived project UUID.
    project_id: &'a str,
    /// Stable lifecycle result.
    status: &'static str,
    /// Deterministic active-catalog recovery.
    next: &'static str,
}

/// Strict backend error envelope.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorEnvelope {
    /// Human backend error, validated and then discarded.
    error: String,
    /// Stable backend error code.
    code: String,
    /// Human backend recovery, validated and then discarded.
    next: String,
    /// Typed backend recovery metadata.
    next_action: ErrorAction,
}

/// Strict typed backend recovery metadata.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorAction {
    /// Stable action code.
    code: String,
    /// Stable action target.
    target: String,
}

/// Executes one strict account-authenticated project archive.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !is_uuid(project_id) {
        return Err(CliError::InvalidProjectArchiveCommand.into());
    }
    let project_id = project_id.to_ascii_lowercase();
    let origin = normalized_origin(env.base_url.as_str())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| transport_error())?;
    let url = format!("{origin}/api/projects/{project_id}");
    let response = send_account_authenticated_with_refresh(&client, env, |client, credential| {
        client.delete(url.as_str()).bearer_auth(credential.token())
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    let body = bounded_body(response).await?;

    if status != 204 {
        return Err(validate_error(status, body.as_str(), &credential)?);
    }
    if !body.is_empty() {
        return Err(invalid_response());
    }
    write_success(project_id.as_str(), json, output)?;
    Ok(())
}

/// Reads one response without retaining oversized server content.
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

/// Converts request failures into fixed, host-free archive recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::Unavailable {
            message: "account authentication is required",
            ..
        } => account_auth_error(),
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::Api { .. }
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::ExplainResponseInvalid
        | RuntimeError::AnalyticsResponseInvalid
        | RuntimeError::AnalyticsFunnelResponseInvalid
        | RuntimeError::AnalyticsRetentionResponseInvalid
        | RuntimeError::AnalyticsLifecycleResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_error(),
    }
}

/// Validates one typed backend error before replacing all server text.
fn validate_error(
    status: u16,
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    let envelope = serde_json::from_str::<ErrorEnvelope>(body).map_err(|_| invalid_response())?;
    if !bounded_safe_text(envelope.error.as_str(), 512)
        || !bounded_safe_text(envelope.code.as_str(), 64)
        || !bounded_safe_text(envelope.next.as_str(), 512)
        || !bounded_safe_text(envelope.next_action.code.as_str(), 64)
        || !bounded_safe_text(envelope.next_action.target.as_str(), 64)
    {
        return Err(invalid_response());
    }
    let typed = matches!(
        (
            status,
            envelope.code.as_str(),
            envelope.next_action.code.as_str(),
            envelope.next_action.target.as_str(),
        ),
        (401, "unauthorized", "sign_in", "auth")
            | (403, "forbidden", "request_access", "auth")
            | (404, "not_found", "check_resource", "resource")
            | (
                405,
                "method_not_allowed",
                "use_supported_method",
                "api_method"
            )
            | (
                500..=599,
                "storage_error",
                "retry_or_check_storage",
                "backend_status",
            )
            | (500..=599, "json_error", "send_valid_json", "request")
            | (
                500..=599,
                "internal_error",
                "retry_or_contact_support",
                "support",
            )
    );
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

/// Writes a deterministic archive confirmation without server-provided text.
fn write_success<W: std::io::Write>(
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if json {
        let result = ArchiveSuccess {
            ok: true,
            project_id,
            status: "archived",
            next: "run logbrew projects --json",
        };
        serde_json::to_writer(&mut *output, &result).map_err(|_| output_error())?;
        writeln!(output)?;
        return Ok(());
    }
    writeln!(output, "Project archived: {project_id}")?;
    writeln!(output, "Project ingest keys: disabled")?;
    writeln!(output, "Next: run logbrew projects")?;
    Ok(())
}

/// Normalizes an HTTP(S) API origin without credentials, query, or fragment.
fn normalized_origin(value: &str) -> Result<String, RuntimeError> {
    let mut parsed = reqwest::Url::parse(value).map_err(|_| transport_error())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(transport_error());
    }
    parsed.set_path("");
    Ok(parsed.to_string().trim_end_matches('/').to_owned())
}

/// Returns whether text is non-empty, bounded, and display-safe.
fn bounded_safe_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= limit
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

/// Returns a synthetic allowlisted API error body.
fn safe_error_body(status: u16) -> String {
    let value = match status {
        401 => serde_json::json!({
            "error": "account authentication is invalid",
            "code": "unauthorized",
            "next": "run logbrew login",
            "next_action": {"code": "sign_in", "target": "auth"}
        }),
        403 => serde_json::json!({
            "error": "project archive access is forbidden",
            "code": "forbidden",
            "next": "use an account that owns the project",
            "next_action": {"code": "request_access", "target": "auth"}
        }),
        404 => serde_json::json!({
            "error": "active project was not found",
            "code": "not_found",
            "next": "refresh the active project list and check project_id",
            "next_action": {"code": "check_resource", "target": "resource"}
        }),
        405 => serde_json::json!({
            "error": "project archive method is not supported",
            "code": "method_not_allowed",
            "next": "retry with logbrew projects archive",
            "next_action": {"code": "use_supported_method", "target": "api_method"}
        }),
        _ => serde_json::json!({
            "error": "project archive could not be confirmed",
            "code": "server_error",
            "next": "retry the same project archive command later",
            "next_action": {"code": "retry", "target": "request"}
        }),
    };
    value.to_string()
}

/// Returns stable account-auth recovery for this command.
const fn account_auth_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "account authentication is required",
        next: "run logbrew login and retry the project archive command",
    }
}

/// Returns fixed archive response recovery without retaining server content.
const fn invalid_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project archive response was invalid",
        next: "refresh the active project list before retrying the archive command",
    }
}

/// Returns fixed archive network recovery without a URL.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project archive request could not be completed",
        next: "check network connectivity and retry the same archive command",
    }
}

/// Returns a fixed output serialization failure.
fn output_error() -> RuntimeError {
    RuntimeError::Io(std::io::Error::other(
        "project archive output could not be written",
    ))
}
