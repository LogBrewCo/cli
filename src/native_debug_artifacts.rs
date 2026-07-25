//! Apple native debug-artifact upload and exact lookup verification.

mod artifact;
mod wire;

use crate::{
    CliEnvironment, NativeDebugArtifactsTarget, NativeDebugLookupOptions, NativeDebugUploadOptions,
    RuntimeError,
};
use artifact::Artifact;
use wire::{LookupResult, UploadReceipt};

/// Connection establishment timeout shared by upload and lookup.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Bounded upload window for the maximum public multipart size on slower uplinks.
const UPLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// Bounded exact lookup window.
const LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Maximum exact multipart attempts after lookup-based ambiguity recovery.
const MAX_UPLOAD_ATTEMPTS: usize = 3;

/// Executes one bounded native debug-artifact operation.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    target: &NativeDebugArtifactsTarget,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    match target {
        NativeDebugArtifactsTarget::Upload(options) => {
            let artifacts = artifact::collect(std::path::Path::new(options.path.as_str()))?;
            artifact::validate_expected_uuids(
                artifacts.as_slice(),
                options.expected_image_uuids.as_slice(),
            )?;
            if options.dry_run {
                return write_validation(output, artifacts.as_slice(), json);
            }
            let url = wire::native_artifact_url(env.base_url.as_str())?;
            let upload_client = build_client(UPLOAD_TIMEOUT)?;
            let lookup_client = build_client(LOOKUP_TIMEOUT)?;
            let context = UploadContext {
                upload_client: &upload_client,
                lookup_client: &lookup_client,
                env,
                url,
                options,
                artifacts: artifacts.as_slice(),
            };
            execute_upload(&context, json, output).await
        }
        NativeDebugArtifactsTarget::Lookup(options) => {
            let url = wire::native_artifact_url(env.base_url.as_str())?;
            let client = build_client(LOOKUP_TIMEOUT)?;
            let lookup = wire::lookup(&client, env, url, options).await?;
            write_lookup(output, options, &lookup, json)
        }
    }
}

/// Immutable request and artifact state shared across bounded upload attempts.
struct UploadContext<'a> {
    /// Client with the longer multipart request window.
    upload_client: &'a reqwest::Client,
    /// Client with the short exact-lookup request window.
    lookup_client: &'a reqwest::Client,
    /// Canonical account-auth and API environment.
    env: &'a CliEnvironment,
    /// Parsed native debug-artifact endpoint.
    url: reqwest::Url,
    /// Validated upload scope and local mode.
    options: &'a NativeDebugUploadOptions,
    /// Canonically ordered validated object identities.
    artifacts: &'a [Artifact],
}

/// Validates, uploads, and verifies every discovered object identity.
async fn execute_upload<W: std::io::Write>(
    context: &UploadContext<'_>,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if verify_present(context, None).await? {
        return write_already_present(output, context.artifacts, json);
    }

    for attempt in 0..MAX_UPLOAD_ATTEMPTS {
        match wire::upload(
            context.upload_client,
            context.env,
            context.url.clone(),
            context.options,
            context.artifacts,
        )
        .await
        {
            Ok(upload) => {
                if !verify_present(context, Some(upload.upload_id.as_str())).await? {
                    return Err(RuntimeError::NativeDebugVerificationFailed);
                }
                return write_upload(output, &upload, context.artifacts, json);
            }
            Err(error)
                if upload_error_is_retryable(&error) && attempt + 1 < MAX_UPLOAD_ATTEMPTS =>
            {
                if verify_present(context, None).await? {
                    return write_recovered(output, context.artifacts, json);
                }
                tokio::time::sleep(upload_retry_delay(attempt)).await;
            }
            Err(error) => return Err(error),
        }
    }
    Err(RuntimeError::NativeDebugVerificationFailed)
}

/// Returns whether every exact identity is present and bound to the local bytes.
async fn verify_present(
    context: &UploadContext<'_>,
    expected_upload_id: Option<&str>,
) -> Result<bool, RuntimeError> {
    let mut all_present = true;
    for artifact in context.artifacts {
        let lookup_options = lookup_options(context.options, artifact);
        match wire::lookup(
            context.lookup_client,
            context.env,
            context.url.clone(),
            &lookup_options,
        )
        .await?
        {
            LookupResult::Missing => all_present = false,
            LookupResult::Found(found)
                if found.debug_file_sha256 == artifact.sha256
                    && found.debug_file_byte_size == artifact.byte_size()
                    && expected_upload_id.is_none_or(|upload_id| found.upload_id == upload_id) => {}
            LookupResult::Found(_) => return Err(RuntimeError::NativeDebugVerificationFailed),
        }
    }
    Ok(all_present)
}

