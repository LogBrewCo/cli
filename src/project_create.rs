//! Secure account-owned project bootstrap and one-time key persistence.

use crate::auth::send_authenticated_with_refresh;
use crate::{AuthCredential, CliEnvironment, ProjectCreateOptions, RuntimeError};
use std::io::Write as _;

/// Shared owner-only directory for private CLI state.
const PRIVATE_DIR: &str = ".logbrew";
/// Advisory lock serializing project bootstrap attempts.
const LOCK_FILE: &str = "project-create.lock";
/// Pending byte-identical request and idempotency key.
const RETRY_FILE: &str = "project-create-retry.json";
/// Maximum accepted project-create response body.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// Lowercase alphabet used for random retry and temporary-file names.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Executes one locked, idempotent project bootstrap.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &ProjectCreateOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !private_storage_supported() {
        return Err(private_storage_unavailable());
    }
    let home = env.home.clone().ok_or_else(retry_state_unavailable)?;
    let target = resolve_target(
        env,
        options.ingest_key_file.as_str(),
        home_owner(home.as_path())?,
    )?;
    let origin = normalized_origin(env.base_url.as_str())?;
    let body = serde_json::to_string(&project_body(options)).map_err(|_| invalid_request())?;
    let lock = tokio::task::spawn_blocking(move || BootstrapLock::exclusive(home.as_path()))
        .await
        .map_err(|_| retry_state_unavailable())??;
    let pending = lock.prepare(
        origin.as_str(),
        body.as_str(),
        target.as_path(),
        options.abandon_retry,
    )?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| transport_unavailable())?;
    let url = format!("{origin}/api/projects");
    let response = send_authenticated_with_refresh(&client, env, |client, credential| {
        client
            .post(url.as_str())
            .bearer_auth(credential.token())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", pending.retry_key.as_str())
            .body(pending.request_body.clone())
    })
    .await
    .map_err(project_request_error)?;
    let (response, credential) = response;
    let status = response.status();
    let response_body = bounded_response_body(response).await?;
    if !status.is_success() {
        return Err(safe_api_error(
            status.as_u16(),
            response_body.as_str(),
            &credential,
        )?);
    }

    let created = CreatedProject::parse(response_body.as_str(), options)?;
    persist_ingest_key(
        target.as_path(),
        created.token.as_str(),
        pending.allow_existing_target,
        home_owner(lock.home.as_path())?,
    )?;
    lock.clear_retry()?;
    write_success(&created, json, output)?;
    Ok(())
}

/// Reads a response incrementally without buffering beyond the public limit.
async fn bounded_response_body(mut response: reqwest::Response) -> Result<String, RuntimeError> {
    if response.content_length().is_some_and(|length| {
        usize::try_from(length).map_or(true, |length| length > MAX_RESPONSE_BYTES)
    }) {
        return Err(invalid_response());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| transport_unavailable())?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(invalid_response());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| invalid_response())
}

/// Preserves fixed auth recovery while redacting transport and storage errors.
fn project_request_error(error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::MissingToken | RuntimeError::Unavailable { .. } => error,
        RuntimeError::Io(_) => local_auth_unavailable(),
        RuntimeError::Cli(_)
        | RuntimeError::Http(_)
        | RuntimeError::Api { .. }
        | RuntimeError::StatusUnavailable { .. }
        | RuntimeError::InvestigationResponseInvalid
        | RuntimeError::NativeDebugArtifactInvalid
        | RuntimeError::NativeDebugResponseInvalid
        | RuntimeError::NativeDebugVerificationFailed => transport_unavailable(),
    }
}

/// Builds the canonical request object with a fixed source.
fn project_body(options: &ProjectCreateOptions) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    drop(body.insert(
        "name".to_owned(),
        serde_json::Value::String(options.name.clone()),
    ));
    if let Some(runtime) = options.runtime.as_ref() {
        drop(body.insert(
            "runtime".to_owned(),
            serde_json::Value::String(runtime.clone()),
        ));
    }
    if let Some(environment) = options.environment.as_ref() {
        drop(body.insert(
            "environment".to_owned(),
            serde_json::Value::String(environment.clone()),
        ));
    }
    drop(body.insert(
        "source".to_owned(),
        serde_json::Value::String(String::from("cli")),
    ));
    serde_json::Value::Object(body)
}

/// One validated create response without rendering the one-time token.
struct CreatedProject {
    /// Account-owned project UUID.
    project_id: String,
    /// Canonical backend setup state.
    setup_status: String,
    /// Project creation timestamp.
    project_created_at: String,
    /// Validated setup recovery action code.
    setup_action_code: String,
    /// Validated setup recovery action target.
    setup_action_target: String,
    /// Public identifier for the created ingest key.
    ingest_id: String,
    /// Display-safe ingest-key label.
    ingest_label: String,
    /// Ingest-key creation timestamp.
    ingest_created_at: String,
    /// Ingest-key expiration timestamp.
    ingest_expires_at: String,
    /// One-time secret, used only for durable local persistence.
    token: String,
}

