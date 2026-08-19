//! Server-aware CLI logout contract tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn logout_revokes_refresh_family_without_bearer_then_clears_local_pair()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .and(body_json(serde_json::json!({
            "refresh_token": "logout-refresh-proof"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "revoked": true,
            "next_action": {
                "code": "clear_local_session",
                "target": "local_credentials"
            }
        })))
        .mount(&server)
        .await;
    let home = logout_home("revoke-success")?;
    let session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "logout-access-proof",
        "logout-refresh-proof",
    )?;
    let env = logout_env(&server, home);
    let command = parse_command(["logbrew", "logout", "--json"])?;
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].headers.contains_key("authorization"));
    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["removed"], true);
    assert_eq!(body["server_session"], "revoked");
    assert!(!session_path.exists());
    for secret in ["logout-access-proof", "logout-refresh-proof"] {
        assert!(!String::from_utf8_lossy(output.as_slice()).contains(secret));
    }
    Ok(())
}

#[tokio::test]
async fn logout_treats_rejected_refresh_as_inactive_and_clears_local_pair()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .and(body_json(serde_json::json!({
            "refresh_token": "inactive-refresh-proof"
        })))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "code": "unauthorized",
            "error": "unsafe inactive-refresh-proof",
            "next": "unsafe server guidance"
        })))
        .mount(&server)
        .await;
    let home = logout_home("inactive")?;
    let session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "inactive-access-proof",
        "inactive-refresh-proof",
    )?;
    let body = run_logout_json(&server, home, None).await?;

    assert_eq!(body["removed"], true);
    assert_eq!(body["server_session"], "inactive");
    assert_eq!(body["next"], "run logbrew login to authenticate again");
    assert!(!session_path.exists());
    let rendered = body.to_string();
    for hidden in [
        "inactive-access-proof",
        "inactive-refresh-proof",
        "unsafe server guidance",
    ] {
        assert!(!rendered.contains(hidden));
    }
    Ok(())
}

#[tokio::test]
async fn logout_clears_local_pair_when_server_revocation_is_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, response) in [
        (
            "hostile-503",
            ResponseTemplate::new(503).set_body_string(
                "unsafe unknown-refresh-proof authorization: Bearer private-value",
            ),
        ),
        (
            "malformed-200",
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "revoked": true,
                "next_action": {
                    "code": "clear_local_session",
                    "target": "local_credentials"
                },
                "refresh_token": "unsafe-response-token"
            })),
        ),
        (
            "malformed-401",
            ResponseTemplate::new(401)
                .set_body_string("proxy denied unknown-refresh-proof without LogBrew code"),
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth/logout"))
            .respond_with(response)
            .mount(&server)
            .await;
        let home = logout_home(name)?;
        let session_path = write_session(
            home.as_path(),
            server.uri().as_str(),
            "unknown-access-proof",
            "unknown-refresh-proof",
        )?;

        let body = run_logout_json(&server, home, None).await?;

        assert_eq!(body["removed"], true);
        assert_eq!(body["server_session"], "unknown");
        assert_eq!(
            body["next"],
            "run logbrew login; then use logbrew support create --help if revocation must be confirmed"
        );
        assert!(!session_path.exists());
        let rendered = body.to_string();
        for hidden in [
            "unknown-access-proof",
            "unknown-refresh-proof",
            "unsafe-response-token",
            "authorization",
            "private-value",
        ] {
            assert!(!rendered.contains(hidden));
        }
    }
    Ok(())
}

#[tokio::test]
async fn logout_revokes_stored_session_without_sending_env_or_legacy_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .and(body_json(serde_json::json!({
            "refresh_token": "env-file-refresh-proof"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "revoked": true,
            "next_action": {
                "code": "clear_local_session",
                "target": "local_credentials"
            }
        })))
        .mount(&server)
        .await;

    let env_home = logout_home("env-token")?;
    let env_session = write_session(
        env_home.as_path(),
        server.uri().as_str(),
        "env-file-access-proof",
        "env-file-refresh-proof",
    )?;
    let env_body = run_logout_json(&server, env_home, Some("active-env-proof")).await?;
    assert_eq!(env_body["removed"], true);
    assert_eq!(env_body["env_token_active"], true);
    assert_eq!(env_body["server_session"], "revoked");
    assert!(!env_session.exists());

    let legacy_home = logout_home("legacy-token")?;
    let legacy_path = legacy_home.join(".logbrew").join("token");
    std::fs::create_dir_all(legacy_path.parent().expect("legacy path has parent"))?;
    std::fs::write(legacy_path.as_path(), "legacy-access-proof\n")?;
    let legacy_body = run_logout_json(&server, legacy_home, None).await?;
    assert_eq!(legacy_body["removed"], true);
    assert_eq!(legacy_body["server_session"], "not_applicable");
    assert!(!legacy_path.exists());

    let missing_home = logout_home("missing-token")?;
    let missing_body = run_logout_json(&server, missing_home, None).await?;
    assert_eq!(missing_body["removed"], false);
    assert_eq!(missing_body["server_session"], "not_applicable");

    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].headers.contains_key("authorization"));
    let rendered = format!("{env_body}{legacy_body}{missing_body}");
    for hidden in [
        "active-env-proof",
        "env-file-access-proof",
        "env-file-refresh-proof",
        "legacy-access-proof",
    ] {
        assert!(!rendered.contains(hidden));
    }
    Ok(())
}

