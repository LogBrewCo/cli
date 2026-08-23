//! Shared process, environment, and credential helpers for integration targets.

use super::MockServer;

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

pub(crate) fn assert_cli_success(process: &std::process::Output) {
    assert!(
        process.status.success(),
        "built binary failed: {}",
        String::from_utf8_lossy(process.stderr.as_slice())
    );
    assert!(process.stderr.is_empty());
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
