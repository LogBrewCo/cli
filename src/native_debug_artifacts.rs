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
    let url = wire::native_artifact_url(env.base_url.as_str())?;
    match target {
        NativeDebugArtifactsTarget::Upload(options) => {
            let upload_client = build_client(UPLOAD_TIMEOUT)?;
            let lookup_client = build_client(LOOKUP_TIMEOUT)?;
            execute_upload(
                &upload_client,
                &lookup_client,
                env,
                url,
                options,
                json,
                output,
            )
            .await
        }
        NativeDebugArtifactsTarget::Lookup(options) => {
            let client = build_client(LOOKUP_TIMEOUT)?;
            let lookup = wire::lookup(&client, env, url, options).await?;
            write_lookup(output, options, &lookup, json)
        }
    }
}

/// Validates, uploads, and verifies every discovered object identity.
async fn execute_upload<W: std::io::Write>(
    upload_client: &reqwest::Client,
    lookup_client: &reqwest::Client,
    env: &CliEnvironment,
    url: reqwest::Url,
    options: &NativeDebugUploadOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let artifacts = artifact::collect(std::path::Path::new(options.path.as_str()))?;
    let upload = wire::upload(
        upload_client,
        env,
        url.clone(),
        options,
        artifacts.as_slice(),
    )
    .await?;

    for artifact in &artifacts {
        let lookup_options = NativeDebugLookupOptions {
            project_id: options.project_id.clone(),
            release: options.release.clone(),
            environment: options.environment.clone(),
            service: options.service.clone(),
            image_uuid: artifact.image_uuid.clone(),
            architecture: artifact.architecture.as_str().to_owned(),
        };
        let LookupResult::Found(found) =
            wire::lookup(lookup_client, env, url.clone(), &lookup_options).await?
        else {
            return Err(RuntimeError::NativeDebugVerificationFailed);
        };
        if found.upload_id != upload.upload_id {
            return Err(RuntimeError::NativeDebugVerificationFailed);
        }
        if found.debug_file_sha256 != artifact.sha256 {
            return Err(RuntimeError::NativeDebugVerificationFailed);
        }
        if found.debug_file_byte_size != artifact.byte_size() {
            return Err(RuntimeError::NativeDebugVerificationFailed);
        }
    }

    write_upload(output, &upload, artifacts.as_slice(), json)
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

/// Writes bounded verified upload output without local artifact identity.
fn write_upload<W: std::io::Write>(
    output: &mut W,
    upload: &UploadReceipt,
    artifacts: &[Artifact],
    json: bool,
) -> Result<(), RuntimeError> {
    if json {
        let identities = artifacts
            .iter()
            .map(|artifact| {
                serde_json::json!({
                    "image_uuid": artifact.image_uuid,
                    "architecture": artifact.architecture.as_str(),
                    "debug_file_sha256": artifact.sha256,
                    "debug_file_byte_size": artifact.byte_size(),
                    "status": "verified",
                })
            })
            .collect::<Vec<_>>();
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