impl CreatedProject {
    /// Parses and binds the exact public success shape.
    fn parse(body: &str, request: &ProjectCreateOptions) -> Result<Self, RuntimeError> {
        let value =
            serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_response())?;
        exact_keys(&value, &["project", "setup", "ingest"])?;
        let (project_id, setup_status, project_created_at) =
            parse_created_project(value.get("project"), request)?;
        let (setup_action_code, setup_action_target) = parse_created_setup(
            value.get("setup"),
            project_id.as_str(),
            setup_status.as_str(),
        )?;
        let ingest = parse_created_ingest(value.get("ingest"))?;

        Ok(Self {
            project_id,
            setup_status,
            project_created_at,
            setup_action_code,
            setup_action_target,
            ingest_id: ingest.id,
            ingest_label: ingest.label,
            ingest_created_at: ingest.created_at,
            ingest_expires_at: ingest.expires_at,
            token: ingest.token,
        })
    }
}

/// Validated one-time ingest credential from the create response.
struct CreatedIngest {
    /// Public key identifier.
    id: String,
    /// Display-safe key label.
    label: String,
    /// Creation timestamp.
    created_at: String,
    /// Expiration timestamp.
    expires_at: String,
    /// One-time secret.
    token: String,
}

/// Parses and binds the exact project object to the normalized request.
fn parse_created_project(
    value: Option<&serde_json::Value>,
    request: &ProjectCreateOptions,
) -> Result<(String, String, String), RuntimeError> {
    let project = object(value)?;
    exact_map_keys(
        project,
        &[
            "id",
            "name",
            "provider_project_id",
            "provider_project_slug",
            "provider",
            "is_active",
            "language",
            "setup_status",
            "setup_started_at",
            "first_telemetry_seen_at",
            "last_seen_at",
            "last_release",
            "last_environment",
            "created_at",
        ],
    )?;
    let project_id = required_safe(project, "id", 64)?;
    drop(required_safe(project, "provider_project_id", 256)?);
    optional_safe(project, "provider_project_slug", 256)?;
    drop(required_safe(project, "provider", 64)?);
    optional_safe(project, "language", 64)?;
    if !crate::ids::is_uuid(project_id.as_str())
        || required_safe(project, "name", 120)? != request.name
        || project
            .get("is_active")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
    {
        return Err(invalid_response());
    }
    let setup_status =
        validated_setup_status(required_safe(project, "setup_status", 32)?.as_str())?;
    optional_timestamp(project, "setup_started_at")?;
    optional_timestamp(project, "first_telemetry_seen_at")?;
    optional_timestamp(project, "last_seen_at")?;
    optional_safe(project, "last_release", 256)?;
    optional_safe(project, "last_environment", 128)?;
    let created_at = required_timestamp(project, "created_at")?;
    Ok((project_id, setup_status, created_at))
}

/// Parses the exact setup object and binds it to the returned project.
fn parse_created_setup(
    value: Option<&serde_json::Value>,
    project_id: &str,
    setup_status: &str,
) -> Result<(String, String), RuntimeError> {
    let setup = object(value)?;
    exact_map_keys(
        setup,
        &[
            "project_id",
            "status",
            "runtime",
            "source",
            "created_at",
            "setup_started_at",
            "first_telemetry_seen_at",
            "last_seen_at",
            "last_release",
            "last_environment",
            "last_signal",
            "next",
            "next_action",
        ],
    )?;
    optional_safe(setup, "runtime", 64)?;
    optional_setup_source(setup, "source")?;
    if required_safe(setup, "project_id", 64)? != project_id
        || validated_setup_status(required_safe(setup, "status", 32)?.as_str())? != setup_status
    {
        return Err(invalid_response());
    }
    drop(required_timestamp(setup, "created_at")?);
    optional_timestamp(setup, "setup_started_at")?;
    optional_timestamp(setup, "first_telemetry_seen_at")?;
    optional_timestamp(setup, "last_seen_at")?;
    optional_safe(setup, "last_release", 256)?;
    optional_safe(setup, "last_environment", 128)?;
    optional_last_signal(setup.get("last_signal"))?;
    drop(required_safe(setup, "next", 512)?);
    setup_action(setup.get("next_action"), setup_status)
}