#[tokio::test]
async fn logout_keeps_legacy_credentials_local_when_api_url_is_invalid()
-> Result<(), Box<dyn std::error::Error>> {
    let home = logout_home("legacy-invalid-api")?;
    let legacy_path = home.join(".logbrew").join("token");
    std::fs::create_dir_all(legacy_path.parent().expect("legacy path has parent"))?;
    std::fs::write(legacy_path.as_path(), "legacy-invalid-api-proof\n")?;
    let command = parse_command(["logbrew", "logout", "--json"])?;
    let env = CliEnvironment {
        base_url: "not a valid API URL".to_owned(),
        token: None,
        home: Some(home),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["removed"], true);
    assert_eq!(body["server_session"], "not_applicable");
    assert!(!legacy_path.exists());
    assert!(!body.to_string().contains("legacy-invalid-api-proof"));
    Ok(())
}

#[tokio::test]
async fn logout_human_output_reports_server_and_environment_state_without_secrets()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "revoked": true,
            "next_action": {
                "code": "clear_local_session",
                "target": "local_credentials"
            }
        })))
        .mount(&server)
        .await;
    let home = logout_home("human-env")?;
    let _session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "human-access-proof",
        "human-refresh-proof",
    )?;
    let command = parse_command(["logbrew", "logout"])?;
    let mut env = logout_env(&server, home);
    env.token = Some("human-env-proof".to_owned());
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let rendered = String::from_utf8(output)?;
    assert!(rendered.contains("Local LogBrew token removed."));
    assert!(rendered.contains("Server session: revoked"));
    assert!(rendered.contains("Auth: env token still active"));
    assert!(rendered.contains("Next: unset LOGBREW_TOKEN to fully log out"));
    for secret in [
        "human-access-proof",
        "human-refresh-proof",
        "human-env-proof",
    ] {
        assert!(!rendered.contains(secret));
    }
    Ok(())
}

#[tokio::test]
async fn logout_unknown_server_with_env_token_preserves_both_recovery_steps()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .respond_with(ResponseTemplate::new(503).set_body_string("unsafe combined-proof"))
        .mount(&server)
        .await;
    let home = logout_home("unknown-env")?;
    let session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "combined-access-proof",
        "combined-refresh-proof",
    )?;

    let body = run_logout_json(&server, home, Some("combined-env-proof")).await?;

    assert_eq!(body["removed"], true);
    assert_eq!(body["env_token_active"], true);
    assert_eq!(body["server_session"], "unknown");
    assert_eq!(
        body["next"],
        "unset LOGBREW_TOKEN, run logbrew login, then use logbrew support create --help if revocation must be confirmed"
    );
    assert!(!session_path.exists());
    let rendered = body.to_string();
    for secret in [
        "combined-access-proof",
        "combined-refresh-proof",
        "combined-env-proof",
        "unsafe combined-proof",
    ] {
        assert!(!rendered.contains(secret));
    }
    Ok(())
}

#[tokio::test]
async fn logout_does_not_forward_refresh_token_across_redirects()
-> Result<(), Box<dyn std::error::Error>> {
    let redirect_target = MockServer::start().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/capture", redirect_target.uri())),
        )
        .mount(&server)
        .await;
    let home = logout_home("redirect")?;
    let session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "redirect-access-proof",
        "redirect-refresh-proof",
    )?;

    let body = run_logout_json(&server, home, None).await?;

    assert_eq!(body["removed"], true);
    assert_eq!(body["server_session"], "unknown");
    assert!(!session_path.exists());
    assert!(
        redirect_target
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
    assert!(!body.to_string().contains("redirect-refresh-proof"));
    Ok(())
}

