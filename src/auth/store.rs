//! Locked local credential persistence.

use crate::RuntimeError;
use std::io::Write as _;

/// Canonical atomically replaced credential session file.
const SESSION_FILE: &str = "session.json";

/// Access and refresh tokens read while holding the store lock.
pub(super) struct StoredCredentials {
    /// Current access token.
    pub(super) access_token: String,
    /// Current refresh token.
    pub(super) refresh_token: String,
    /// API base that owns this credential pair.
    origin: String,
}

/// Exclusive cross-process credential-store lock.
pub(super) struct CredentialStoreLock {
    /// Open lock file whose lifetime holds the advisory lock.
    _file: std::fs::File,
    /// Directory containing all CLI auth state.
    auth_dir: std::path::PathBuf,
}

impl CredentialStoreLock {
    /// Acquires an exclusive lock for credential refresh or deletion.
    pub(super) fn exclusive(home: &std::path::Path) -> Result<Self, RuntimeError> {
        let auth_dir = auth_dir(home);
        ensure_auth_dir(auth_dir.as_path())?;
        let file = open_lock_file(auth_dir.as_path())?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Self {
            _file: file,
            auth_dir,
        })
    }

    /// Acquires the lock only when local auth state already exists.
    pub(super) fn exclusive_if_present(
        home: &std::path::Path,
    ) -> Result<Option<Self>, RuntimeError> {
        if !auth_dir(home).is_dir() {
            return Ok(None);
        }
        Self::exclusive(home).map(Some)
    }

    /// Reads a complete pair while this exclusive lock is held.
    pub(super) fn read_credentials(
        &self,
        expected_origin: &str,
    ) -> Result<Option<StoredCredentials>, RuntimeError> {
        let credentials = read_session(self.auth_dir.as_path())?;
        if let Some(credentials) = &credentials {
            ensure_matching_origin(credentials.origin.as_str(), expected_origin)?;
        }
        Ok(credentials)
    }

    /// Reports whether a canonical refresh-backed session exists while locked.
    pub(super) fn has_refresh_backed_session(&self) -> Result<bool, RuntimeError> {
        Ok(self.auth_dir.join(SESSION_FILE).try_exists()?)
    }

    /// Atomically replaces the complete bound session while this lock is held.
    pub(super) fn persist(
        &self,
        access_token: &str,
        refresh_token: &str,
        origin: &str,
    ) -> Result<(), RuntimeError> {
        let session = serde_json::json!({
            "access_token": access_token.trim(),
            "refresh_token": refresh_token.trim(),
            "origin": origin.trim(),
        });
        write_secret_atomically(
            self.auth_dir.as_path(),
            SESSION_FILE,
            session.to_string().as_str(),
        )
    }

    /// Removes every supported local credential representation while locked.
    pub(super) fn remove_credentials(&self) -> Result<bool, RuntimeError> {
        let session_removed = remove_if_present(self.auth_dir.join(SESSION_FILE).as_path())?;
        let access_removed = remove_if_present(self.auth_dir.join("token").as_path())?;
        let refresh_removed = remove_if_present(self.auth_dir.join("refresh-token").as_path())?;
        let origin_removed = remove_if_present(self.auth_dir.join("auth-origin").as_path())?;
        Ok(session_removed || access_removed || refresh_removed || origin_removed)
    }
}

/// Reads the access token under a shared cross-process lock.
pub(super) fn read_access_token(
    home: &std::path::Path,
    expected_origin: &str,
) -> Result<String, RuntimeError> {
    let auth_dir = auth_dir(home);
    if !auth_dir.is_dir() {
        return Err(RuntimeError::MissingToken);
    }
    ensure_auth_dir(auth_dir.as_path())?;
    let lock_file = open_lock_file(auth_dir.as_path())?;
    fs2::FileExt::lock_shared(&lock_file)?;
    if let Some(credentials) = read_session(auth_dir.as_path())? {
        ensure_matching_origin(credentials.origin.as_str(), expected_origin)?;
        return Ok(credentials.access_token);
    }

    let access_token = read_secret(auth_dir.join("token").as_path())?;
    let refresh_token = read_secret(auth_dir.join("refresh-token").as_path())?;
    let origin = read_secret(auth_dir.join("auth-origin").as_path())?;
    if refresh_token.is_some() || origin.is_some() {
        return Err(unbound_refresh_credentials());
    }
    access_token.ok_or(RuntimeError::MissingToken)
}

