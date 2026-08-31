//! Upload receipt and exact lookup wire contract.

use super::artifact::MAX_ARTIFACT_BYTES;
use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::{CliEnvironment, NativeDebugLookupOptions, RuntimeError};

/// Maximum success response retained from the server.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
/// Exact accepted upload guidance.
const UPLOAD_NEXT: &str =
    "Native debug artifact upload accepted. Verify exact image UUID and architecture lookup.";
/// Exact successful lookup guidance.
const LOOKUP_FOUND_NEXT: &str =
    "Native debug artifact lookup matched. Verify issue-detail native symbolication.";
/// Exact missing lookup guidance.
const LOOKUP_MISSING_NEXT: &str =
    "No exact native debug artifact matched. Upload the release debug file and retry lookup.";

/// Validated upload receipt needed by orchestration and output.
pub(super) struct UploadReceipt {
    /// Public upload identifier.
    pub(super) upload_id: String,
    /// Exact accepted artifact count.
    pub(super) artifact_count: u64,
}

/// Validated lookup result.
pub(super) enum LookupResult {
    /// One exact matching artifact.
    Found(LookupArtifact),
    /// Valid terminal absence.
    Missing,
}

/// Validated public artifact returned by lookup.
pub(super) struct LookupArtifact {
    /// Public artifact identifier.
    pub(super) artifact_id: String,
    /// Public upload identifier.
    pub(super) upload_id: String,
    /// Canonical image UUID.
    pub(super) image_uuid: String,
    /// Canonical architecture.
    pub(super) architecture: String,
    /// Exact native artifact family.
    pub(super) artifact_type: String,
    /// Lowercase SHA-256.
    pub(super) debug_file_sha256: String,
    /// Positive bounded payload size.
    pub(super) debug_file_byte_size: u64,
    /// Fixed upload status.
    pub(super) upload_status: String,
    /// RFC3339 UTC creation time.
    pub(super) created_at: String,
}

/// Sends and validates one exact lookup.
pub(super) async fn lookup(
    client: &reqwest::Client,
    env: &CliEnvironment,
    mut url: reqwest::Url,
    options: &NativeDebugLookupOptions,
) -> Result<LookupResult, RuntimeError> {
    {
        let _query = url
            .query_pairs_mut()
            .clear()
            .append_pair("project_id", options.project_id.as_str())
            .append_pair("release", options.release.as_str())
            .append_pair("environment", options.environment.as_str())
            .append_pair("service", options.service.as_str())
            .append_pair("image_uuid", options.image_uuid.as_str())
            .append_pair("architecture", options.architecture.as_str());
    }
    let response = send_account_authenticated_with_refresh(client, env, |client, credential| {
        client.get(url.clone()).bearer_auth(credential.token())
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(safe_api_error(status, &credential));
    }
    let body = bounded_body(response).await?;
    parse_lookup_response(body.as_str(), options)
}

/// Creates the fixed native debug-artifact API URL without retaining private path state.
pub(super) fn native_artifact_url(base_url: &str) -> Result<reqwest::Url, RuntimeError> {
    api_url(base_url, "/api/native-debug-artifacts")
}

/// Creates one fixed native debug-artifact API URL from a validated public path.
pub(super) fn api_url(base_url: &str, path: &str) -> Result<reqwest::Url, RuntimeError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| transport_error())?;
    let secure_transport = url.scheme() == "https"
        || url.scheme() == "http" && url.host_str().is_some_and(is_loopback_host);
    if !secure_transport
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(transport_error());
    }
    url.set_path(path);
    url.set_query(None);
    Ok(url)
}

/// Allows plaintext transport only for installed local loopback proof.
fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// Exact upload success surface.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadResponse {
    /// Public upload identifier.
    upload_id: String,
    /// Fixed uploaded state.
    status: String,
    /// Exact number of accepted artifact identities.
    artifact_count: u64,
    /// Fixed public guidance.
    next: String,
    /// Fixed lookup-verification action.
    next_action: NextAction,
}

/// Exact two-key next action.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NextAction {
    /// Stable action code.
    code: String,
    /// Stable action target.
    target: String,
}

/// Parses and binds the exact upload response.
pub(super) fn parse_upload_response(
    body: &str,
    expected_count: usize,
) -> Result<UploadReceipt, RuntimeError> {
    let response = serde_json::from_str::<UploadResponse>(body).map_err(|_| invalid_response())?;
    if !is_public_id(response.upload_id.as_str(), "nativeart_")
        || response.status != "uploaded"
        || usize::try_from(response.artifact_count).ok() != Some(expected_count)
        || response.next != UPLOAD_NEXT
        || response.next_action.code != "verify_native_debug_artifact_lookup"
        || response.next_action.target != "native_debug_artifact_lookup"
    {
        return Err(invalid_response());
    }
    Ok(UploadReceipt {
        upload_id: response.upload_id,
        artifact_count: response.artifact_count,
    })
}

/// Exact lookup response envelope.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupResponse {
    /// Exact artifact or null.
    artifact: Option<LookupArtifactDto>,
    /// Fixed public guidance matching found state.
    next: String,
    /// Fixed action matching found state.
    next_action: NextAction,
}

/// Exact public artifact DTO returned by lookup.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupArtifactDto {
    /// Public artifact identifier.
    artifact_id: String,
    /// Public upload identifier.
    upload_id: String,
    /// Account-owned project UUID.
    project_id: String,
    /// Exact release scope.
    release: String,
    /// Exact environment scope.
    environment: String,
    /// Exact service scope.
    service: String,
    /// Fixed artifact type.
    artifact_type: String,
    /// Canonical image UUID.
    image_uuid: String,
    /// Canonical architecture.
    architecture: String,
    /// Lowercase SHA-256.
    debug_file_sha256: String,
    /// Positive bounded payload size.
    debug_file_byte_size: u64,
    /// Fixed upload status.
    upload_status: String,
    /// RFC3339 UTC creation time.
    created_at: String,
}

