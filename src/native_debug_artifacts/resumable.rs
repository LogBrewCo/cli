//! Typed resumable native debug-artifact wire contract.

use super::artifact::{Artifact, ArtifactChunk, RESUMABLE_CHUNK_BYTES};
use super::wire::{self, UploadReceipt};
use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::{CliEnvironment, NativeDebugUploadOptions, RuntimeError};
use std::collections::BTreeMap;

/// Maximum serialized start manifest size.
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// Maximum reconstructed bytes declared by one session.
const MAX_AGGREGATE_BYTES: usize = 512 * 1024 * 1024;
/// Maximum server-directed retry delay accepted by the CLI.
const MAX_RETRY_AFTER_SECONDS: u64 = 30;
/// Exact chunk receipt guidance.
const CHUNK_NEXT: &str =
    "Native debug artifact chunk accepted. Continue missing chunks or complete the session.";
/// Exact missing-chunk recovery.
const CHUNK_RETRY_NEXT: &str =
    "retry only the missing native debug artifact chunk with its exact digest";
/// Exact missing-session recovery.
pub(super) const RESTART_SESSION_NEXT: &str =
    "start the native debug artifact upload session again with the same manifest";
/// Exact completion-in-progress recovery.
const COMPLETION_PENDING_NEXT: &str =
    "wait briefly, then retry the same upload completion or verify the exact artifact lookup";
/// Fixed correction for an unrecognized completion validation failure.
const COMPLETION_INVALID_NEXT: &str =
    "check the native debug-artifact manifest and upload session, then retry";
/// Exact stable initial method recovery required before compatibility fallback.
const START_METHOD_NEXT: &str = "use the supported native debug-artifact request method";

/// Fully validated immutable start body and unique chunks.
pub(super) struct PreparedUpload {
    /// Byte-identical JSON used for every bounded start attempt.
    manifest: String,
    /// Unique chunks in first-appearance order.
    chunks: Vec<PreparedChunk>,
}

impl PreparedUpload {
    /// Returns the byte-identical start body.
    pub(super) const fn manifest(&self) -> &str {
        self.manifest.as_str()
    }

    /// Finds one exact server-requested chunk.
    pub(super) fn chunk(&self, digest: &str) -> Option<&PreparedChunk> {
        self.chunks.iter().find(|chunk| chunk.digest == digest)
    }
}

/// One unique immutable chunk that can be cheaply replayed.
pub(super) struct PreparedChunk {
    /// Lowercase SHA-256 used as the public chunk identity.
    pub(super) digest: String,
    /// Exact octet-stream payload.
    bytes: bytes::Bytes,
}

impl PreparedChunk {
    /// Returns a cheap immutable payload handle.
    pub(super) fn payload(&self) -> bytes::Bytes {
        self.bytes.clone()
    }

    /// Binds a server receipt digest to this exact prepared chunk.
    fn matches_digest(&self, digest: &str) -> bool {
        self.digest == digest
    }
}

/// Initial capability negotiation result.
pub(super) enum StartOutcome {
    /// Resumable session was established.
    Session(Session),
    /// Exact initial capability absence permits one-shot fallback.
    Unsupported,
}

/// Validated resumable session state.
pub(super) struct Session {
    /// Deterministic public session identifier.
    pub(super) session_id: String,
    /// Ordered unique chunk digests still required by the server.
    pub(super) missing_chunks: Vec<String>,
}

/// One request failure plus bounded retry classification.
pub(super) struct AttemptFailure {
    /// Fixed public-safe runtime error.
    pub(super) error: RuntimeError,
    /// Phase-specific handling.
    pub(super) kind: FailureKind,
    /// Optional validated server retry delay.
    pub(super) retry_after: Option<std::time::Duration>,
}

/// Phase-specific request failure behavior.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureKind {
    /// The exact same small request may be replayed.
    Retryable,
    /// Completion is still processing and may receive one brief retry.
    CompletionPending,
    /// Completion session vanished; exact lookup must run once before restart recovery.
    CompletionSessionMissing,
    /// No automatic retry is permitted.
    Terminal,
}

