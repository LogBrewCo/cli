//! Native debug-artifact upload and exact lookup verification.

mod artifact;
mod resumable;
mod wire;

use crate::{
    CliEnvironment, NativeDebugArtifactsTarget, NativeDebugLookupOptions, NativeDebugUploadOptions,
    RuntimeError,
};
use artifact::Artifact;
use wire::{LookupResult, UploadReceipt};

/// Connection establishment timeout shared by upload and lookup.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Bounded small request and exact lookup window.
const SHORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// Bounded chunk and completion request window.
const TRANSFER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Overall network deadline for one upload invocation.
const OVERALL_UPLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// Maximum same-phase attempts: one request plus one explicit idempotent retry.
const MAX_PHASE_ATTEMPTS: usize = 2;

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
                return write_artifact_result(
                    output,
                    artifacts.as_slice(),
                    json,
                    "validated",
                    None,
                );
            }
            let url = wire::native_artifact_url(env.base_url.as_str())?;
            let session_url = resumable::upload_session_url(env.base_url.as_str())?;
            let short_client = build_client(SHORT_TIMEOUT)?;
            let transfer_client = build_client(TRANSFER_TIMEOUT)?;
            let context = UploadContext {
                short_client: &short_client,
                transfer_client: &transfer_client,
                env,
                url,
                session_url,
                options,
                artifacts: artifacts.as_slice(),
                write_only: crate::auth::token_is_project_ingest_key(env.token.as_deref()),
            };
            with_upload_deadline(
                OVERALL_UPLOAD_TIMEOUT,
                execute_upload(&context, json, output),
            )
            .await
        }
        NativeDebugArtifactsTarget::Lookup(options) => {
            let url = wire::native_artifact_url(env.base_url.as_str())?;
            let client = build_client(SHORT_TIMEOUT)?;
            let lookup = wire::lookup(&client, env, url, options).await?;
            write_lookup(output, options, &lookup, json)
        }
    }
}

/// Immutable request and artifact state shared across bounded upload attempts.
struct UploadContext<'a> {
    /// Client for small requests and lookup.
    short_client: &'a reqwest::Client,
    /// Client for chunk transfer and completion.
    transfer_client: &'a reqwest::Client,
    /// Authentication and API environment.
    env: &'a CliEnvironment,
    /// Native artifact endpoint.
    url: reqwest::Url,
    /// Resumable session endpoint.
    session_url: reqwest::Url,
    /// Validated upload scope.
    options: &'a NativeDebugUploadOptions,
    /// Ordered object identities.
    artifacts: &'a [Artifact],
    /// Whether lookup is unavailable to the supplied key.
    write_only: bool,
}

/// Validates, uploads, and verifies every discovered object identity.
async fn execute_upload<W: std::io::Write>(
    context: &UploadContext<'_>,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !context.write_only && !json {
        write_progress(output, "Checking native debug artifact availability.")?;
    }
    if !context.write_only && verify_present(context, None).await? {
        return write_artifact_result(output, context.artifacts, json, "already_present", None);
    }

    if !json {
        write_progress(output, "Starting resumable native debug artifact upload.")?;
    }
    let prepared = resumable::prepare(context.options, context.artifacts)?;
    let session = start_resumable(context, &prepared).await?;
    execute_resumable(context, &prepared, &session, json, output).await
}

/// Starts one resumable session with bounded byte-identical manifest retries.
async fn start_resumable(
    context: &UploadContext<'_>,
    prepared: &resumable::PreparedUpload,
) -> Result<resumable::Session, RuntimeError> {
    for attempt in 0..MAX_PHASE_ATTEMPTS {
        match resumable::start(
            context.short_client,
            context.env,
            context.session_url.clone(),
            prepared,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(failure)
                if failure.kind == resumable::FailureKind::Retryable
                    && attempt + 1 < MAX_PHASE_ATTEMPTS =>
            {
                tokio::time::sleep(retry_delay(attempt, failure.retry_after)).await;
            }
            Err(failure) => return Err(*failure.error),
        }
    }
    Err(RuntimeError::NativeDebugVerificationFailed)
}

/// Uploads only server-declared missing chunks, then completes and verifies.
async fn execute_resumable<W: std::io::Write>(
    context: &UploadContext<'_>,
    prepared: &resumable::PreparedUpload,
    session: &resumable::Session,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    for (index, digest) in session.missing_chunks.iter().enumerate() {
        let chunk = prepared
            .chunk(digest.as_str())
            .ok_or(RuntimeError::NativeDebugResponseInvalid)?;
        if !json {
            write_progress(
                output,
                format!(
                    "Uploading chunk {}/{}.",
                    index + 1,
                    session.missing_chunks.len()
                )
                .as_str(),
            )?;
        }
        put_chunk_with_retry(context, session, chunk).await?;
        if !json {
            writeln!(
                output,
                "Uploaded chunk {}/{}.",
                index + 1,
                session.missing_chunks.len()
            )?;
        }
    }
    if !json {
        write_progress(output, "Completing native debug artifact upload.")?;
    }
    let (receipt, lookup_verified) = complete_with_recovery(context, session).await?;
    if !context.write_only
        && !lookup_verified
        && !verify_present(context, Some(receipt.upload_id.as_str())).await?
    {
        return Err(RuntimeError::NativeDebugVerificationFailed);
    }
    write_artifact_result(
        output,
        context.artifacts,
        json,
        if context.write_only {
            "uploaded"
        } else {
            "verified"
        },
        Some(&receipt),
    )
}

