//! Strict authenticated account identity reads.

use super::{AuthCredential, send_account_authenticated_with_refresh};
use crate::{CliEnvironment, RuntimeError};

/// Maximum accepted account identity response body.
const RESPONSE_LIMIT: usize = 768 * 1024;
const MAX_AVATAR_ENCODED_BYTES: usize = 512 * 1024 * 4 / 3 + 4;

/// Duplicate-aware exact deployed account shape.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityShape {
    id: String,
    email: String,
    display_name: String,
    first_name: String,
    last_name: String,
    avatar_data_url: Option<String>,
    tier: String,
}

/// Duplicate-aware standard API error envelope.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorShape {
    /// Human-readable error field.
    #[serde(rename = "error")]
    _error: serde_json::Value,
    /// Stable error-code field.
    #[serde(rename = "code")]
    _code: serde_json::Value,
    /// Recovery guidance field.
    #[serde(rename = "next")]
    _next: serde_json::Value,
    /// Typed recovery-action field.
    #[serde(rename = "next_action")]
    _next_action: ErrorActionShape,
}

/// Duplicate-aware standard API error action.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorActionShape {
    /// Stable action-code field.
    #[serde(rename = "code")]
    _code: serde_json::Value,
    /// Stable action-target field.
    #[serde(rename = "target")]
    _target: serde_json::Value,
}

/// Duplicate-aware recovery-available API error envelope.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryErrorShape {
    /// Human-readable error field.
    #[serde(rename = "error")]
    _error: serde_json::Value,
    /// Stable error-code field.
    #[serde(rename = "code")]
    _code: serde_json::Value,
    /// Recovery guidance field.
    #[serde(rename = "next")]
    _next: serde_json::Value,
    /// Typed recovery-action field.
    #[serde(rename = "next_action")]
    _next_action: ErrorActionShape,
    /// Account deletion timestamp field.
    #[serde(rename = "deleted_at")]
    _deleted_at: serde_json::Value,
    /// Private one-time recovery token field.
    #[serde(rename = "recovery_token")]
    _recovery_token: serde_json::Value,
}

/// Executes one strict authenticated account identity read.
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let origin = super::normalized_api_base(env.base_url.as_str())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| transport_error())?;
    let url = format!("{origin}/api/auth/account");
    let response = send_account_authenticated_with_refresh(&client, env, |client, credential| {
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
    let identity =
        serde_json::from_str::<IdentityShape>(body.as_str()).map_err(|_| invalid_response())?;
    validate_identity(&identity)?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        write_human(&identity, output)?;
    }
    Ok(())
}

/// Converts auth and transport failures into fixed, value-safe recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    if matches!(
        &error,
        RuntimeError::Unavailable {
            message: "account authentication is required",
            ..
        }
    ) {
        return account_auth_error();
    }
    error.auth_or(transport_error())
}

/// Validates the exact account identity contract before any output.
fn validate_identity(identity: &IdentityShape) -> Result<(), RuntimeError> {
    (crate::ids::is_uuid(identity.id.as_str())
        && safe_text(identity.email.as_str(), 320, false)
        && safe_text(identity.display_name.as_str(), 200, false)
        && safe_text(identity.first_name.as_str(), 200, true)
        && safe_text(identity.last_name.as_str(), 200, true)
        && safe_avatar(identity.avatar_data_url.as_deref())
        && safe_text(identity.tier.as_str(), 64, false))
    .then_some(())
    .ok_or_else(invalid_response)
}