/// Builds one canonical start manifest and immutable unique chunk set.
pub(super) fn prepare(
    options: &NativeDebugUploadOptions,
    artifacts: &[Artifact],
) -> Result<PreparedUpload, RuntimeError> {
    if artifacts.is_empty() || artifacts.len() > 50 {
        return Err(invalid_artifact());
    }
    let aggregate_size = artifacts.iter().try_fold(0_usize, |total, artifact| {
        total.checked_add(artifact.bytes.len())
    });
    if aggregate_size.is_none_or(|size| size == 0 || size > MAX_AGGREGATE_BYTES) {
        return Err(invalid_artifact());
    }

    let artifact_chunks = artifacts
        .iter()
        .map(Artifact::resumable_chunks)
        .collect::<Vec<_>>();
    let manifest = serialize_manifest(options, artifacts, artifact_chunks.as_slice())?;
    let chunks = unique_chunks(artifact_chunks.as_slice())?;
    Ok(PreparedUpload { manifest, chunks })
}

/// Sends one exact start request.
pub(super) async fn start(
    client: &reqwest::Client,
    env: &CliEnvironment,
    url: reqwest::Url,
    prepared: &PreparedUpload,
) -> Result<StartOutcome, AttemptFailure> {
    let response = send_account_authenticated_with_refresh(client, env, |client, credential| {
        client
            .post(url.clone())
            .bearer_auth(credential.token())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(prepared.manifest().to_owned())
    })
    .await
    .map_err(request_failure)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status == 200 {
        let body = wire::bounded_body(response)
            .await
            .map_err(terminal_failure)?;
        return parse_start_response(body.as_str(), prepared)
            .map(StartOutcome::Session)
            .map_err(terminal_failure);
    }
    classify_start_error(response, &credential).await
}

/// Sends one exact chunk PUT.
pub(super) async fn put_chunk(
    client: &reqwest::Client,
    env: &CliEnvironment,
    base_url: &reqwest::Url,
    session: &Session,
    chunk: &PreparedChunk,
) -> Result<(), AttemptFailure> {
    let url = session_url(
        base_url,
        format!(
            "/api/native-debug-artifact-uploads/{}/chunks/{}",
            session.session_id, chunk.digest
        )
        .as_str(),
    );
    let response = send_account_authenticated_with_refresh(client, env, |client, credential| {
        client
            .put(url.clone())
            .bearer_auth(credential.token())
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(chunk.payload())
    })
    .await
    .map_err(request_failure)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status == 200 {
        let body = wire::bounded_body(response)
            .await
            .map_err(terminal_failure)?;
        return parse_chunk_response(body.as_str(), session, chunk).map_err(terminal_failure);
    }
    Err(classify_phase_error(response, &credential, Phase::Chunk).await)
}

/// Sends one exact completion request.
pub(super) async fn complete(
    client: &reqwest::Client,
    env: &CliEnvironment,
    base_url: &reqwest::Url,
    session: &Session,
    artifact_count: usize,
) -> Result<UploadReceipt, AttemptFailure> {
    let url = session_url(
        base_url,
        format!(
            "/api/native-debug-artifact-uploads/{}/complete",
            session.session_id
        )
        .as_str(),
    );
    let response = send_account_authenticated_with_refresh(client, env, |client, credential| {
        client
            .post(url.clone())
            .bearer_auth(credential.token())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes::Bytes::new())
    })
    .await
    .map_err(request_failure)?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status == 200 {
        let body = wire::bounded_body(response)
            .await
            .map_err(terminal_failure)?;
        return wire::parse_upload_response(body.as_str(), artifact_count)
            .map_err(terminal_failure);
    }
    Err(classify_phase_error(response, &credential, Phase::CompleteRejected).await)
}

