//! Multipart and exact lookup wire contract.

use super::artifact::{Artifact, MAX_ARTIFACT_BYTES};
use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{CliEnvironment, NativeDebugLookupOptions, NativeDebugUploadOptions, RuntimeError};

/// Maximum encoded multipart request size.
const MAX_MULTIPART_BYTES: usize = 128 * 1024 * 1024;
/// Maximum serialized manifest size.
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// Maximum success response retained from the server.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
/// Conservative multipart framing allowance per form part.
const MULTIPART_PART_OVERHEAD: usize = 512;
/// Exact accepted upload guidance.
const UPLOAD_NEXT: &str =
    "Native debug artifact upload accepted. Verify exact image UUID and architecture lookup.";
/// Exact successful lookup guidance.
const LOOKUP_FOUND_NEXT: &str =
    "Native debug artifact lookup matched. Verify issue-detail native symbolication.";
/// Exact missing lookup guidance.
const LOOKUP_MISSING_NEXT: &str =
    "No exact native debug artifact matched. Upload the release dSYM and retry lookup.";

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
    /// Lowercase SHA-256.
    pub(super) debug_file_sha256: String,
    /// Positive bounded payload size.
    pub(super) debug_file_byte_size: u64,
    /// Fixed upload status.
    pub(super) upload_status: String,
    /// RFC3339 UTC creation time.
    pub(super) created_at: String,
}

/// Sends and validates one exact multipart upload.
pub(super) async fn upload(
    client: &reqwest::Client,
    env: &CliEnvironment,
    url: reqwest::Url,
    options: &NativeDebugUploadOptions,
    artifacts: &[Artifact],
) -> Result<UploadReceipt, RuntimeError> {
    let manifest = serialize_manifest(options, artifacts)?;
    validate_multipart_size(manifest.len(), artifacts)?;
    let response = send_authenticated_with_refresh(client, env, |client, credential| {
        client
            .post(url.clone())
            .bearer_auth(credential.token())
            .multipart(upload_form(manifest.as_str(), artifacts))
    })
    .await
    .map_err(request_error)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(safe_api_error(status, &credential));
    }
    let body = bounded_body(response).await?;
    parse_upload_response(body.as_str(), artifacts.len())
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
    let response = send_authenticated_with_refresh(client, env, |client, credential| {
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
    let mut url = reqwest::Url::parse(base_url).map_err(|_| transport_error())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(transport_error());
    }
    url.set_path("/api/native-debug-artifacts");
    url.set_query(None);
    Ok(url)
}

/// Exact camelCase multipart manifest.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadManifest<'a> {
    /// Account-owned project UUID.
    project_id: &'a str,
    /// Exact release scope.
    release: &'a str,
    /// Exact environment scope.
    environment: &'a str,
    /// Exact service scope.
    service: &'a str,
    /// Fixed Apple dSYM manifest discriminator.
    artifact_type: &'static str,
    /// Fixed local validation result.
    validation: ManifestValidation,
    /// Exact ordered object identities.
    artifacts: Vec<ManifestArtifact<'a>>,
}

/// Fixed manifest validation object.
#[derive(serde::Serialize)]
struct ManifestValidation {
    /// Fixed ready state after local validation.
    status: &'static str,
}

/// One manifest artifact identity.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestArtifact<'a> {
    /// Canonical image UUID.
    image_uuid: &'a str,
    /// Canonical architecture.
    architecture: &'static str,
    /// Exact uploaded payload metadata.
    debug_file: ManifestDebugFile<'a>,
}

/// Exact hash and size for one multipart byte part.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestDebugFile<'a> {
    /// Lowercase SHA-256.
    artifact_sha256: &'a str,
    /// Exact payload byte count.
    byte_size: u64,
}