/// Builds exact lookup scope from the upload context and one validated identity.
fn lookup_options(
    options: &NativeDebugUploadOptions,
    artifact: &Artifact,
) -> NativeDebugLookupOptions {
    NativeDebugLookupOptions {
        project_id: options.project_id.clone(),
        release: options.release.clone(),
        environment: options.environment.clone(),
        service: options.service.clone(),
        image_uuid: artifact.image_uuid.clone(),
        architecture: artifact.architecture.as_str().to_owned(),
    }
}

/// Restricts automatic retries to transport ambiguity, rate limiting, and server failures.
const fn upload_error_is_retryable(error: &RuntimeError) -> bool {
    match error {
        RuntimeError::Unavailable { .. } => true,
        RuntimeError::Api { status, .. } => *status == 429 || *status >= 500,
        RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::Http(_)
        | RuntimeError::MissingToken
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => false,
    }
}

/// Returns one fixed bounded delay before an exact replay.
const fn upload_retry_delay(attempt: usize) -> std::time::Duration {
    std::time::Duration::from_millis(if attempt == 0 { 250 } else { 1_000 })
}

/// Builds one redirect-refusing client with operation-specific request timeout.
fn build_client(timeout: std::time::Duration) -> Result<reqwest::Client, RuntimeError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|_| transport_error())
}

/// Returns a fixed URL-free transport error.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "native debug-artifact request could not be completed",
        next: "check network connectivity and retry the native debug-artifact command",
    }
}