/// Parses exact found/missing lookup surfaces and binds request context.
fn parse_lookup_response(
    body: &str,
    options: &NativeDebugLookupOptions,
) -> Result<LookupResult, RuntimeError> {
    let response = serde_json::from_str::<LookupResponse>(body).map_err(|_| invalid_response())?;
    if let Some(artifact) = response.artifact {
        if response.next != LOOKUP_FOUND_NEXT
            || response.next_action.code != "verify_native_issue_symbolication"
            || response.next_action.target != "native_issue_symbolication"
            || !valid_lookup_artifact(&artifact, options)
        {
            return Err(invalid_response());
        }
        Ok(LookupResult::Found(LookupArtifact {
            artifact_id: artifact.artifact_id,
            upload_id: artifact.upload_id,
            image_uuid: artifact.image_uuid,
            architecture: artifact.architecture,
            artifact_type: artifact.artifact_type,
            debug_file_sha256: artifact.debug_file_sha256,
            debug_file_byte_size: artifact.debug_file_byte_size,
            upload_status: artifact.upload_status,
            created_at: artifact.created_at,
        }))
    } else {
        if response.next != LOOKUP_MISSING_NEXT
            || response.next_action.code != "upload_native_debug_artifact"
            || response.next_action.target != "native_debug_artifact_upload"
        {
            return Err(invalid_response());
        }
        Ok(LookupResult::Missing)
    }
}

/// Validates one found artifact against exact requested context and identity.
fn valid_lookup_artifact(artifact: &LookupArtifactDto, options: &NativeDebugLookupOptions) -> bool {
    is_public_id(artifact.artifact_id.as_str(), "nativeartifact_")
        && is_public_id(artifact.upload_id.as_str(), "nativeart_")
        && artifact.project_id == options.project_id
        && artifact.release == options.release
        && artifact.environment == options.environment
        && artifact.service == options.service
        && matches!(
            artifact.artifact_type.as_str(),
            "apple_dsym" | "android_elf"
        )
        && artifact.image_uuid == options.image_uuid
        && artifact.architecture == options.architecture
        && is_lower_hex(artifact.debug_file_sha256.as_str(), 64)
        && artifact.debug_file_byte_size > 0
        && artifact.debug_file_byte_size <= u64::try_from(MAX_ARTIFACT_BYTES).unwrap_or(u64::MAX)
        && artifact.upload_status == "uploaded"
        && crate::render::is_rfc3339_utc(artifact.created_at.as_str())
}

/// Restricts public IDs to their exact prefix plus 32 lowercase hex bytes.
fn is_public_id(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|raw| is_lower_hex(raw, 32))
}

/// Checks an exact-length lowercase hexadecimal string.
fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Reads one bounded response without retaining hostile text on failure.
pub(super) async fn bounded_body(response: reqwest::Response) -> Result<String, RuntimeError> {
    crate::http::bounded_body(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => invalid_response(),
            crate::http::BodyError::Transport => transport_error(),
        })
}

/// Converts auth, refresh, transport, and body errors to fixed local recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    if let RuntimeError::Api {
        status,
        auth_source,
        auth_label,
        ..
    } = error
    {
        RuntimeError::Api {
            status,
            body: safe_api_body(status),
            auth_source,
            auth_label,
        }
    } else {
        error.auth_or(transport_error())
    }
}

/// Builds a fixed status-derived API error without server text.
fn safe_api_error(status: u16, credential: &AuthCredential) -> RuntimeError {
    RuntimeError::Api {
        status,
        body: safe_api_body(status),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

/// Produces a fixed, value-safe API envelope from status only.
fn safe_api_body(status: u16) -> String {
    let (error, code, next, action_code, target) = match status {
        400 => (
            "native debug-artifact request was rejected",
            "validation_failed",
            "check the artifact identity and request scope, then retry",
            "fix_request",
            "request",
        ),
        422 => (
            "native debug-artifact request was rejected",
            "validation_failed",
            "check the native debug-artifact request fields and retry",
            "fix_request",
            "request",
        ),
        401 | 403 => (
            "authentication is required",
            "unauthorized",
            "sign in and retry the native debug-artifact command",
            "sign_in",
            "auth",
        ),
        404 => (
            "native debug artifact was not found",
            "not_found",
            "check the exact project, release, environment, service, UUID, and architecture",
            "check_resource",
            "resource",
        ),
        413 => (
            "native debug-artifact payload is too large",
            "payload_too_large",
            "reduce the native debug-artifact upload below the documented size limits and retry",
            "reduce_artifact_size",
            "native_debug_artifact_upload",
        ),
        429 => (
            "native debug-artifact request is temporarily limited",
            "rate_limited",
            "retry the same native debug-artifact command later",
            "retry_later",
            "request",
        ),
        500..=599 => (
            "native debug-artifact service is unavailable",
            "server_error",
            "retry the same native debug-artifact command later",
            "retry_later",
            "request",
        ),
        _ => (
            "native debug-artifact request returned an unexpected status",
            "unexpected_response",
            "retry the native debug-artifact command",
            "retry_request",
            "request",
        ),
    };
    serde_json::json!({
        "error": error,
        "code": code,
        "next": next,
        "next_action": {"code": action_code, "target": target}
    })
    .to_string()
}

/// Returns the fixed path-free response contract error.
const fn invalid_response() -> RuntimeError {
    RuntimeError::NativeDebugResponseInvalid
}

/// Returns a fixed URL-free transport error.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "native debug-artifact request could not be completed",
        next: "check network connectivity and retry the native debug-artifact command",
    }
}