/// Builds the exact static start endpoint.
pub(super) fn upload_session_url(base_url: &str) -> Result<reqwest::Url, RuntimeError> {
    wire::api_url(base_url, "/api/native-debug-artifact-uploads")
}

/// Serializes the canonical camelCase session manifest.
fn serialize_manifest(
    options: &NativeDebugUploadOptions,
    artifacts: &[Artifact],
    artifact_chunks: &[Vec<ArtifactChunk>],
) -> Result<String, RuntimeError> {
    let manifest = StartManifest {
        project_id: options.project_id.as_str(),
        release: options.release.as_str(),
        environment: options.environment.as_str(),
        service: options.service.as_str(),
        artifact_type: "apple_dsym_manifest",
        validation: ManifestValidation { status: "ready" },
        artifacts: artifacts
            .iter()
            .zip(artifact_chunks)
            .map(|(artifact, chunks)| ManifestArtifact {
                image_uuid: artifact.image_uuid.as_str(),
                architecture: artifact.architecture.as_str(),
                debug_file: ManifestDebugFile {
                    artifact_sha256: artifact.sha256.as_str(),
                    byte_size: artifact.byte_size(),
                },
                chunks: chunks
                    .iter()
                    .map(|chunk| ManifestChunk {
                        sha256: chunk.sha256.as_str(),
                        byte_size: chunk.byte_size(),
                    })
                    .collect(),
            })
            .collect(),
    };
    let body = serde_json::to_string(&manifest).map_err(|_| invalid_artifact())?;
    if body.len() > MAX_MANIFEST_BYTES {
        return Err(invalid_artifact());
    }
    Ok(body)
}

/// Deduplicates chunks by digest while preserving first appearance.
fn unique_chunks(
    artifact_chunks: &[Vec<ArtifactChunk>],
) -> Result<Vec<PreparedChunk>, RuntimeError> {
    let mut indexes = BTreeMap::<String, usize>::new();
    let mut unique = Vec::<PreparedChunk>::new();
    for chunk in artifact_chunks.iter().flatten() {
        if let Some(index) = indexes.get(chunk.sha256.as_str()).copied() {
            let existing = &unique[index];
            if existing.bytes.len() != chunk.bytes.len() || existing.bytes != chunk.bytes {
                return Err(invalid_artifact());
            }
            continue;
        }
        let _ = indexes.insert(chunk.sha256.clone(), unique.len());
        unique.push(PreparedChunk {
            digest: chunk.sha256.clone(),
            bytes: chunk.bytes.clone(),
        });
    }
    if unique.is_empty() {
        return Err(invalid_artifact());
    }
    Ok(unique)
}

/// Parses and binds one exact session response.
fn parse_start_response(body: &str, prepared: &PreparedUpload) -> Result<Session, RuntimeError> {
    let response = serde_json::from_str::<StartResponse>(body).map_err(|_| invalid_response())?;
    let expected_action = if response.missing_chunks.is_empty() {
        "complete_native_debug_artifact_upload"
    } else {
        "upload_native_debug_artifact_chunks"
    };
    if !is_public_id(response.session_id.as_str(), "nativeupload_", 64)
        || response.status != "ready"
        || response.chunk_size != u64::try_from(RESUMABLE_CHUNK_BYTES).unwrap_or(u64::MAX)
        || response.next_action.code != expected_action
        || response.next_action.target != "native_debug_artifact_upload"
        || !is_public_text(response.next.as_str())
        || !missing_chunks_are_ordered(response.missing_chunks.as_slice(), prepared)
    {
        return Err(invalid_response());
    }
    Ok(Session {
        session_id: response.session_id,
        missing_chunks: response.missing_chunks,
    })
}