/// Replays only one exact ambiguous chunk within the bounded phase budget.
async fn put_chunk_with_retry(
    context: &UploadContext<'_>,
    session: &resumable::Session,
    chunk: &resumable::PreparedChunk,
) -> Result<(), RuntimeError> {
    for attempt in 0..MAX_PHASE_ATTEMPTS {
        match resumable::put_chunk(
            context.transfer_client,
            context.env,
            &context.session_url,
            session,
            chunk,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(failure)
                if failure.kind == resumable::FailureKind::Retryable
                    && attempt + 1 < MAX_PHASE_ATTEMPTS =>
            {
                tokio::time::sleep(retry_delay(attempt, failure.retry_after)).await;
            }
            Err(failure) => return Err(*failure.error),
        }
    }
    Err(RuntimeError::NativeDebugVerificationFailed)
}

/// Completes one session with exact lookup recovery after ambiguous responses.
async fn complete_with_recovery(
    context: &UploadContext<'_>,
    session: &resumable::Session,
) -> Result<(UploadReceipt, bool), RuntimeError> {
    for attempt in 0..MAX_PHASE_ATTEMPTS {
        match resumable::complete(
            context.transfer_client,
            context.env,
            &context.session_url,
            session,
            context.artifacts.len(),
        )
        .await
        {
            Ok(receipt) => return Ok((receipt, false)),
            Err(failure)
                if matches!(
                    failure.kind,
                    resumable::FailureKind::Retryable
                        | resumable::FailureKind::CompletionPending
                        | resumable::FailureKind::CompletionSessionMissing
                ) =>
            {
                if !context.write_only {
                    if let Some(upload) = lookup_recovered_upload(context).await? {
                        return Ok((upload, true));
                    }
                    if failure.kind == resumable::FailureKind::CompletionSessionMissing {
                        return Err(*failure.error);
                    }
                }
                if attempt + 1 >= MAX_PHASE_ATTEMPTS {
                    return Err(*failure.error);
                }
                tokio::time::sleep(retry_delay(attempt, failure.retry_after)).await;
            }
            Err(failure) => return Err(*failure.error),
        }
    }
    Err(RuntimeError::NativeDebugVerificationFailed)
}

/// Returns one recovered receipt only when every identity matches one upload.
async fn lookup_recovered_upload(
    context: &UploadContext<'_>,
) -> Result<Option<UploadReceipt>, RuntimeError> {
    let mut upload_id = None::<String>;
    let mut complete = true;
    for artifact in context.artifacts {
        let options = lookup_options(context.options, artifact);
        match wire::lookup(
            context.short_client,
            context.env,
            context.url.clone(),
            &options,
        )
        .await?
        {
            LookupResult::Missing => complete = false,
            LookupResult::Found(found)
                if found.debug_file_sha256 == artifact.sha256
                    && found.debug_file_byte_size == artifact.byte_size()
                    && found.artifact_type == artifact.kind.artifact() =>
            {
                if upload_id
                    .as_ref()
                    .is_some_and(|expected| expected != &found.upload_id)
                {
                    return Err(RuntimeError::NativeDebugVerificationFailed);
                }
                let _ = upload_id.get_or_insert(found.upload_id);
            }
            LookupResult::Found(_) => return Err(RuntimeError::NativeDebugVerificationFailed),
        }
    }
    if !complete {
        return Ok(None);
    }
    let upload_id = upload_id.ok_or(RuntimeError::NativeDebugVerificationFailed)?;
    Ok(Some(UploadReceipt {
        upload_id,
        artifact_count: u64::try_from(context.artifacts.len()).unwrap_or(u64::MAX),
    }))
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
            context.short_client,
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
                    && found.artifact_type == artifact.kind.artifact()
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

/// Returns one bounded delay, honoring a validated server value when present.
fn retry_delay(attempt: usize, retry_after: Option<std::time::Duration>) -> std::time::Duration {
    retry_after
        .unwrap_or_else(|| std::time::Duration::from_millis(if attempt == 0 { 250 } else { 1_000 }))
}

/// Builds one redirect-refusing client with operation-specific request timeout.
fn build_client(timeout: std::time::Duration) -> Result<reqwest::Client, RuntimeError> {
    crate::http::client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|_| transport_error())
}