/// Parses the exact one-time ingest credential object.
fn parse_created_ingest(value: Option<&serde_json::Value>) -> Result<CreatedIngest, RuntimeError> {
    let ingest = object(value)?;
    exact_map_keys(
        ingest,
        &[
            "id",
            "label",
            "kind",
            "token",
            "created_at",
            "expires_at",
            "next",
            "next_action",
        ],
    )?;
    if required_safe(ingest, "kind", 32)?.as_str() != "cli" {
        return Err(invalid_response());
    }
    drop(required_safe(ingest, "next", 512)?);
    drop(safe_action(ingest.get("next_action"))?);
    Ok(CreatedIngest {
        id: required_safe(ingest, "id", 128)?,
        label: required_safe(ingest, "label", 128)?,
        created_at: required_timestamp(ingest, "created_at")?,
        expires_at: required_timestamp(ingest, "expires_at")?,
        token: required_visible_secret(ingest, "token")?,
    })
}

/// Writes a CLI-owned success body only after the credential is durable.
fn write_success<W: std::io::Write>(
    created: &CreatedProject,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if json {
        let body = serde_json::json!({
            "status": "created",
            "project": {
                "id": created.project_id,
                "setup_status": created.setup_status,
                "created_at": created.project_created_at,
            },
            "setup": {
                "status": created.setup_status,
                "next_action": {
                    "code": created.setup_action_code,
                    "target": created.setup_action_target,
                }
            },
            "ingest_key": {
                "id": created.ingest_id,
                "label": created.ingest_label,
                "kind": "cli",
                "created_at": created.ingest_created_at,
                "expires_at": created.ingest_expires_at,
            },
            "checks": [
                {"check": "project", "status": "created"},
                {"check": "setup", "status": created.setup_status},
                {"check": "ingest_key", "status": "stored"},
            ],
            "next": "run logbrew doctor --project <project_id>",
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "LogBrew project created.")?;
        writeln!(output, "Project: {}", created.project_id)?;
        writeln!(output, "Setup: {}", created.setup_status)?;
        writeln!(output, "Ingest key: stored")?;
        writeln!(output, "Next: run logbrew doctor --project <project_id>")?;
    }
    Ok(())
}

/// Private retry state held while the request and key file are unresolved.
struct PendingRetry {
    /// Normalized API origin owning the attempt.
    origin: String,
    /// Exact serialized request body.
    request_body: String,
    /// Exact idempotency key.
    retry_key: String,
    /// Canonical local destination bound to the attempt.
    ingest_key_file: String,
}

impl PendingRetry {
    /// Parses the exact private retry-state shape.
    fn parse(value: &serde_json::Value) -> Result<Self, RuntimeError> {
        exact_keys(
            value,
            &[
                "version",
                "origin",
                "request_body",
                "retry_key",
                "ingest_key_file",
            ],
        )?;
        let object = object(Some(value))?;
        if object.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err(retry_state_invalid());
        }
        let origin = state_string(object, "origin", 2048)?;
        let request_body = state_string(object, "request_body", 4096)?;
        let retry_key = state_string(object, "retry_key", 128)?;
        let ingest_key_file = state_string(object, "ingest_key_file", 4096)?;
        if !valid_retry_key(retry_key.as_str()) {
            return Err(retry_state_invalid());
        }
        Ok(Self {
            origin,
            request_body,
            retry_key,
            ingest_key_file,
        })
    }

    /// Serializes the exact private retry-state shape.
    fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "origin": self.origin,
            "request_body": self.request_body,
            "retry_key": self.retry_key,
            "ingest_key_file": self.ingest_key_file,
        })
    }
}

/// Prepared retry plus whether an existing exact token file may be verified.
struct PreparedRetry {
    /// Exact serialized request body.
    request_body: String,
    /// Exact idempotency key.
    retry_key: String,
    /// Whether an already-written destination may be verified.
    allow_existing_target: bool,
}

/// Cross-process project-create lock and private retry directory.
struct BootstrapLock {
    /// Open file whose lifetime holds the advisory lock.
    _file: std::fs::File,
    /// Home directory owning private retry state.
    home: std::path::PathBuf,
    /// Owner-only directory containing retry state.
    private_dir: std::path::PathBuf,
}

impl BootstrapLock {
    /// Acquires the project-bootstrap lock in an owner-only directory.
    fn exclusive(home: &std::path::Path) -> Result<Self, RuntimeError> {
        let home_metadata = safe_directory_metadata(home, None)?;
        let private_dir = home.join(PRIVATE_DIR);
        match std::fs::symlink_metadata(private_dir.as_path()) {
            Ok(metadata) => {
                validate_private_directory(&metadata, owner_id(&home_metadata))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(private_dir.as_path())
                    .map_err(|_| retry_state_unavailable())?;
                set_private_directory_permissions(private_dir.as_path())?;
            }
            Err(_) => return Err(retry_state_unavailable()),
        }
        let mut options = std::fs::OpenOptions::new();
        let _options = options.read(true).write(true).create(true);
        set_private_open_mode(&mut options);
        let file = options
            .open(private_dir.join(LOCK_FILE))
            .map_err(|_| retry_state_unavailable())?;
        set_private_file_permissions(&file)?;
        fs2::FileExt::lock_exclusive(&file).map_err(|_| retry_state_unavailable())?;
        let private_dir =
            std::fs::canonicalize(private_dir.as_path()).map_err(|_| retry_state_unavailable())?;
        Ok(Self {
            _file: file,
            home: home.to_path_buf(),
            private_dir,
        })
    }