/// Validates ordered unique missing digests against first appearance.
fn missing_chunks_are_ordered(missing: &[String], prepared: &PreparedUpload) -> bool {
    let indexes = prepared
        .chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| (chunk.digest.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut prior = None;
    for digest in missing {
        if !is_lower_hex(digest.as_str(), 64) {
            return false;
        }
        let Some(index) = indexes.get(digest.as_str()).copied() else {
            return false;
        };
        if prior.is_some_and(|prior| index <= prior) {
            return false;
        }
        prior = Some(index);
    }
    true
}

/// Parses and binds one exact chunk receipt.
fn parse_chunk_response(
    body: &str,
    session: &Session,
    chunk: &PreparedChunk,
) -> Result<(), RuntimeError> {
    let response = serde_json::from_str::<ChunkResponse>(body).map_err(|_| invalid_response())?;
    if response.session_id != session.session_id
        || !chunk.matches_digest(response.chunk_sha256.as_str())
        || response.status != "uploaded"
        || response.next != CHUNK_NEXT
        || response.next_action.code != "upload_native_debug_artifact_chunks"
        || response.next_action.target != "native_debug_artifact_upload"
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Classifies start capability negotiation without trusting response text.
async fn classify_start_error(
    response: reqwest::Response,
    credential: &AuthCredential,
) -> Result<StartOutcome, AttemptFailure> {
    let status = response.status().as_u16();
    if status == 404 {
        let body = wire::bounded_body(response)
            .await
            .map_err(terminal_failure)?;
        if parse_error_envelope(body.as_str()).is_none() {
            return Ok(StartOutcome::Unsupported);
        }
        return Err(phase_failure(status, credential, Phase::Start, None));
    }
    if status == 405 {
        let body = wire::bounded_body(response)
            .await
            .map_err(terminal_failure)?;
        let exact_absence = parse_error_envelope(body.as_str()).is_some_and(|error| {
            error.code == "method_not_allowed"
                && error.next == START_METHOD_NEXT
                && error.next_action.code == "use_supported_method"
                && error.next_action.target == "api_method"
                && error.retry_after_seconds.is_none()
        });
        if exact_absence {
            return Ok(StartOutcome::Unsupported);
        }
        return Err(terminal_failure(invalid_response()));
    }
    Err(classify_phase_error(response, credential, Phase::Start).await)
}

/// Classifies one non-success phase response.
async fn classify_phase_error(
    response: reqwest::Response,
    credential: &AuthCredential,
    phase: Phase,
) -> AttemptFailure {
    let status = response.status().as_u16();
    let body = if matches!(status, 404 | 405 | 422 | 429) {
        wire::bounded_body(response).await.ok()
    } else {
        None
    };
    let envelope = body.as_deref().and_then(parse_error_envelope);
    let retry_after = envelope
        .as_ref()
        .and_then(|error| error.retry_after_seconds)
        .map(|seconds| std::time::Duration::from_secs(seconds.min(MAX_RETRY_AFTER_SECONDS)));
    let (safe_phase, kind) = if retryable_status(status) {
        (phase, FailureKind::Retryable)
    } else if phase.is_complete() && status == 422 {
        completion_validation_class(envelope.as_ref())
    } else if phase.is_complete() && status == 404 {
        (phase, FailureKind::CompletionSessionMissing)
    } else {
        (phase, FailureKind::Terminal)
    };
    phase_failure(status, credential, safe_phase, retry_after).with_kind(kind)
}

/// Binds a completion validation response to one fixed recovery class.
fn completion_validation_class(envelope: Option<&ErrorEnvelope>) -> (Phase, FailureKind) {
    if envelope.is_some_and(|error| {
        error.code == "validation_failed"
            && error.next == COMPLETION_PENDING_NEXT
            && error.next_action.code == "complete_native_debug_artifact_upload"
            && error.next_action.target == "native_debug_artifact_upload"
    }) {
        return (Phase::CompletePending, FailureKind::CompletionPending);
    }
    if envelope.is_some_and(|error| {
        error.code == "validation_failed"
            && error.next == CHUNK_RETRY_NEXT
            && error.next_action.code == "upload_native_debug_artifact_chunks"
            && error.next_action.target == "native_debug_artifact_upload"
    }) {
        return (Phase::CompleteMissingChunk, FailureKind::Terminal);
    }
    (Phase::CompleteRejected, FailureKind::Terminal)
}

/// Converts a request-construction or transport error into bounded retry metadata.
fn request_failure(error: RuntimeError) -> AttemptFailure {
    match error {
        RuntimeError::Http(_) => AttemptFailure {
            error: transport_error(),
            kind: FailureKind::Retryable,
            retry_after: None,
        },
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => terminal_failure(error),
        RuntimeError::Api { .. }
        | RuntimeError::Cli(_)
        | RuntimeError::Io(_)
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::ExplainResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => terminal_failure(transport_error()),
    }
}

/// Creates one phase-specific fixed API failure.
fn phase_failure(
    status: u16,
    credential: &AuthCredential,
    phase: Phase,
    retry_after: Option<std::time::Duration>,
) -> AttemptFailure {
    AttemptFailure {
        error: RuntimeError::Api {
            status,
            body: safe_api_body(status, phase),
            auth_source: credential.source(),
            auth_label: credential.label(),
        },
        kind: if retryable_status(status) {
            FailureKind::Retryable
        } else {
            FailureKind::Terminal
        },
        retry_after,
    }
}

impl AttemptFailure {
    /// Replaces the default status classification with phase-specific behavior.
    const fn with_kind(mut self, kind: FailureKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Creates one non-retryable failure.
const fn terminal_failure(error: RuntimeError) -> AttemptFailure {
    AttemptFailure {
        error,
        kind: FailureKind::Terminal,
        retry_after: None,
    }
}

/// Produces one fixed phase-aware safe envelope from status only.
fn safe_api_body(status: u16, phase: Phase) -> String {
    let (error, code, next, action_code, target) = safe_api_fields(status, phase);
    serde_json::json!({
        "error": error,
        "code": code,
        "next": next,
        "next_action": {"code": action_code, "target": target}
    })
    .to_string()
}

/// Fixed value-safe fields used to synthesize one local API envelope.
type SafeApiFields = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

/// Selects phase-specific fields after common statuses are handled.
const fn safe_api_fields(status: u16, phase: Phase) -> SafeApiFields {
    if let Some(fields) = common_safe_api_fields(status) {
        return fields;
    }
    match (phase, status) {
        (Phase::Start, 404) => (
            "native debug-artifact upload scope was not found",
            "not_found",
            "check the exact project and upload scope",
            "check_resource",
            "resource",
        ),
        (
            Phase::Chunk
            | Phase::CompletePending
            | Phase::CompleteMissingChunk
            | Phase::CompleteRejected,
            404,
        ) => (
            "native debug-artifact upload session was not found",
            "not_found",
            RESTART_SESSION_NEXT,
            "start_native_debug_artifact_upload",
            "native_debug_artifact_upload",
        ),
        (Phase::Chunk, 422) => (
            "native debug-artifact chunk was rejected",
            "validation_failed",
            CHUNK_RETRY_NEXT,
            "upload_native_debug_artifact_chunks",
            "native_debug_artifact_upload",
        ),
        (Phase::CompletePending, 422) => (
            "native debug-artifact completion was rejected",
            "validation_failed",
            COMPLETION_PENDING_NEXT,
            "complete_native_debug_artifact_upload",
            "native_debug_artifact_upload",
        ),
        (Phase::CompleteMissingChunk, 422) => (
            "native debug-artifact completion is missing a chunk",
            "validation_failed",
            CHUNK_RETRY_NEXT,
            "upload_native_debug_artifact_chunks",
            "native_debug_artifact_upload",
        ),
        (Phase::CompleteRejected, 422) => (
            "native debug-artifact completion was rejected",
            "validation_failed",
            COMPLETION_INVALID_NEXT,
            "fix_request",
            "request",
        ),
        (Phase::Start, 422) => (
            "native debug-artifact manifest was rejected",
            "validation_failed",
            "check the native debug-artifact manifest and retry",
            "fix_request",
            "request",
        ),
        _ => (
            "native debug-artifact request returned an unexpected status",
            "unexpected_response",
            "retry the native debug-artifact command",
            "retry_request",
            "request",
        ),
    }
}

/// Returns fields shared by every resumable request phase.
const fn common_safe_api_fields(status: u16) -> Option<SafeApiFields> {
    match status {
        401 | 403 => Some((
            "authentication is required",
            "unauthorized",
            "sign in and retry the native debug-artifact command",
            "sign_in",
            "auth",
        )),
        405 => Some((
            "native debug-artifact request method is unsupported",
            "method_not_allowed",
            "use the supported native debug-artifact request method",
            "use_supported_method",
            "api_method",
        )),
        408 => Some((
            "native debug-artifact request timed out",
            "request_timeout",
            "retry the same native debug-artifact request",
            "retry_request",
            "request",
        )),
        429 => Some((
            "native debug-artifact request is temporarily limited",
            "rate_limited",
            "retry the same native debug-artifact request later",
            "retry_later",
            "request",
        )),
        500 | 502 | 503 | 504 => Some((
            "native debug-artifact service is unavailable",
            "server_error",
            "retry the same native debug-artifact request later",
            "retry_later",
            "request",
        )),
        _ => None,
    }
}

/// Returns whether the same phase request can be replayed.
const fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// Parses one exact bounded public error envelope.
fn parse_error_envelope(body: &str) -> Option<ErrorEnvelope> {
    let response = serde_json::from_str::<ErrorEnvelope>(body).ok()?;
    if !is_public_text(response.error.as_str())
        || !is_public_token(response.code.as_str())
        || !is_public_text(response.next.as_str())
        || !is_public_token(response.next_action.code.as_str())
        || !is_public_token(response.next_action.target.as_str())
    {
        return None;
    }
    Some(response)
}

/// Builds a session URL after validating every dynamic segment locally.
fn session_url(base_url: &reqwest::Url, path: &str) -> reqwest::Url {
    let mut url = base_url.clone();
    url.set_path(path);
    url.set_query(None);
    url
}

/// Restricts public IDs to one exact prefix and lowercase hexadecimal suffix.
fn is_public_id(value: &str, prefix: &str, suffix_length: usize) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| is_lower_hex(suffix, suffix_length))
}

/// Checks one exact lowercase hexadecimal token.
fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Restricts display-safe server strings to one bounded control-free line.
fn is_public_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

/// Restricts typed server tokens to bounded lowercase public vocabulary.
fn is_public_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Returns a fixed path- and URL-free transport error.
const fn transport_error() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "native debug-artifact request could not be completed",
        next: "check network connectivity and retry the native debug-artifact command",
    }
}

