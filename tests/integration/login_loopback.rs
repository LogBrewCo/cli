//! Built-binary browser login proof.

#![cfg(unix)]

use crate::matchers::body_json;
use crate::{Mock, MockServer, ResponseTemplate};
use std::os::unix::fs::PermissionsExt as _;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn built_binary_completes_loopback_login_without_exposing_credentials() -> TestResult {
    drop(rustls::crypto::ring::default_provider().install_default());
    let server = MockServer::start().await;
    Mock::route("POST", "/api/auth/gitlab")
        .and(body_json(
            serde_json::json!({ "code": "binary-provider-code" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "binary-access",
            "refresh_token": "binary-refresh"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (result, home, state) =
        run_binary_login(&server, "binary-provider-code", "success").await?;
    assert!(result.status.success());
    assert_output_safe(
        &result,
        &[
            "binary-provider-code",
            "binary-access",
            "binary-refresh",
            state.as_str(),
        ],
    );
    let stdout = String::from_utf8(result.stdout)?;
    let stderr = String::from_utf8(result.stderr)?;
    assert!(stdout.contains("Logged in to LogBrew."));
    assert!(stderr.is_empty());
    let saved: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(home.join(".logbrew/session.json"))?.as_str(),
    )?;
    assert_eq!(saved["access_token"], "binary-access");
    assert_eq!(saved["refresh_token"], "binary-refresh");
    assert_eq!(saved["origin"], server.uri());
    assert_eq!(file_mode(home.join(".logbrew").as_path())?, 0o700);
    for name in ["session.json", "credentials.lock"] {
        assert_eq!(
            file_mode(home.join(".logbrew").join(name).as_path())?,
            0o600
        );
    }
    Ok(())
}

#[tokio::test]
async fn browser_login_does_not_forward_exchange_codes_across_redirects() -> TestResult {
    drop(rustls::crypto::ring::default_provider().install_default());
    let server = MockServer::start().await;
    let redirected = MockServer::start().await;
    Mock::route("POST", "/api/auth/gitlab")
        .and(body_json(
            serde_json::json!({ "code": "redirect-provider-code" }),
        ))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/capture", redirected.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::route("POST", "/capture")
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "redirect-access",
            "refresh_token": "redirect-refresh"
        })))
        .mount(&redirected)
        .await;

    let (result, home, _) = run_binary_login(&server, "redirect-provider-code", "redirect").await?;

    assert!(!result.status.success());
    assert!(redirected.received_requests().await.is_empty());
    assert!(!home.join(".logbrew/session.json").exists());
    assert_output_safe(
        &result,
        &[
            "redirect-provider-code",
            "redirect-access",
            "redirect-refresh",
        ],
    );
    Ok(())
}

async fn run_binary_login(
    server: &MockServer,
    code: &str,
    label: &str,
) -> TestResult<(std::process::Output, std::path::PathBuf, String)> {
    let root = binary_login_root(label)?;
    let home = root.join("home");
    let bin_dir = root.join("bin");
    let call_file = root.join("browser-url");
    std::fs::create_dir_all(home.as_path())?;
    std::fs::create_dir_all(bin_dir.as_path())?;
    let opener_name = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let opener = bin_dir.join(opener_name);
    std::fs::write(
        opener.as_path(),
        "#!/bin/sh\nprintf '%s' \"$1\" > \"$LOGBREW_TEST_BROWSER_URL_FILE\"\n",
    )?;
    std::fs::set_permissions(opener.as_path(), std::fs::Permissions::from_mode(0o700))?;
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![bin_dir.clone()];
    path_entries.extend(std::env::split_paths(&inherited_path));
    let child_path = std::env::join_paths(path_entries)?;
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_logbrew"))
        .arg("login")
        .args(["--provider", "gitlab"])
        .env("HOME", home.as_os_str())
        .env("LOGBREW_API_URL", server.uri())
        .env("LOGBREW_TEST_BROWSER_URL_FILE", call_file.as_os_str())
        .env("PATH", child_path)
        .env_remove("LOGBREW_TOKEN")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let auth_url = wait_for_browser_url(call_file.as_path()).await?;
    let auth_url = reqwest::Url::parse(auth_url.as_str())?;
    assert_eq!(auth_url.path(), "/api/auth/cli/login");
    let query = auth_url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        query.get("provider").map(|value| value.as_ref()),
        Some("gitlab")
    );
    let state = query.get("state").ok_or("missing state")?.to_string();
    let mut callback = reqwest::Url::parse(
        query
            .get("redirect_uri")
            .ok_or("missing redirect URI")?
            .as_ref(),
    )?;
    let _query = callback
        .query_pairs_mut()
        .append_pair("provider", "gitlab")
        .append_pair("code", code)
        .append_pair("state", state.as_str());
    let callback_response = reqwest::get(callback).await?;
    assert_eq!(callback_response.status(), reqwest::StatusCode::OK);

    let result = tokio::task::spawn_blocking(move || child.wait_with_output()).await??;
    Ok((result, home, state))
}

fn assert_output_safe(result: &std::process::Output, secrets: &[&str]) {
    for output in [&result.stdout, &result.stderr] {
        let output = String::from_utf8_lossy(output);
        assert!(secrets.iter().all(|secret| !output.contains(secret)));
    }
}

fn file_mode(path: &std::path::Path) -> Result<u32, std::io::Error> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
}

/// Waits for the fake browser launcher to record its URL.
async fn wait_for_browser_url(path: &std::path::Path) -> Result<String, std::io::Error> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match std::fs::read_to_string(path) {
            Ok(value) if !value.trim().is_empty() => return Ok(value),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "browser launcher did not receive login URL",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// Creates one isolated root for the executable login flow.
fn binary_login_root(label: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let root = std::env::temp_dir().join(format!(
        "logbrew-cli-binary-login-{label}-{}",
        std::process::id()
    ));
    match std::fs::remove_dir_all(root.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(root.as_path())?;
    Ok(root)
}