    /// Reuses an exact pending attempt or durably records a new one.
    fn prepare(
        &self,
        origin: &str,
        body: &str,
        target: &std::path::Path,
        abandon: bool,
    ) -> Result<PreparedRetry, RuntimeError> {
        if target == self.private_dir.join(RETRY_FILE) || target == self.private_dir.join(LOCK_FILE)
        {
            return Err(invalid_key_destination());
        }
        let expected_owner = home_owner(self.home.as_path())?;
        if abandon {
            let _target_absent = validate_target_before_request(target, false, expected_owner)?;
            self.clear_retry()?;
        }
        let target_string = target
            .to_str()
            .ok_or_else(invalid_key_destination)?
            .to_owned();
        if let Some(pending) = self.read_retry()? {
            if pending.origin != origin
                || pending.request_body != body
                || pending.ingest_key_file != target_string
            {
                return Err(retry_mismatch());
            }
            let exists = validate_target_before_request(target, true, expected_owner)?;
            return Ok(PreparedRetry {
                request_body: pending.request_body,
                retry_key: pending.retry_key,
                allow_existing_target: exists,
            });
        }
        let _target_absent = validate_target_before_request(target, false, expected_owner)?;
        let pending = PendingRetry {
            origin: origin.to_owned(),
            request_body: body.to_owned(),
            retry_key: random_retry_key()?,
            ingest_key_file: target_string,
        };
        self.write_retry(&pending)?;
        Ok(PreparedRetry {
            request_body: pending.request_body,
            retry_key: pending.retry_key,
            allow_existing_target: false,
        })
    }

    /// Reads and validates an optional pending retry.
    fn read_retry(&self) -> Result<Option<PendingRetry>, RuntimeError> {
        let path = self.private_dir.join(RETRY_FILE);
        let metadata = match std::fs::symlink_metadata(path.as_path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(retry_state_invalid()),
        };
        validate_private_file(&metadata, home_owner(self.home.as_path())?)?;
        if metadata.len() > 16 * 1024 {
            return Err(retry_state_invalid());
        }
        let contents = std::fs::read_to_string(path).map_err(|_| retry_state_invalid())?;
        let value = serde_json::from_str::<serde_json::Value>(contents.as_str())
            .map_err(|_| retry_state_invalid())?;
        PendingRetry::parse(&value).map(Some)
    }

    /// Atomically persists a pending retry before network activity.
    fn write_retry(&self, pending: &PendingRetry) -> Result<(), RuntimeError> {
        let mut file = atomic_write_file::AtomicWriteFile::open(self.private_dir.join(RETRY_FILE))
            .map_err(|_| retry_state_unavailable())?;
        set_private_file_permissions(file.as_file())?;
        file.write_all(pending.value().to_string().as_bytes())
            .map_err(|_| retry_state_unavailable())?;
        file.as_file()
            .sync_all()
            .map_err(|_| retry_state_unavailable())?;
        file.commit().map_err(|_| retry_state_unavailable())?;
        sync_directory(self.private_dir.as_path())?;
        Ok(())
    }