/// Returns the fixed local artifact failure.
const fn invalid_artifact() -> RuntimeError {
    RuntimeError::NativeDebugArtifactInvalid
}

/// Returns the fixed response-contract failure.
const fn invalid_response() -> RuntimeError {
    RuntimeError::NativeDebugResponseInvalid
}

/// Resumable request phase.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Session start request.
    Start,
    /// One exact chunk request.
    Chunk,
    /// Exact completion-in-progress response.
    CompletePending,
    /// Exact missing-chunk completion response.
    CompleteMissingChunk,
    /// Any other completion response.
    CompleteRejected,
}

impl Phase {
    /// Returns whether this phase represents session completion.
    const fn is_complete(self) -> bool {
        matches!(
            self,
            Self::CompletePending | Self::CompleteMissingChunk | Self::CompleteRejected
        )
    }
}

/// Exact camelCase start manifest.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StartManifest<'a> {
    /// Account-owned project UUID.
    project_id: &'a str,
    /// Exact release scope.
    release: &'a str,
    /// Exact environment scope.
    environment: &'a str,
    /// Exact service scope.
    service: &'a str,
    /// Fixed Apple dSYM discriminator.
    artifact_type: &'static str,
    /// Fixed local validation state.
    validation: ManifestValidation,
    /// Ordered validated artifacts.
    artifacts: Vec<ManifestArtifact<'a>>,
}