#[tokio::test]
async fn repeated_logout_does_not_replay_deleted_refresh_credentials()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "revoked": true,
            "next_action": {
                "code": "clear_local_session",
                "target": "local_credentials"
            }
        })))
        .mount(&server)
        .await;
    let home = logout_home("repeat")?;
    let _session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "repeat-access-proof",
        "repeat-refresh-proof",
    )?;

    let first = run_logout_json(&server, home.clone(), None).await?;
    let second = run_logout_json(&server, home, None).await?;

    assert_eq!(first["server_session"], "revoked");
    assert_eq!(second["removed"], false);
    assert_eq!(second["server_session"], "not_applicable");
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_new_login_survives_logout_revocation() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(200))
                .set_body_json(serde_json::json!({
                    "revoked": true,
                    "next_action": {
                        "code": "clear_local_session",
                        "target": "local_credentials"
                    }
                })),
        )
        .mount(&server)
        .await;
    let home = logout_home("concurrent-login")?;
    let session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "old-access-proof",
        "old-refresh-proof",
    )?;
    let env = logout_env(&server, home.clone());
    let logout_task = tokio::spawn(async move {
        let command = parse_command(["logbrew", "logout", "--json"])?;
        let mut output = Vec::new();
        execute_command(&command, &env, &mut output).await?;
        Ok::<Vec<u8>, logbrew_cli::RuntimeError>(output)
    });

    for _attempt in 0..50 {
        if !server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(
        !server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
    let writer_home = home;
    let writer = tokio::task::spawn_blocking(move || write_new_session_after_lock(writer_home));

    let output = logout_task.await??;
    writer.await??;

    let saved: serde_json::Value =
        serde_json::from_str(std::fs::read_to_string(session_path)?.as_str())?;
    assert_eq!(saved["access_token"], "new-access-proof");
    assert_eq!(saved["refresh_token"], "new-refresh-proof");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(output.as_slice())?["server_session"],
        "revoked"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn waiting_for_credential_lock_does_not_block_async_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    use fs2::FileExt as _;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "revoked": true,
            "next_action": {
                "code": "clear_local_session",
                "target": "local_credentials"
            }
        })))
        .mount(&server)
        .await;
    let home = logout_home("contended-runtime")?;
    let _session_path = write_session(
        home.as_path(),
        server.uri().as_str(),
        "contended-access-proof",
        "contended-refresh-proof",
    )?;
    let lock_path = home.join(".logbrew").join("credentials.lock");
    let held_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)?;
    held_lock.lock_exclusive()?;
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(held_lock);
    });
    let env = logout_env(&server, home);
    let logout_task = tokio::spawn(async move {
        let command = parse_command(["logbrew", "logout", "--json"])?;
        let mut output = Vec::new();
        execute_command(&command, &env, &mut output).await?;
        Ok::<Vec<u8>, logbrew_cli::RuntimeError>(output)
    });

    let started = std::time::Instant::now();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let scheduler_delay = started.elapsed();
    let output = logout_task.await??;
    releaser.join().expect("credential-lock releaser completes");

    assert!(
        scheduler_delay < std::time::Duration::from_millis(100),
        "credential lock stalled the async runtime for {scheduler_delay:?}"
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(output.as_slice())?["server_session"],
        "revoked"
    );
    Ok(())
}

fn logout_env(server: &MockServer, home: std::path::PathBuf) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(home),
        cwd: None,
    }
}

async fn run_logout_json(
    server: &MockServer,
    home: std::path::PathBuf,
    env_token: Option<&str>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let command = parse_command(["logbrew", "logout", "--json"])?;
    let mut env = logout_env(server, home);
    env.token = env_token.map(ToOwned::to_owned);
    let mut output = Vec::new();
    execute_command(&command, &env, &mut output).await?;
    Ok(serde_json::from_slice(output.as_slice())?)
}

fn logout_home(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let path = std::env::temp_dir().join(format!(
        "logbrew-cli-logout-session-{name}-{}",
        std::process::id()
    ));
    match std::fs::remove_dir_all(path.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(path.as_path())?;
    Ok(path)
}

fn write_session(
    home: &std::path::Path,
    origin: &str,
    access_token: &str,
    refresh_token: &str,
) -> Result<std::path::PathBuf, std::io::Error> {
    let auth_dir = home.join(".logbrew");
    std::fs::create_dir_all(auth_dir.as_path())?;
    let session_path = auth_dir.join("session.json");
    std::fs::write(
        session_path.as_path(),
        serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "origin": origin,
        })
        .to_string(),
    )?;
    Ok(session_path)
}

fn write_new_session_after_lock(home: std::path::PathBuf) -> Result<(), std::io::Error> {
    use fs2::FileExt as _;

    let auth_dir = home.join(".logbrew");
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(auth_dir.join("credentials.lock"))?;
    lock_file.lock_exclusive()?;
    std::fs::write(
        auth_dir.join("session.json"),
        serde_json::json!({
            "access_token": "new-access-proof",
            "refresh_token": "new-refresh-proof",
            "origin": "https://new-session.example"
        })
        .to_string(),
    )
}