    /// Removes durable retry state after key persistence or explicit abandonment.
    fn clear_retry(&self) -> Result<(), RuntimeError> {
        match std::fs::remove_file(self.private_dir.join(RETRY_FILE)) {
            Ok(()) => sync_directory(self.private_dir.as_path()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(retry_state_unavailable()),
        }
    }
}

/// Resolves one explicit path through a real, non-symlink private parent.
fn resolve_target(
    env: &CliEnvironment,
    value: &str,
    expected_owner: u64,
) -> Result<std::path::PathBuf, RuntimeError> {
    let raw = std::path::PathBuf::from(value);
    let joined = if raw.is_absolute() {
        raw
    } else {
        env.cwd
            .as_ref()
            .ok_or_else(invalid_key_destination)?
            .join(raw)
    };
    let file_name = joined
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(invalid_key_destination)?;
    let parent = joined.parent().ok_or_else(invalid_key_destination)?;
    reject_symlink_ancestors(parent, expected_owner)?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(|_| invalid_key_destination())?;
    Ok(canonical_parent.join(file_name))
}

/// Validates destination privacy and reports whether a retry target exists.
fn validate_target_before_request(
    target: &std::path::Path,
    allow_existing: bool,
    expected_owner: u64,
) -> Result<bool, RuntimeError> {
    let parent = target.parent().ok_or_else(invalid_key_destination)?;
    let parent_metadata =
        std::fs::symlink_metadata(parent).map_err(|_| invalid_key_destination())?;
    validate_private_directory(&parent_metadata, expected_owner)?;
    match std::fs::symlink_metadata(target) {
        Ok(metadata) if allow_existing => {
            validate_private_file(&metadata, expected_owner)?;
            Ok(true)
        }
        Ok(_) => Err(key_destination_exists()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(invalid_key_destination()),
    }
}

/// Persists or verifies the one-time token without overwriting any file.
fn persist_ingest_key(
    target: &std::path::Path,
    token: &str,
    allow_existing: bool,
    expected_owner: u64,
) -> Result<(), RuntimeError> {
    if allow_existing {
        let metadata = std::fs::symlink_metadata(target).map_err(|_| key_storage_ambiguous())?;
        validate_private_file(&metadata, expected_owner)?;
        if metadata.len() > 4096 {
            return Err(key_storage_ambiguous());
        }
        let stored = std::fs::read_to_string(target).map_err(|_| key_storage_ambiguous())?;
        if stored != token {
            return Err(key_destination_exists());
        }
        sync_directory(target.parent().ok_or_else(invalid_key_destination)?)?;
        return Ok(());
    }

    let parent = target.parent().ok_or_else(invalid_key_destination)?;
    let (temp_path, mut file) = create_private_temp(parent)?;
    let result = (|| {
        file.write_all(token.as_bytes())
            .map_err(|_| key_storage_ambiguous())?;
        file.sync_all().map_err(|_| key_storage_ambiguous())?;
        std::fs::hard_link(temp_path.as_path(), target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                key_destination_exists()
            } else {
                key_storage_ambiguous()
            }
        })?;
        std::fs::remove_file(temp_path.as_path()).map_err(|_| key_storage_ambiguous())?;
        sync_directory(parent)
    })();
    if result.is_err() {
        drop(std::fs::remove_file(temp_path));
    }
    result
}

/// Creates a random owner-only temporary key file in the destination directory.
fn create_private_temp(
    parent: &std::path::Path,
) -> Result<(std::path::PathBuf, std::fs::File), RuntimeError> {
    for _attempt in 0..8 {
        let suffix = random_hex::<16>()?;
        let path = parent.join(format!(".logbrew-ingest-{suffix}.tmp"));
        let mut options = std::fs::OpenOptions::new();
        let _options = options.write(true).create_new(true);
        set_private_open_mode(&mut options);
        match options.open(path.as_path()) {
            Ok(file) => {
                set_private_file_permissions(&file)?;
                return Ok((path, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(key_storage_ambiguous()),
        }
    }
    Err(key_storage_ambiguous())
}

/// Returns a 64-character visible idempotency key.
fn random_retry_key() -> Result<String, RuntimeError> {
    random_hex::<32>()
}

/// Generates lowercase random hexadecimal bytes.
fn random_hex<const N: usize>() -> Result<String, RuntimeError> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|_| retry_state_unavailable())?;
    let mut value = String::with_capacity(N * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(value)
}

/// Checks the deployed visible-ASCII idempotency-key boundary.
fn valid_retry_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

/// Converts one validated backend error to fixed local guidance.
fn safe_api_error(
    status: u16,
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    let value =
        serde_json::from_str::<serde_json::Value>(body).map_err(|_| invalid_error_response())?;
    let object = object(Some(&value)).map_err(|_| invalid_error_response())?;
    let allowed_keys = if status == 429 {
        &[
            "error",
            "code",
            "next",
            "next_action",
            "limit",
            "retry_after_seconds",
        ][..]
    } else {
        &["error", "code", "next", "next_action"][..]
    };
    if object
        .keys()
        .any(|key| !allowed_keys.contains(&key.as_str()))
        || !["error", "code", "next", "next_action"]
            .iter()
            .all(|key| object.contains_key(*key))
        || required_safe(object, "error", 512).is_err()
        || required_safe(object, "next", 512).is_err()
        || safe_action(object.get("next_action")).is_err()
    {
        return Err(invalid_error_response());
    }
    if status == 429
        && ["limit", "retry_after_seconds"].iter().any(|key| {
            object
                .get(*key)
                .is_some_and(|value| value.as_u64().is_none())
        })
    {
        return Err(invalid_error_response());
    }
    let code = required_safe(object, "code", 64).map_err(|_| invalid_error_response())?;
    let (safe_code, safe_error, safe_next, action_code, action_target) =
        match (status, code.as_str()) {
            (401, "unauthorized") => (
                "unauthorized",
                "authentication failed",
                "run logbrew login",
                "sign_in",
                "auth",
            ),
            (409, "idempotency_conflict") => (
                "idempotency_conflict",
                "project creation retry conflict",
                "rerun with --abandon-retry only when intentionally discarding the pending attempt",
                "use_new_idempotency_key",
                "request",
            ),
            (422, "validation_failed" | "invalid_json") => (
                code.as_str(),
                "project creation request was rejected",
                "correct project fields, then use --abandon-retry to start the corrected request",
                "fix_request",
                "request",
            ),
            (429, "project_limit_exceeded") => (
                "project_limit_exceeded",
                "project limit reached",
                "remove an unused project or review account limits",
                "review_project_limit",
                "projects",
            ),
            (429, "rate_limited") => (
                "rate_limited",
                "project creation is rate limited",
                "retry the exact same command later",
                "retry",
                "request",
            ),
            (500..=599, _) => (
                "server_error",
                "project creation could not be confirmed",
                "retry the exact same command to reuse the pending request",
                "retry",
                "request",
            ),
            _ => return Err(invalid_error_response()),
        };
    let safe_body = serde_json::json!({
        "error": safe_error,
        "code": safe_code,
        "next": safe_next,
        "next_action": {"code": action_code, "target": action_target},
    });
    Ok(RuntimeError::Api {
        status,
        body: safe_body.to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    })
}

/// Requires one JSON object to contain exactly the named keys.
fn exact_keys(value: &serde_json::Value, keys: &[&str]) -> Result<(), RuntimeError> {
    exact_map_keys(object(Some(value))?, keys)
}

/// Requires one parsed object to contain exactly the named keys.
fn exact_map_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Result<(), RuntimeError> {
    if object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)) {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Extracts a JSON object or returns the fixed invalid-response error.
fn object(
    value: Option<&serde_json::Value>,
) -> Result<&serde_json::Map<String, serde_json::Value>, RuntimeError> {
    value
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid_response)
}

/// Extracts one nonempty bounded display-safe string.
fn required_safe(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    limit: usize,
) -> Result<String, RuntimeError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| safe_text(value, limit) && !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(invalid_response)
}