/// Returns the private auth-state directory for one home directory.
fn auth_dir(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".logbrew")
}

/// Creates and secures the private auth-state directory.
fn ensure_auth_dir(path: &std::path::Path) -> Result<(), RuntimeError> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Opens the stable advisory-lock file with private permissions.
fn open_lock_file(auth_dir: &std::path::Path) -> Result<std::fs::File, RuntimeError> {
    let mut options = std::fs::OpenOptions::new();
    let _options = options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        let _options = options.mode(0o600);
    }
    let file = options.open(auth_dir.join("credentials.lock"))?;
    secure_file_permissions(&file)?;
    Ok(file)
}

/// Reads one trimmed secret or reports that it is absent/empty.
fn read_secret(path: &std::path::Path) -> Result<Option<String>, RuntimeError> {
    let secret = match std::fs::read_to_string(path) {
        Ok(secret) => secret,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(RuntimeError::Io(error)),
    };
    let secret = secret.trim();
    secure_path_permissions(path)?;
    Ok((!secret.is_empty()).then(|| secret.to_owned()))
}

/// Parses the exact canonical session shape from private local storage.
fn read_session(auth_dir: &std::path::Path) -> Result<Option<StoredCredentials>, RuntimeError> {
    let Some(session) = read_secret(auth_dir.join(SESSION_FILE).as_path())? else {
        return Ok(None);
    };
    let value = serde_json::from_str::<serde_json::Value>(session.as_str())
        .map_err(|_| invalid_credentials())?;
    let Some(object) = value.as_object().filter(|object| object.len() == 3) else {
        return Err(invalid_credentials());
    };
    let access_token = session_field(object, "access_token")?;
    let refresh_token = session_field(object, "refresh_token")?;
    let origin = session_field(object, "origin")?;
    Ok(Some(StoredCredentials {
        access_token,
        refresh_token,
        origin,
    }))
}

/// Extracts one required non-empty session field.
fn session_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, RuntimeError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(invalid_credentials)
}

/// Applies private permissions to an open credential-state file.
fn secure_file_permissions(file: &std::fs::File) -> Result<(), RuntimeError> {
    #[cfg(not(unix))]
    let _ = file;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Applies private permissions to an existing credential file.
fn secure_path_permissions(path: &std::path::Path) -> Result<(), RuntimeError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Writes one private token file through a same-directory temporary file.
fn write_secret_atomically(
    auth_dir: &std::path::Path,
    name: &str,
    secret: &str,
) -> Result<(), RuntimeError> {
    if secret.trim().is_empty() {
        return Err(invalid_credentials());
    }
    let mut file = atomic_write_file::AtomicWriteFile::open(auth_dir.join(name))?;
    secure_file_permissions(file.as_file())?;
    writeln!(file, "{}", secret.trim())?;
    file.commit()?;
    Ok(())
}

/// Removes a credential if present.
fn remove_if_present(path: &std::path::Path) -> Result<bool, RuntimeError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(RuntimeError::Io(error)),
    }
}

/// Returns a stable error for malformed local credential input.
const fn invalid_credentials() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "authentication credentials were invalid",
        next: "run logbrew login",
    }
}

/// Rejects a persisted session that belongs to another API base.
fn ensure_matching_origin(origin: &str, expected_origin: &str) -> Result<(), RuntimeError> {
    if origin == expected_origin {
        Ok(())
    } else {
        Err(origin_mismatch())
    }
}

/// Returns a stable error for mismatched persisted session ownership.
const fn origin_mismatch() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "local authentication belongs to a different API",
        next: "run logbrew login for the configured API",
    }
}

/// Returns a stable migration error instead of using an unbound refresh token.
const fn unbound_refresh_credentials() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "local refresh credentials are missing API ownership",
        next: "run logbrew login",
    }
}