/// Validates a duplicate-aware standard error before replacing its text.
fn validate_error(
    status: u16,
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    if status == 409 {
        return validate_recovery_error(body, credential);
    }
    let _shape = serde_json::from_str::<ErrorShape>(body).map_err(|_| invalid_response())?;
    let value = serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_response())?;
    let object = value.as_object().ok_or_else(invalid_response)?;
    let _error = safe_string(object.get("error"), 512)?;
    let code = safe_string(object.get("code"), 64)?;
    let _next = safe_string(object.get("next"), 512)?;
    let action = object
        .get("next_action")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid_response)?;
    let action_code = safe_string(action.get("code"), 64)?;
    let action_target = safe_string(action.get("target"), 64)?;
    let typed = match status {
        401 => code == "unauthorized" && action_code == "sign_in" && action_target == "auth",
        403 => code == "forbidden" && action_code == "request_access" && action_target == "auth",
        410 => {
            code == "account_recovery_expired"
                && action_code == "create_account"
                && action_target == "auth"
        }
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

/// Validates and redacts the recovery-available response with its private token.
fn validate_recovery_error(
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    let _shape =
        serde_json::from_str::<RecoveryErrorShape>(body).map_err(|_| invalid_response())?;
    let value = serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_response())?;
    let object = value.as_object().ok_or_else(invalid_response)?;
    let _error = safe_string(object.get("error"), 512)?;
    let code = safe_string(object.get("code"), 64)?;
    let _next = safe_string(object.get("next"), 512)?;
    let _deleted_at = safe_string(object.get("deleted_at"), 64)?;
    let _recovery_token = safe_string(object.get("recovery_token"), 4096)?;
    let action = object
        .get("next_action")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid_response)?;
    let action_code = safe_string(action.get("code"), 64)?;
    let action_target = safe_string(action.get("target"), 64)?;
    if code != "account_recovery_available"
        || action_code != "restore_account"
        || action_target != "account_recovery"
    {
        return Err(invalid_response());
    }
    Ok(RuntimeError::Api {
        status: 409,
        body: safe_error_body(409),
        auth_source: credential.source(),
        auth_label: credential.label(),
    })
}

/// Writes bounded human account identity output.
fn write_human<W: std::io::Write>(
    identity: &IdentityShape,
    output: &mut W,
) -> Result<(), std::io::Error> {
    writeln!(output, "Account")?;
    writeln!(output, "- id: {}", identity.id)?;
    writeln!(output, "- email: {}", identity.email)?;
    writeln!(output, "- name: {}", identity.display_name)?;
    writeln!(output, "- tier: {}", identity.tier)?;
    writeln!(output, "Next: run logbrew projects")
}

fn safe_text(value: &str, limit: usize, allow_empty: bool) -> bool {
    value.len() <= limit
        && value.trim() == value
        && (allow_empty || !value.is_empty())
        && value.chars().all(|character| !character.is_control())
}

fn safe_string(value: Option<&serde_json::Value>, limit: usize) -> Result<&str, RuntimeError> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| safe_text(value, limit, false))
        .ok_or_else(invalid_response)
}

fn safe_avatar(value: Option<&str>) -> bool {
    let Some(value) = value else { return true };
    let encoded = [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/webp;base64,",
    ]
    .into_iter()
    .find_map(|prefix| value.strip_prefix(prefix));
    let Some(encoded) = encoded else { return false };
    let unpadded = encoded.trim_end_matches('=');
    !encoded.is_empty()
        && encoded.len() <= MAX_AVATAR_ENCODED_BYTES
        && encoded.len().is_multiple_of(4)
        && encoded.len() - unpadded.len() <= 2
        && unpadded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

/// Returns a fixed local API error body based only on validated status.
fn safe_error_body(status: u16) -> String {
    match status {
        401 => serde_json::json!({
            "error": "Authentication required",
            "code": "unauthorized",
            "next": "run logbrew login",
        }),
        403 => serde_json::json!({
            "error": "Account access is not available",
            "code": "forbidden",
            "next": "run logbrew login",
        }),
        405 => serde_json::json!({
            "error": "Account identity method is not supported",
            "code": "method_not_allowed",
            "next": "retry logbrew whoami",
        }),
        409 => serde_json::json!({
            "error": "Account recovery is available",
            "code": "account_recovery_available",
            "next": "complete account recovery in LogBrew, then run logbrew login",
        }),
        410 => serde_json::json!({
            "error": "Account recovery window expired",
            "code": "account_recovery_expired",
            "next": "run logbrew login to create a new account",
        }),
        500..=599 => serde_json::json!({
            "error": "Account identity is temporarily unavailable",
            "code": "server_error",
            "next": "retry logbrew whoami later",
        }),
        _ => serde_json::json!({
            "error": "Account identity request failed",
            "code": "unexpected_response",
            "next": "retry logbrew whoami",
        }),
    }
    .to_string()
}

/// Returns a stable account-only credential error.
const fn account_auth_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "account authentication is required",
        next: "run logbrew login and retry logbrew whoami",
    }
}

/// Returns stable invalid-response recovery without server content.
const fn invalid_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "account identity response was invalid",
        next: "retry logbrew whoami; if it repeats, report the public response contract",
    }
}

/// Returns stable request recovery without configured host details.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "account identity request could not be completed",
        next: "check network connectivity and retry logbrew whoami",
    }
}