/// Validates one nullable bounded display-safe string.
fn optional_safe(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    limit: usize,
) -> Result<(), RuntimeError> {
    match object.get(key) {
        Some(serde_json::Value::Null) => Ok(()),
        Some(serde_json::Value::String(value)) if safe_text(value, limit) => Ok(()),
        Some(_) | None => Err(invalid_response()),
    }
}

/// Validates one nullable canonical setup source.
fn optional_setup_source(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<(), RuntimeError> {
    match object.get(key) {
        Some(serde_json::Value::Null) => Ok(()),
        Some(serde_json::Value::String(value))
            if matches!(value.as_str(), "api" | "cli" | "sdk") =>
        {
            Ok(())
        }
        Some(_) | None => Err(invalid_response()),
    }
}

/// Extracts one bounded visible-ASCII secret without rendering it.
fn required_visible_secret(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, RuntimeError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| {
            (1..=4096).contains(&value.len())
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        })
        .map(ToOwned::to_owned)
        .ok_or_else(invalid_response)
}

/// Extracts one bounded RFC3339 timestamp.
fn required_timestamp(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, RuntimeError> {
    let value = required_safe(object, key, 64)?;
    is_rfc3339(value.as_str())
        .then_some(value)
        .ok_or_else(invalid_response)
}

/// Validates one nullable RFC3339 timestamp.
fn optional_timestamp(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<(), RuntimeError> {
    match object.get(key) {
        Some(serde_json::Value::Null) => Ok(()),
        Some(serde_json::Value::String(value)) if is_rfc3339(value) => Ok(()),
        Some(_) | None => Err(invalid_response()),
    }
}

/// Validates one nullable exact project last-signal shape.
fn optional_last_signal(value: Option<&serde_json::Value>) -> Result<(), RuntimeError> {
    let Some(value) = value else {
        return Err(invalid_response());
    };
    if value.is_null() {
        return Ok(());
    }
    exact_keys(value, &["kind", "id", "message", "occurred_at"])?;
    let object = object(Some(value))?;
    drop(required_safe(object, "kind", 64)?);
    optional_safe(object, "id", 256)?;
    optional_safe(object, "message", 512)?;
    drop(required_timestamp(object, "occurred_at")?);
    Ok(())
}

/// Validates one canonical project setup status.
fn validated_setup_status(value: &str) -> Result<String, RuntimeError> {
    matches!(
        value,
        "created" | "setup_started" | "sdk_seen" | "first_telemetry_seen" | "active"
    )
    .then(|| value.to_owned())
    .ok_or_else(invalid_response)
}

/// Binds one setup action to its canonical status.
fn setup_action(
    value: Option<&serde_json::Value>,
    status: &str,
) -> Result<(String, String), RuntimeError> {
    let (code, target) = safe_action(value)?;
    let valid = match status {
        "created" => code == "choose_setup_path" && target == "project_setup",
        "setup_started" | "sdk_seen" => {
            code == "send_first_telemetry" && target == "telemetry_ingest"
        }
        "first_telemetry_seen" | "active" => {
            code == "review_project_dashboard" && target == "project_dashboard"
        }
        _ => false,
    };
    valid.then_some((code, target)).ok_or_else(invalid_response)
}

/// Parses one exact bounded action object.
fn safe_action(value: Option<&serde_json::Value>) -> Result<(String, String), RuntimeError> {
    let value = value.ok_or_else(invalid_response)?;
    exact_keys(value, &["code", "target"])?;
    let object = object(Some(value))?;
    Ok((
        required_safe(object, "code", 64)?,
        required_safe(object, "target", 64)?,
    ))
}

/// Extracts one bounded private-state string.
fn state_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    limit: usize,
) -> Result<String, RuntimeError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
        })
        .map(ToOwned::to_owned)
        .ok_or_else(retry_state_invalid)
}