/// Serializes and bounds the exact upload manifest.
fn serialize_manifest(
    options: &NativeDebugUploadOptions,
    artifacts: &[Artifact],
) -> Result<String, RuntimeError> {
    let manifest = UploadManifest {
        project_id: options.project_id.as_str(),
        release: options.release.as_str(),
        environment: options.environment.as_str(),
        service: options.service.as_str(),
        artifact_type: "apple_dsym_manifest",
        validation: ManifestValidation { status: "ready" },
        artifacts: artifacts
            .iter()
            .map(|artifact| ManifestArtifact {
                image_uuid: artifact.image_uuid.as_str(),
                architecture: artifact.architecture.as_str(),
                debug_file: ManifestDebugFile {
                    artifact_sha256: artifact.sha256.as_str(),
                    byte_size: artifact.byte_size(),
                },
            })
            .collect(),
    };
    let body = serde_json::to_string(&manifest).map_err(|_| invalid_artifact())?;
    if body.len() > MAX_MANIFEST_BYTES {
        return Err(invalid_artifact());
    }
    Ok(body)
}

/// Ensures multipart framing plus payload remains inside the public limit.
fn validate_multipart_size(
    manifest_size: usize,
    artifacts: &[Artifact],
) -> Result<(), RuntimeError> {
    let sizes = artifacts
        .iter()
        .map(|artifact| artifact.bytes.len())
        .collect::<Vec<_>>();
    if !multipart_size_allowed(manifest_size, sizes.as_slice()) {
        return Err(invalid_artifact());
    }
    Ok(())
}

/// Returns whether payload plus conservative framing fits the public aggregate bound.
fn multipart_size_allowed(manifest_size: usize, artifact_sizes: &[usize]) -> bool {
    let payload = artifact_sizes
        .iter()
        .try_fold(manifest_size, |total, size| total.checked_add(*size));
    let overhead = artifact_sizes
        .len()
        .saturating_add(1)
        .saturating_mul(MULTIPART_PART_OVERHEAD);
    payload
        .and_then(|payload| payload.checked_add(overhead))
        .is_some_and(|total| total <= MAX_MULTIPART_BYTES)
}

/// Builds a filename-free multipart form in manifest order.
fn upload_form(manifest: &str, artifacts: &[Artifact]) -> reqwest::multipart::Form {
    let mut headers = reqwest::header::HeaderMap::new();
    drop(headers.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    ));
    let manifest_part = reqwest::multipart::Part::text(manifest.to_owned()).headers(headers);
    let mut form = reqwest::multipart::Form::new().part("manifest", manifest_part);
    for (index, artifact) in artifacts.iter().enumerate() {
        form = form.part(
            format!("debug_file_{index}"),
            reqwest::multipart::Part::stream_with_length(
                artifact.multipart_payload(),
                artifact.byte_size(),
            ),
        );
    }
    form
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
fn parse_upload_response(body: &str, expected_count: usize) -> Result<UploadReceipt, RuntimeError> {
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
        && artifact.artifact_type == "apple_dsym"
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
async fn bounded_body(mut response: reqwest::Response) -> Result<String, RuntimeError> {
    if response.content_length().is_some_and(|length| {
        usize::try_from(length).map_or(true, |length| length > MAX_RESPONSE_BYTES)
    }) {
        return Err(invalid_response());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| transport_error())? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(invalid_response());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| invalid_response())
}

/// Converts auth, refresh, transport, and body errors to fixed local recovery.
fn request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Api {
            status,
            auth_source,
            auth_label,
            ..
        } => RuntimeError::Api {
            status,
            body: safe_api_body(status),
            auth_source,
            auth_label,
        },
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
        400 | 422 => (
            "native debug-artifact request was rejected",
            "validation_failed",
            "check the artifact identity and request scope, then retry",
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

/// Returns the fixed path-free local artifact error.
const fn invalid_artifact() -> RuntimeError {
    RuntimeError::NativeDebugArtifactInvalid
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

#[cfg(test)]
mod tests {
    use super::{MAX_MULTIPART_BYTES, MULTIPART_PART_OVERHEAD, multipart_size_allowed};

    /// Proves artifact bytes plus multipart framing are bounded together.
    #[test]
    fn aggregate_bound_includes_multipart_framing() {
        let manifest_size = 1024;
        let framing = 4 * MULTIPART_PART_OVERHEAD;
        let remaining = MAX_MULTIPART_BYTES - manifest_size - framing;
        let first = 50 * 1024 * 1024;
        let second = 50 * 1024 * 1024;
        let third = remaining - first - second;
        assert!(multipart_size_allowed(
            manifest_size,
            &[first, second, third]
        ));
        assert!(!multipart_size_allowed(
            manifest_size,
            &[first, second, third + 1]
        ));
    }
}