/// Writes local-only validation output without request scope or file identity.
fn write_validation<W: std::io::Write>(
    output: &mut W,
    artifacts: &[Artifact],
    json: bool,
) -> Result<(), RuntimeError> {
    if json {
        let identities = artifact_summaries(artifacts, "validated");
        let body = serde_json::json!({
            "ok": true,
            "status": "validated",
            "artifact_count": artifacts.len(),
            "artifacts": identities,
            "next_action": {
                "code": "upload_native_debug_artifact",
                "target": "native_debug_artifact_upload"
            }
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "Native debug artifacts validated.")?;
        writeln!(output, "Artifacts: {}", artifacts.len())?;
        for artifact in artifacts {
            writeln!(
                output,
                "{} {} validated",
                artifact.architecture.as_str(),
                artifact.image_uuid
            )?;
        }
        writeln!(output, "Next: rerun without --dry-run to upload.")?;
    }
    Ok(())
}

/// Writes a lookup-confirmed no-op upload result.
fn write_already_present<W: std::io::Write>(
    output: &mut W,
    artifacts: &[Artifact],
    json: bool,
) -> Result<(), RuntimeError> {
    write_without_upload_id(output, artifacts, json, "already_present")
}

/// Writes an upload result recovered by exact lookup after an ambiguous attempt.
fn write_recovered<W: std::io::Write>(
    output: &mut W,
    artifacts: &[Artifact],
    json: bool,
) -> Result<(), RuntimeError> {
    write_without_upload_id(output, artifacts, json, "verified")
}

/// Writes a verified result that is not owned by one new upload identifier.
fn write_without_upload_id<W: std::io::Write>(
    output: &mut W,
    artifacts: &[Artifact],
    json: bool,
    status: &'static str,
) -> Result<(), RuntimeError> {
    if json {
        let identities = artifact_summaries(artifacts, status);
        let body = serde_json::json!({
            "ok": true,
            "status": status,
            "artifact_count": artifacts.len(),
            "artifacts": identities,
            "next_action": {
                "code": "verify_native_issue_symbolication",
                "target": "native_issue_symbolication"
            }
        });
        writeln!(output, "{body}")?;
    } else {
        let label = if status == "already_present" {
            "already present and verified"
        } else {
            "uploaded and verified"
        };
        writeln!(output, "Native debug artifacts {label}.")?;
        writeln!(output, "Artifacts: {}", artifacts.len())?;
        for artifact in artifacts {
            writeln!(
                output,
                "{} {} {status}",
                artifact.architecture.as_str(),
                artifact.image_uuid
            )?;
        }
        writeln!(output, "Next: verify native issue symbolication.")?;
    }
    Ok(())
}

/// Builds bounded identity metadata shared by dry-run and verified output.
fn artifact_summaries(artifacts: &[Artifact], status: &'static str) -> Vec<serde_json::Value> {
    artifacts
        .iter()
        .map(|artifact| {
            serde_json::json!({
                "image_uuid": artifact.image_uuid,
                "architecture": artifact.architecture.as_str(),
                "debug_file_sha256": artifact.sha256,
                "debug_file_byte_size": artifact.byte_size(),
                "status": status,
            })
        })
        .collect()
}

/// Writes bounded verified upload output without local artifact identity.
fn write_upload<W: std::io::Write>(
    output: &mut W,
    upload: &UploadReceipt,
    artifacts: &[Artifact],
    json: bool,
) -> Result<(), RuntimeError> {
    if json {
        let identities = artifact_summaries(artifacts, "verified");
        let body = serde_json::json!({
            "ok": true,
            "status": "verified",
            "upload_id": upload.upload_id,
            "artifact_count": upload.artifact_count,
            "artifacts": identities,
            "next_action": {
                "code": "verify_native_issue_symbolication",
                "target": "native_issue_symbolication"
            }
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "Native debug artifacts uploaded and verified.")?;
        writeln!(output, "Artifacts: {}", artifacts.len())?;
        for artifact in artifacts {
            writeln!(
                output,
                "{} {} verified",
                artifact.architecture.as_str(),
                artifact.image_uuid
            )?;
        }
        writeln!(output, "Next: verify native issue symbolication.")?;
    }
    Ok(())
}

/// Writes bounded standalone lookup output without echoing request scope.
fn write_lookup<W: std::io::Write>(
    output: &mut W,
    options: &NativeDebugLookupOptions,
    lookup: &LookupResult,
    json: bool,
) -> Result<(), RuntimeError> {
    match lookup {
        LookupResult::Found(artifact) if json => {
            let body = serde_json::json!({
                "ok": true,
                "status": "found",
                "artifact": {
                    "artifact_id": artifact.artifact_id,
                    "upload_id": artifact.upload_id,
                    "image_uuid": artifact.image_uuid,
                    "architecture": artifact.architecture,
                    "debug_file_sha256": artifact.debug_file_sha256,
                    "debug_file_byte_size": artifact.debug_file_byte_size,
                    "upload_status": artifact.upload_status,
                    "created_at": artifact.created_at,
                },
                "next_action": {
                    "code": "verify_native_issue_symbolication",
                    "target": "native_issue_symbolication"
                }
            });
            writeln!(output, "{body}")?;
        }
        LookupResult::Missing if json => {
            let body = serde_json::json!({
                "ok": true,
                "status": "missing",
                "artifact": null,
                "identity": {
                    "image_uuid": options.image_uuid,
                    "architecture": options.architecture,
                },
                "next_action": {
                    "code": "upload_native_debug_artifact",
                    "target": "native_debug_artifact_upload"
                }
            });
            writeln!(output, "{body}")?;
        }
        LookupResult::Found(_) => {
            writeln!(output, "Native debug artifact found.")?;
            writeln!(
                output,
                "Identity: {} {}",
                options.architecture, options.image_uuid
            )?;
            writeln!(output, "Status: uploaded")?;
            writeln!(output, "Next: verify native issue symbolication.")?;
        }
        LookupResult::Missing => {
            writeln!(output, "No exact native debug artifact matched.")?;
            writeln!(output, "Next: upload the release dSYM and retry lookup.")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CONNECT_TIMEOUT, LOOKUP_TIMEOUT, UPLOAD_TIMEOUT, build_client};

    /// Proves fixed bounded timeout selection without a slow network request.
    #[test]
    fn clients_use_separate_bounded_operation_windows() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(CONNECT_TIMEOUT, std::time::Duration::from_secs(10));
        assert_eq!(LOOKUP_TIMEOUT, std::time::Duration::from_secs(30));
        assert_eq!(UPLOAD_TIMEOUT, std::time::Duration::from_secs(30 * 60));
        let _upload = build_client(UPLOAD_TIMEOUT)?;
        let _lookup = build_client(LOOKUP_TIMEOUT)?;
        Ok(())
    }
}