/// Rejects control and display-direction characters in server text.
fn safe_text(value: &str, limit: usize) -> bool {
    value.chars().count() <= limit
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

/// Validates an RFC3339 timestamp without adding a time dependency.
fn is_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let Some(year) = bytes.get(0..4).and_then(digits_u32) else {
        return false;
    };
    let Some(month) = bytes.get(5..7).and_then(digits_u32) else {
        return false;
    };
    let Some(day) = bytes.get(8..10).and_then(digits_u32) else {
        return false;
    };
    let Some(hour) = bytes.get(11..13).and_then(digits_u32) else {
        return false;
    };
    let Some(minute) = bytes.get(14..16).and_then(digits_u32) else {
        return false;
    };
    let Some(second) = bytes.get(17..19).and_then(digits_u32) else {
        return false;
    };
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return false;
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    match bytes.get(index) {
        Some(b'Z') => index + 1 == bytes.len(),
        Some(b'+' | b'-') => {
            let Some(offset_hour) = bytes.get(index + 1..index + 3).and_then(digits_u32) else {
                return false;
            };
            let Some(offset_minute) = bytes.get(index + 4..index + 6).and_then(digits_u32) else {
                return false;
            };
            bytes.get(index + 3) == Some(&b':')
                && index + 6 == bytes.len()
                && offset_hour <= 23
                && offset_minute <= 59
        }
        Some(_) | None => false,
    }
}

/// Parses an ASCII digit slice as an unsigned integer.
fn digits_u32(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(*byte - b'0'))
    })
}

/// Returns the number of days in one Gregorian month.
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Normalizes and validates the configured HTTP API origin.
fn normalized_origin(base_url: &str) -> Result<String, RuntimeError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| invalid_request())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_request());
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(path.as_str());
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// Rejects symlinks while walking the user-owned path portion.
fn reject_symlink_ancestors(
    path: &std::path::Path,
    expected_owner: u64,
) -> Result<(), RuntimeError> {
    for ancestor in path.ancestors() {
        let metadata =
            std::fs::symlink_metadata(ancestor).map_err(|_| invalid_key_destination())?;
        if metadata.file_type().is_symlink() {
            return Err(invalid_key_destination());
        }
        if owner_id(&metadata) != expected_owner {
            break;
        }
    }
    Ok(())
}

/// Reads directory metadata without following a final symlink.
fn safe_directory_metadata(
    path: &std::path::Path,
    expected_owner: Option<u64>,
) -> Result<std::fs::Metadata, RuntimeError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| retry_state_unavailable())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(retry_state_unavailable());
    }
    if let Some(owner) = expected_owner
        && owner_id(&metadata) != owner
    {
        return Err(retry_state_unavailable());
    }
    Ok(metadata)
}

/// Requires an owner-only real directory.
fn validate_private_directory(
    metadata: &std::fs::Metadata,
    expected_owner: u64,
) -> Result<(), RuntimeError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || owner_id(metadata) != expected_owner
        || !private_directory_permissions(metadata)
    {
        return Err(invalid_key_destination());
    }
    Ok(())
}

/// Requires an owner-only regular file.
fn validate_private_file(
    metadata: &std::fs::Metadata,
    expected_owner: u64,
) -> Result<(), RuntimeError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || owner_id(metadata) != expected_owner
        || !private_file_permissions(metadata)
    {
        return Err(invalid_key_destination());
    }
    Ok(())
}

/// Returns the Unix owner identifier for one filesystem object.
#[cfg(unix)]
fn owner_id(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    u64::from(metadata.uid())
}

/// Uses a single synthetic owner on platforms without Unix user identifiers.
#[cfg(not(unix))]
fn owner_id(_metadata: &std::fs::Metadata) -> u64 {
    0
}

/// Reports that Unix ownership and mode checks can enforce the storage contract.
#[cfg(unix)]
const fn private_storage_supported() -> bool {
    true
}

/// Fails closed where this build cannot prove owner-only ACL semantics.
#[cfg(not(unix))]
const fn private_storage_supported() -> bool {
    false
}