/// Fixed local validation state.
#[derive(serde::Serialize)]
struct ManifestValidation {
    /// Fixed ready value.
    status: &'static str,
}

/// One exact artifact plus ordered chunk descriptors.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestArtifact<'a> {
    /// Canonical lowercase image UUID.
    image_uuid: &'a str,
    /// Supported canonical architecture.
    architecture: &'static str,
    /// Whole reconstructed file metadata.
    debug_file: ManifestDebugFile<'a>,
    /// Ordered fixed-size chunk descriptors.
    chunks: Vec<ManifestChunk<'a>>,
}

/// Exact reconstructed file metadata.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestDebugFile<'a> {
    /// Lowercase whole-file SHA-256.
    artifact_sha256: &'a str,
    /// Exact reconstructed file size.
    byte_size: u64,
}

/// One ordered chunk descriptor.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestChunk<'a> {
    /// Lowercase chunk SHA-256.
    sha256: &'a str,
    /// Exact chunk size.
    byte_size: u64,
}

/// Exact start success surface.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StartResponse {
    /// Deterministic public session identifier.
    session_id: String,
    /// Fixed ready state.
    status: String,
    /// Fixed server chunk size.
    chunk_size: u64,
    /// Ordered unique missing chunk digests.
    missing_chunks: Vec<String>,
    /// Bounded display-safe guidance.
    next: String,
    /// Exact next resumable action.
    next_action: NextAction,
}