/// Applies one fixed end-to-end deadline without changing the operation's error.
async fn with_upload_deadline<F, T>(
    timeout: std::time::Duration,
    operation: F,
) -> Result<T, RuntimeError>
where
    F: Future<Output = Result<T, RuntimeError>>,
{
    tokio::time::timeout(timeout, operation)
        .await
        .unwrap_or_else(|_| Err(overall_timeout_error()))
}

/// Returns fixed recovery for an upload that exceeded its invocation budget.
const fn overall_timeout_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "native debug-artifact upload exceeded its overall time limit",
        next: "rerun the same command; resumable upload will request only missing chunks",
    }
}

/// Returns a fixed URL-free transport error.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "native debug-artifact request could not be completed",
        next: "check network connectivity and retry the native debug-artifact command",
    }
}

/// Writes and flushes one fixed human-only phase before a network wait.
fn write_progress<W: std::io::Write>(output: &mut W, message: &str) -> Result<(), RuntimeError> {
    writeln!(output, "{message}")?;
    output.flush()?;
    Ok(())
}

/// Writes one bounded validation or upload result without local file identity.
fn write_artifact_result<W: std::io::Write>(
    output: &mut W,
    artifacts: &[Artifact],
    json: bool,
    status: &'static str,
    upload: Option<&UploadReceipt>,
) -> Result<(), RuntimeError> {
    let (heading, next, code, target) = match status {
        "validated" => (
            "Native debug artifacts validated.",
            "rerun without --dry-run to upload.",
            "upload_native_debug_artifact",
            "native_debug_artifact_upload",
        ),
        "uploaded" => (
            "Native debug artifacts uploaded.",
            "run logbrew login, then verify each identity with logbrew debug-artifacts lookup.",
            "verify_native_debug_artifact_lookup",
            "native_debug_artifact_lookup",
        ),
        "verified" => (
            "Native debug artifacts uploaded and verified.",
            "verify native issue symbolication.",
            "verify_native_issue_symbolication",
            "native_issue_symbolication",
        ),
        "already_present" => (
            "Native debug artifacts already present and verified.",
            "verify native issue symbolication.",
            "verify_native_issue_symbolication",
            "native_issue_symbolication",
        ),
        _ => return Err(RuntimeError::NativeDebugResponseInvalid),
    };
    if json {
        let artifact_count = upload.map_or_else(
            || u64::try_from(artifacts.len()).unwrap_or(u64::MAX),
            |receipt| receipt.artifact_count,
        );
        let mut body = serde_json::json!({
            "ok": true,
            "status": status,
            "artifact_count": artifact_count,
            "artifacts": artifact_summaries(artifacts, status),
            "next_action": {"code": code, "target": target}
        });
        if let Some(receipt) = upload {
            body["upload_id"] = serde_json::Value::String(receipt.upload_id.clone());
        }
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "{heading}")?;
        writeln!(output, "Artifacts: {}", artifacts.len())?;
        for artifact in artifacts {
            writeln!(
                output,
                "{} {} {status}",
                artifact.architecture.as_str(),
                artifact.image_uuid
            )?;
        }
        writeln!(output, "Next: {next}")?;
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
                "artifact_type": artifact.kind.artifact(),
                "debug_file_sha256": artifact.sha256,
                "debug_file_byte_size": artifact.byte_size(),
                "status": status,
            })
        })
        .collect()
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
                    "artifact_type": artifact.artifact_type,
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
            writeln!(
                output,
                "Next: upload the release debug file and retry lookup."
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CONNECT_TIMEOUT, OVERALL_UPLOAD_TIMEOUT, SHORT_TIMEOUT, TRANSFER_TIMEOUT, build_client,
        with_upload_deadline,
    };

    /// Proves fixed bounded timeout selection without a slow network request.
    #[test]
    fn clients_use_bounded_operation_windows() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(CONNECT_TIMEOUT, std::time::Duration::from_secs(10));
        assert_eq!(SHORT_TIMEOUT, std::time::Duration::from_secs(15));
        assert_eq!(TRANSFER_TIMEOUT, std::time::Duration::from_secs(60));
        assert_eq!(
            OVERALL_UPLOAD_TIMEOUT,
            std::time::Duration::from_secs(30 * 60)
        );
        let _upload = build_client(TRANSFER_TIMEOUT)?;
        let _lookup = build_client(SHORT_TIMEOUT)?;
        Ok(())
    }

    /// Proves the overall deadline returns fixed recovery without a long-running request.
    #[test]
    fn overall_deadline_is_value_safe() -> Result<(), Box<dyn std::error::Error>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let error = runtime
            .block_on(with_upload_deadline(
                std::time::Duration::from_millis(1),
                std::future::pending::<Result<(), crate::RuntimeError>>(),
            ))
            .expect_err("pending upload must hit the supplied test deadline");
        assert!(matches!(
            error,
            crate::RuntimeError::Unavailable {
                message: "native debug-artifact upload exceeded its overall time limit",
                next: "rerun the same command; resumable upload will request only missing chunks",
            }
        ));
        Ok(())
    }
}