/// Checks that a Unix directory grants no group or other access.
#[cfg(unix)]
fn private_directory_permissions(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode().trailing_zeros() >= 6
}

/// Relies on platform ACL inheritance where Unix mode bits are unavailable.
#[cfg(not(unix))]
fn private_directory_permissions(_metadata: &std::fs::Metadata) -> bool {
    true
}

/// Checks that a Unix file is non-executable and grants no group or other access.
#[cfg(unix)]
fn private_file_permissions(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode().trailing_zeros() >= 7
}

/// Relies on platform ACL inheritance where Unix mode bits are unavailable.
#[cfg(not(unix))]
fn private_file_permissions(_metadata: &std::fs::Metadata) -> bool {
    true
}

/// Sets the restrictive creation mode for Unix files.
#[cfg(unix)]
fn set_private_open_mode(options: &mut std::fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    let _options = options.mode(0o600);
}

/// Leaves file creation to platform ACL inheritance on non-Unix systems.
#[cfg(not(unix))]
fn set_private_open_mode(_options: &mut std::fs::OpenOptions) {}

/// Applies owner read/write permissions to an open Unix file.
#[cfg(unix)]
fn set_private_file_permissions(file: &std::fs::File) -> Result<(), RuntimeError> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|_| retry_state_unavailable())
}

/// Leaves file permissions to platform ACL inheritance on non-Unix systems.
#[cfg(not(unix))]
fn set_private_file_permissions(_file: &std::fs::File) -> Result<(), RuntimeError> {
    Ok(())
}

/// Applies owner-only permissions to a Unix directory.
#[cfg(unix)]
fn set_private_directory_permissions(path: &std::path::Path) -> Result<(), RuntimeError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| retry_state_unavailable())
}

/// Leaves directory permissions to platform ACL inheritance on non-Unix systems.
#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &std::path::Path) -> Result<(), RuntimeError> {
    Ok(())
}

/// Flushes one Unix directory entry update to durable storage.
#[cfg(unix)]
fn sync_directory(path: &std::path::Path) -> Result<(), RuntimeError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| key_storage_ambiguous())
}

/// Treats directory metadata updates as complete on non-Unix systems.
#[cfg(not(unix))]
fn sync_directory(_path: &std::path::Path) -> Result<(), RuntimeError> {
    Ok(())
}

/// Returns the owner identifier for the configured home directory.
fn home_owner(home: &std::path::Path) -> Result<u64, RuntimeError> {
    Ok(owner_id(&safe_directory_metadata(home, None)?))
}

/// Fixed recovery for unavailable private retry storage.
const fn retry_state_unavailable() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "secure project retry state is unavailable",
        next: "check the private home directory and retry the exact same command",
    }
}

/// Fixed recovery when this build cannot prove owner-only key storage.
const fn private_storage_unavailable() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "secure ingest key storage is unavailable on this platform",
        next: "run project creation on a platform with enforceable owner-only file permissions",
    }
}

/// Fixed recovery for unreadable or malformed local authentication state.
const fn local_auth_unavailable() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "local authentication is unavailable",
        next: "run logbrew login",
    }
}

/// Fixed recovery for malformed private retry state.
const fn retry_state_invalid() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "pending project creation state is invalid",
        next: "inspect local permissions, then use --abandon-retry to start a new attempt",
    }
}

/// Fixed recovery for a command that differs from a pending retry.
const fn retry_mismatch() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "pending project creation does not match this request",
        next: "retry the exact original command or use --abandon-retry to start a new attempt",
    }
}

/// Fixed recovery for an invalid API origin or request serialization.
const fn invalid_request() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project creation request is invalid",
        next: "check project create arguments and retry",
    }
}

/// Fixed recovery for an unsafe key-file destination.
const fn invalid_key_destination() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "ingest key destination is not private",
        next: "choose a new file inside an existing owner-only directory",
    }
}

/// Fixed recovery for a destination that would be overwritten.
const fn key_destination_exists() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "ingest key destination already exists",
        next: "choose a new --ingest-key-file path without overwriting the existing file",
    }
}

/// Fixed recovery when durable key persistence is uncertain.
const fn key_storage_ambiguous() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "ingest key storage could not be confirmed",
        next: "retry the exact same command to reuse the pending response",
    }
}

/// Fixed recovery for transport failures with an exact pending retry.
const fn transport_unavailable() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project creation could not confirm the server result",
        next: "retry the exact same command to reuse the pending request",
    }
}

/// Fixed recovery for a malformed successful response.
const fn invalid_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project creation returned an invalid response",
        next: "retry the exact same command; if it repeats, report the public response contract",
    }
}

/// Fixed recovery for a malformed typed error response.
const fn invalid_error_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project creation returned an invalid error response",
        next: "retry the exact same command; if it repeats, report the public error contract",
    }
}