/// Exact chunk success surface.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChunkResponse {
    /// Exact session identifier.
    session_id: String,
    /// Exact accepted chunk digest.
    chunk_sha256: String,
    /// Fixed uploaded state.
    status: String,
    /// Exact public guidance.
    next: String,
    /// Exact next resumable action.
    next_action: NextAction,
}

/// Exact action surface shared by resumable responses.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NextAction {
    /// Typed action code.
    code: String,
    /// Typed action target.
    target: String,
}

/// Exact structured public error surface with optional bounded retry delay.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorEnvelope {
    /// Bounded public error summary.
    error: String,
    /// Typed public error code.
    code: String,
    /// Bounded public recovery.
    next: String,
    /// Typed public recovery action.
    next_action: NextAction,
    #[serde(default)]
    /// Optional server-directed retry delay.
    retry_after_seconds: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_AGGREGATE_BYTES, MAX_RETRY_AFTER_SECONDS, RESUMABLE_CHUNK_BYTES, is_public_id,
        retryable_status,
    };

    #[test]
    fn resumable_boundary_accepts_a_large_universal_ios_dsym() {
        const LARGE_THIN_OBJECT_BYTES: usize = 224 * 1024 * 1024;

        const {
            assert!(MAX_AGGREGATE_BYTES >= LARGE_THIN_OBJECT_BYTES * 2);
        }
    }

    #[test]
    fn retry_and_identity_policy_is_exact() {
        assert_eq!(RESUMABLE_CHUNK_BYTES, 4 * 1024 * 1024);
        assert_eq!(MAX_RETRY_AFTER_SECONDS, 30);
        for status in [408, 429, 500, 502, 503, 504] {
            assert!(retryable_status(status));
        }
        for status in [400, 401, 403, 404, 405, 409, 413, 422, 501] {
            assert!(!retryable_status(status));
        }
        assert!(is_public_id(
            "nativeupload_1111111111111111111111111111111111111111111111111111111111111111",
            "nativeupload_",
            64
        ));
        assert!(!is_public_id(
            "nativeupload_111111111111111111111111111111111111111111111111111111111111111A",
            "nativeupload_",
            64
        ));
    }
}
