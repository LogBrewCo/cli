//! Shared process, environment, and credential helpers for integration targets.

use super::MockServer;

/// Erases the command future so each test compiles one shared async state machine.
pub(crate) fn execute_command<'a>(
    command: &'a logbrew_cli::Command,
    environment: &'a logbrew_cli::CliEnvironment,
    output: &'a mut Vec<u8>,
) -> futures_util::future::BoxFuture<'a, Result<(), logbrew_cli::RuntimeError>> {
    Box::pin(logbrew_cli::execute_command(command, environment, output))
}

pub(crate) async fn run_cli(
    server: &MockServer,
    args: &[&str],
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let mut command = cli_command(server);
    let _command = command.env("HOME", std::env::temp_dir()).args(args);
    run_cli_command(command).await
}

pub(crate) fn cli_command(server: &MockServer) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_logbrew"));
    let _command = command
        .env_clear()
        .env("LOGBREW_API_URL", server.uri())
        .env("LOGBREW_TOKEN", "account-token");
    command
}

pub(crate) async fn run_cli_command(
    mut command: std::process::Command,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    Ok(tokio::task::spawn_blocking(move || command.output()).await??)
}

/// Bounds small trusted native checks, killing and reaping on failure.
pub(crate) fn run_bounded(
    command: &mut std::process::Command,
) -> Result<std::process::Output, std::io::Error> {
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let error = loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output(),
            Err(error) => break error,
            Ok(None) if std::time::Instant::now() >= deadline => {
                break std::io::Error::new(std::io::ErrorKind::TimedOut, "native check timed out");
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    };
    child.kill()?;
    let _status = child.wait()?;
    Err(error)
}

pub(crate) fn assert_cli_success(process: &std::process::Output) {
    assert!(
        process.status.success(),
        "built binary failed: {}",
        String::from_utf8_lossy(process.stderr.as_slice())
    );
    assert!(process.stderr.is_empty(), "successful CLI wrote to stderr");
}

/// Require a failed process, empty stdout, and the exact structured error code.
pub(crate) fn cli_json_error(
    process: std::process::Output,
    code: &str,
) -> crate::TestResult<String> {
    assert!(!process.status.success(), "invalid command succeeded");
    assert!(process.stdout.is_empty(), "failed command emitted stdout");
    let text = String::from_utf8(process.stderr)?;
    let error: serde_json::Value = serde_json::from_str(&text)?;
    assert_eq!(error["error"], code, "incorrect error code");
    Ok(text)
}

pub(crate) fn authenticated_env(
    server: &MockServer,
    token: &str,
    home_name: Option<&str>,
) -> logbrew_cli::CliEnvironment {
    test_env(
        server,
        Some(token),
        home_name.map(|name| std::env::temp_dir().join(format!("logbrew-{name}"))),
    )
}

pub(crate) fn test_env(
    server: &MockServer,
    token: Option<&str>,
    home: Option<std::path::PathBuf>,
) -> logbrew_cli::CliEnvironment {
    logbrew_cli::CliEnvironment {
        base_url: server.uri(),
        token: token.map(str::to_owned),
        home,
        cwd: None,
    }
}

pub(crate) fn isolated_home(
    prefix: &str,
    label: &str,
) -> Result<std::path::PathBuf, std::io::Error> {
    let path = std::env::temp_dir().join(format!("{prefix}-{label}-{}", std::process::id()));
    if let Err(error) = std::fs::remove_dir_all(path.as_path())
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error);
    }
    std::fs::create_dir_all(path.as_path())?;
    Ok(path)
}

pub(crate) fn write_test_session(
    home: &std::path::Path,
    origin: &str,
    access_token: &str,
    refresh_token: &str,
) -> Result<std::path::PathBuf, std::io::Error> {
    let auth_dir = home.join(".logbrew");
    std::fs::create_dir_all(auth_dir.as_path())?;
    let path = auth_dir.join("session.json");
    let session = serde_json::json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "origin": origin,
    });
    std::fs::write(path.as_path(), session.to_string())?;
    Ok(path)
}

pub(crate) fn secure_directory(path: &std::path::Path) -> Result<(), std::io::Error> {
    set_mode(path, 0o700)
}

pub(crate) fn set_private_file_mode(path: &std::path::Path) -> Result<(), std::io::Error> {
    set_mode(path, 0o600)
}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &std::path::Path, _mode: u32) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn assert_private_file(path: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(
        std::fs::metadata(path)?.permissions().mode() & 0o777,
        0o600,
        "test credential must remain owner-only"
    );
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn assert_private_file(path: &std::path::Path) -> Result<(), std::io::Error> {
    assert!(path.is_file(), "test credential must be a regular file");
    Ok(())
}

pub(crate) async fn run_command<const N: usize>(
    server: &MockServer,
    args: [&str; N],
    home_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    logbrew_cli::execute_command(
        &logbrew_cli::parse_command(args)?,
        &authenticated_env(server, "test-token", Some(home_name)),
        &mut output,
    )
    .await?;
    Ok(String::from_utf8(output)?)
}

/// One consistent analytics detail block for a window that starts at `since`.
pub(crate) fn analytics_detail(
    unit: &str,
    previous_since: &str,
    since: &str,
    group: (&str, &str, u64, u64),
) -> serde_json::Value {
    let (key, label, units, base) = group;
    serde_json::json!({
        "unit": unit,
        "previous_since": previous_since,
        "previous_until": since,
        "sample_size": base,
        "previous_sample_size": base,
        "example_limit": 5,
        "groups": [{
            "key": key,
            "label": label,
            "units": units,
            "previous_units": 0,
            "change": units,
            "base_units": base,
            "rate": if base == 0 { serde_json::Value::Null } else {
                let ratio = |value: u64| f64::from(u32::try_from(value).unwrap_or(u32::MAX));
                serde_json::json!(ratio(units) / ratio(base))
            },
            "examples": [{
                "unit_id": "example-unit-1",
                "action_ids": ["11111111-1111-4111-8111-111111111111"],
                "trace_ids": ["0123456789abcdef0123456789abcdef"],
                "first_seen_at": since,
                "last_seen_at": since
            }]
        }],
        "evidence_hint": "Page views are trace spans."
    })
}
