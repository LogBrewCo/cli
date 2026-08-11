//! CLI execution tests for local command flows.

use std::fs;

use futures_util::SinkExt;
use logbrew_cli::{
    CliEnvironment, Command, RuntimeError, WatchOptions, WatchTarget, execute_command,
    parse_command, write_runtime_error,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn authenticated_reads_without_token_explain_login_step() {
    let command = parse_command(["logbrew", "read", "logs", "--release", "api@1", "--json"])
        .expect("command parses");
    let env = test_env("http://127.0.0.1:1", None, "missing-token");
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("missing token fails");

    assert!(matches!(error, RuntimeError::MissingToken));
}

#[tokio::test]
async fn login_no_open_json_prints_auth_url_without_browser_side_effect() {
    let env = test_env("https://example.test", None, "login-no-open");
    for args in [
        &["logbrew", "login", "--no-open", "--json"][..],
        &["logbrew", "--json", "login"][..],
        &[
            "logbrew",
            "login",
            "--provider",
            "gitlab",
            "--no-open",
            "--json",
        ][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("command");
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output)
            .await
            .expect("login succeeds");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        let provider = if args.contains(&"gitlab") {
            "gitlab"
        } else {
            "github"
        };
        assert_eq!(body["ok"], true);
        assert_eq!(
            body["auth_url"],
            format!("https://example.test/api/auth/cli/login?provider={provider}")
        );
        assert_eq!(body["provider"], provider);
        assert_eq!(body["browser_opened"], false);
        assert_eq!(body["next"], "open auth_url in a browser");
    }
}

#[tokio::test]
async fn login_no_open_human_prints_browser_state_and_next_step() {
    let command = parse_command(["logbrew", "login", "--provider", "bitbucket", "--no-open"])
        .expect("command");
    let env = test_env("https://example.test", None, "login-no-open-human");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("login succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "Open this URL to log in: \
         https://example.test/api/auth/cli/login?provider=bitbucket\nProvider: bitbucket\nBrowser: \
         not opened\nNext: open the URL in a browser\n"
    );
}

#[tokio::test]
async fn setup_json_detects_node_project_without_claiming_install() -> TestResult {
    let project_dir = setup_fixture("setup-node")?;
    fs::write(project_dir.join("package.json"), "{}")?;

    for args in [
        &["logbrew", "setup", "--auto", "--yes", "--json"][..],
        &["logbrew", "--json", "setup", "--auto", "--yes"][..],
    ] {
        let output = setup_output(args, project_dir.as_path()).await?;
        let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
        assert_eq!(body["ok"], true);
        assert_eq!(body["auto"], true);
        assert_eq!(body["yes"], true);
        assert_eq!(body["install_ready"], false);
        assert_eq!(body["detected"][0]["runtime"], "node");
        assert_eq!(body["detected"][0]["package_manager"], "npm");
        assert_eq!(body["detected"][0]["manifest"], "package.json");
        assert_eq!(
            body["next"],
            "use the released SDK guidance for this runtime; this CLI version does not yet provide \
             a structured install plan"
        );
    }
    Ok(())
}

#[tokio::test]
async fn setup_json_detects_parent_project_when_run_from_subdirectory() -> TestResult {
    let project_dir = setup_fixture("setup-parent-node")?;
    fs::write(project_dir.join("package.json"), "{}")?;
    let source_dir = project_dir.join("src");
    fs::create_dir_all(source_dir.as_path())?;
    let output = setup_output(&["logbrew", "setup", "--json"], source_dir.as_path()).await?;
    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["detected"][0]["runtime"], "node");
    assert_eq!(body["detected"][0]["package_manager"], "npm");
    assert_eq!(body["detected"][0]["manifest"], "../package.json");
    Ok(())
}

#[tokio::test]
async fn setup_reports_a_non_mutating_symfony_composer_plan() -> TestResult {
    let project_dir = setup_fixture("setup-symfony")?;
    fs::create_dir_all(project_dir.join("config"))?;
    fs::write(project_dir.join("config/bundles.php"), "<?php return [];\n")?;
    fs::write(
        project_dir.join("composer.json"),
        r#"{"require":{"symfony/framework-bundle":"^7.0"}}"#,
    )?;

    let output = setup_output(&["logbrew", "setup", "--json"], &project_dir).await?;
    let body: serde_json::Value = serde_json::from_slice(&output)?;
    assert_eq!(body["install_ready"], true);
    assert_eq!(
        body["install_plan"],
        serde_json::json!({
            "mode": "non_mutating",
            "ecosystem": "composer",
            "package_manager": "composer",
            "integration": "symfony",
            "package": "logbrew/sdk",
            "framework_manifest": "config/bundles.php",
            "compatibility": {
                "status": "review_required",
                "requires_php": "^8.2",
                "requires_framework": "Symfony^6.4 || ^7.0 || ^8.0",
            },
            "install_command": "composer require logbrew/sdk",
            "next_action": {
                "code": "review_compatibility_and_install",
                "target": "project_environment",
            }
        })
    );
    let text = String::from_utf8(setup_output(&["logbrew", "setup"], &project_dir).await?)?;
    for expected in [
        "Integration: Symfony",
        "Package: logbrew/sdk",
        "Command: composer require logbrew/sdk",
    ] {
        assert!(text.contains(expected));
    }
    assert!(!text.contains(project_dir.to_string_lossy().as_ref()));
    Ok(())
}

#[tokio::test]
async fn setup_json_prefers_objective_c_app_over_nested_swift_package() -> TestResult {
    let project_dir = setup_fixture("setup-xcodegen-objective-c")?;
    fs::create_dir_all(project_dir.join("App/Sources"))?;
    fs::create_dir_all(project_dir.join("Packages/Helper"))?;
    fs::write(project_dir.join("App/project.yml"), "name: Checkout\n")?;
    fs::write(project_dir.join("App/Sources/main.m"), "")?;
    fs::write(project_dir.join("Packages/Helper/Package.swift"), "")?;
    let output = setup_output(&["logbrew", "setup", "--json"], project_dir.as_path()).await?;
    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["install_ready"], true);
    assert_eq!(
        body["install_plan"],
        serde_json::json!({
            "mode": "non_mutating",
            "ecosystem": "source",
            "language": "objective-c",
            "package_url": "https://github.com/LogBrewCo/sdk.git",
            "release_tag": "objc/logbrew-objc/v0.2.3",
            "version": "0.2.3",
            "source_subdirectory": "objc/logbrew-objc",
            "header": "include/LogBrew.h",
            "source_directory": "src",
            "frameworks": ["Foundation"],
            "next_action": {
                "code": "vendor_objective_c_sources",
                "target": "application_target"
            }
        })
    );
    assert_eq!(
        body["detected"],
        serde_json::json!([
            {
                "runtime": "objective-c",
                "package_manager": "xcodegen",
                "manifest": "App/project.yml"
            },
            {
                "runtime": "swift",
                "package_manager": "swift package manager",
                "manifest": "Packages/Helper/Package.swift"
            }
        ])
    );
    let text =
        String::from_utf8(setup_output(&["logbrew", "setup"], project_dir.as_path()).await?)?;
    for expected in [
        "Release tag: objc/logbrew-objc/v0.2.3",
        "Source subdirectory: objc/logbrew-objc",
        "Header: include/LogBrew.h",
        "Source directory: src",
        "Framework: Foundation",
        "No files changed.",
    ] {
        assert!(text.contains(expected));
    }
    assert!(!text.contains(project_dir.to_string_lossy().as_ref()));
    Ok(())
}

#[tokio::test]
async fn setup_human_reports_empty_unsupported_and_preference_states() -> TestResult {
    let empty = setup_fixture("setup-empty")?;
    let text = String::from_utf8(setup_output(&["logbrew", "setup"], &empty).await?)?;
    assert_eq!(
        text,
        "LogBrew setup plan\nMode: non-mutating plan\nNo files changed.\nInstall: not ready\nNo \
         supported project manifest found.\nNext: run logbrew setup from a project containing \
         package.json, pyproject.toml, Pipfile, Cargo.toml, Package.swift, project.yml, \
         project.yaml, .xcodeproj, .xcworkspace, CMakeLists.txt, go.mod, or composer.json.\n"
    );

    let rust = setup_fixture("setup-rust-human")?;
    fs::write(rust.join("Cargo.toml"), "")?;
    let text = String::from_utf8(setup_output(&["logbrew", "setup"], &rust).await?)?;
    assert_eq!(
        text,
        "LogBrew setup plan\nMode: non-mutating plan\nNo files changed.\nInstall: not \
         ready\nDetected runtimes:\n- Rust (cargo) at Cargo.toml\nNext: use the released SDK \
         guidance for this runtime; this CLI version does not yet provide a structured install \
         plan\n"
    );

    let text =
        String::from_utf8(setup_output(&["logbrew", "setup", "--auto", "--yes"], &rust).await?)?;
    assert!(text.contains("Preferences: auto=true, yes=true\n"));
    assert!(text.contains("No files changed.\n"));
    assert!(text.contains("Install: not ready\n"));
    Ok(())
}

#[tokio::test]
async fn watch_json_streams_websocket_events_without_leaking_ticket() {
    let messages = vec![
        serde_json::json!({
            "type": "native_log",
            "data": {
                "id": "log_1",
                "level": "warning",
                "severity": "warning",
                "message": "checkout failed"
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "native_action",
            "data": {
                "id": "action_1",
                "name": "checkout_failed"
            }
        })
        .to_string(),
    ];
    let (base_url, server) = spawn_feed_server("ticket value", messages).await;
    let command = parse_command(["logbrew", "watch", "--json"]).expect("command parses");
    assert_eq!(
        command,
        Command::Watch {
            target: WatchTarget::All,
            options: WatchOptions::default(),
            json: true
        }
    );
    let env = test_env(base_url, Some("fixture-token"), "watch-stream");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("watch succeeds");
    server.await.expect("feed server task succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines[0]).expect("valid event")["type"],
        "native_log"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines[1]).expect("valid event")["type"],
        "native_action"
    );
    assert!(!text.contains("ticket value"));
}

#[tokio::test]
async fn watch_json_refreshes_local_auth_before_requesting_a_ticket() -> TestResult {
    let (base_url, server) = spawn_refreshing_feed_server().await;
    let home = setup_fixture("watch-refresh")?;
    let auth_dir = home.join(".logbrew");
    fs::create_dir_all(auth_dir.as_path())?;
    fs::write(
        auth_dir.join("session.json"),
        serde_json::json!({
            "access_token": "watch-expired",
            "refresh_token": "watch-refresh",
            "origin": base_url.as_str(),
        })
        .to_string(),
    )?;
    let command = parse_command(["logbrew", "watch", "logs", "--json"])?;
    let env = CliEnvironment {
        base_url,
        token: None,
        home: Some(home),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;
    server.await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("log_after_refresh"));
    for secret in [
        "watch-expired",
        "watch-refresh",
        "watch-fresh",
        "watch-next-refresh",
        "watch-ticket",
    ] {
        assert!(!text.contains(secret));
    }
    Ok(())
}

#[tokio::test]
async fn watch_json_filters_error_and_critical_events_client_side() {
    let messages = vec![
        serde_json::json!({
            "type": "native_log",
            "data": {
                "id": "log_warn",
                "level": "warning",
                "severity": "warning",
                "message": "slow checkout"
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "native_log",
            "data": {
                "id": "log_error",
                "level": "error",
                "severity": "error",
                "message": "checkout failed"
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "native_issue",
            "data": {
                "id": "issue_critical",
                "severity": "critical",
                "title": "payment outage"
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "native_action",
            "data": {
                "id": "action_1",
                "name": "checkout_failed"
            }
        })
        .to_string(),
    ];
    let (base_url, server) = spawn_feed_server("ticket/with spaces", messages).await;
    let command = parse_command(["logbrew", "watch", "--severity", "error,critical", "--json"])
        .expect("command parses");
    assert_eq!(
        command,
        Command::Watch {
            target: WatchTarget::All,
            options: WatchOptions {
                severity: vec!["error".to_owned(), "critical".to_owned()]
            },
            json: true
        }
    );
    let env = test_env(base_url, Some("fixture-token"), "watch-filter");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("watch succeeds");
    server.await.expect("feed server task succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("log_error"));
    assert!(lines[1].contains("issue_critical"));
    assert!(!text.contains("log_warn"));
    assert!(!text.contains("action_1"));
    assert!(!text.contains("ticket/with spaces"));
}

#[tokio::test]
async fn watch_json_reconnects_with_fresh_ticket_after_transient_disconnect() {
    let sessions = vec![
        FeedSession {
            ticket: "first ticket",
            messages: vec![
                serde_json::json!({
                    "type": "native_log",
                    "data": {
                        "id": "log_before_disconnect",
                        "level": "error",
                        "severity": "error",
                        "message": "first connection"
                    }
                })
                .to_string(),
            ],
            close: FeedClose::Drop,
        },
        FeedSession {
            ticket: "second ticket",
            messages: vec![
                serde_json::json!({
                    "type": "native_log",
                    "data": {
                        "id": "log_after_reconnect",
                        "level": "critical",
                        "severity": "critical",
                        "message": "second connection"
                    }
                })
                .to_string(),
            ],
            close: FeedClose::Clean,
        },
    ];
    let (base_url, server) = spawn_feed_server_sessions(sessions).await;
    let command = parse_command(["logbrew", "watch", "logs", "--json"]).expect("command parses");
    let env = test_env(base_url, Some("fixture-token"), "watch-reconnect");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("watch reconnect succeeds");
    server.await.expect("feed server task succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("log_before_disconnect"));
    assert!(lines[1].contains("log_after_reconnect"));
    assert!(!text.contains("first ticket"));
    assert!(!text.contains("second ticket"));
}

#[tokio::test]
async fn watch_human_requires_json_for_live_stream() {
    let command = parse_command(["logbrew", "follow", "events"]).expect("command parses");
    assert_eq!(
        command,
        Command::Watch {
            target: WatchTarget::All,
            options: WatchOptions::default(),
            json: false
        }
    );
    let env = test_env("https://example.test", None, "watch-human");
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("human watch requires json");
    write_runtime_error(&error, command.wants_json(), &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "watch streams JSON for agents\nNext: run logbrew watch --json\n"
    );
}

async fn spawn_feed_server(
    ticket: &str,
    messages: Vec<String>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind feed server");
    let address = listener.local_addr().expect("local feed server address");
    let expected_ticket_query = format!("ticket={}", percent_encode(ticket));
    let ticket = ticket.to_owned();
    let server = tokio::spawn(async move {
        let (mut ticket_stream, _) = listener.accept().await.expect("ticket connection");
        let request = read_http_request(&mut ticket_stream).await;
        let lower_request = request.to_ascii_lowercase();
        assert!(request.starts_with("POST /api/feed/ticket "));
        assert!(lower_request.contains("authorization: bearer fixture-token"));
        write_json_response(&mut ticket_stream, serde_json::json!({ "ticket": ticket })).await;

        let (live_stream, _) = listener.accept().await.expect("websocket connection");
        let mut websocket = accept_feed(live_stream, &expected_ticket_query).await;
        for message in messages {
            websocket
                .send(Message::Text(message.into()))
                .await
                .expect("send websocket message");
        }
        websocket.close(None).await.expect("close websocket");
    });
    (format!("http://{address}"), server)
}

async fn spawn_refreshing_feed_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind refreshing feed server");
    let address = listener
        .local_addr()
        .expect("refreshing feed server address");
    let server = tokio::spawn(async move {
        let (mut expired_stream, _) = listener.accept().await.expect("expired ticket request");
        let expired_request = read_http_request(&mut expired_stream).await;
        assert!(expired_request.starts_with("POST /api/feed/ticket "));
        assert!(
            expired_request
                .to_ascii_lowercase()
                .contains("authorization: bearer watch-expired")
        );
        expired_stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            )
            .await
            .expect("write expired response");

        let (mut refresh_stream, _) = listener.accept().await.expect("refresh request");
        let refresh_request = read_http_request(&mut refresh_stream).await;
        assert!(refresh_request.starts_with("POST /api/auth/refresh "));
        assert!(refresh_request.contains(r#"{"refresh_token":"watch-refresh"}"#));
        write_json_response(
            &mut refresh_stream,
            serde_json::json!({
                "access_token": "watch-fresh",
                "refresh_token": "watch-next-refresh"
            }),
        )
        .await;

        let (mut ticket_stream, _) = listener.accept().await.expect("fresh ticket request");
        let ticket_request = read_http_request(&mut ticket_stream).await;
        assert!(ticket_request.starts_with("POST /api/feed/ticket "));
        assert!(
            ticket_request
                .to_ascii_lowercase()
                .contains("authorization: bearer watch-fresh")
        );
        write_json_response(
            &mut ticket_stream,
            serde_json::json!({ "ticket": "watch-ticket" }),
        )
        .await;

        let (live_stream, _) = listener.accept().await.expect("websocket request");
        let mut websocket = accept_feed(live_stream, "ticket=watch-ticket").await;
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "native_log",
                    "data": { "id": "log_after_refresh", "severity": "error" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send refreshed event");
        websocket
            .close(None)
            .await
            .expect("close refreshed websocket");
    });
    (format!("http://{address}"), server)
}

struct FeedSession {
    ticket: &'static str,
    messages: Vec<String>,
    close: FeedClose,
}

#[derive(Clone, Copy)]
enum FeedClose {
    Clean,
    Drop,
}

async fn spawn_feed_server_sessions(
    sessions: Vec<FeedSession>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind feed server");
    let address = listener.local_addr().expect("local feed server address");
    let server = tokio::spawn(async move {
        for session in sessions {
            let (mut ticket_stream, _) = listener.accept().await.expect("ticket connection");
            let request = read_http_request(&mut ticket_stream).await;
            let lower_request = request.to_ascii_lowercase();
            assert!(request.starts_with("POST /api/feed/ticket "));
            assert!(lower_request.contains("authorization: bearer fixture-token"));
            write_json_response(
                &mut ticket_stream,
                serde_json::json!({ "ticket": session.ticket }),
            )
            .await;

            let expected_ticket_query = format!("ticket={}", percent_encode(session.ticket));
            let (live_stream, _) = listener.accept().await.expect("websocket connection");
            let mut websocket = accept_feed(live_stream, &expected_ticket_query).await;
            for message in session.messages {
                websocket
                    .send(Message::Text(message.into()))
                    .await
                    .expect("send websocket message");
            }
            match session.close {
                FeedClose::Clean => websocket.close(None).await.expect("close websocket"),
                FeedClose::Drop => drop(websocket),
            }
        }
    });
    (format!("http://{address}"), server)
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).await.expect("read request");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
    }
    String::from_utf8(request).expect("request is utf8")
}

async fn accept_feed(
    stream: tokio::net::TcpStream,
    expected_query: &str,
) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
    let callback =
        |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
         response: tokio_tungstenite::tungstenite::handshake::server::Response| {
            assert_eq!(request.uri().path(), "/api/feed/live");
            assert_eq!(request.uri().query(), Some(expected_query));
            Ok(response)
        };
    accept_hdr_async(stream, callback)
        .await
        .expect("accept websocket")
}

async fn write_json_response(stream: &mut tokio::net::TcpStream, body: serde_json::Value) {
    let body = body.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .expect("write JSON response");
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + (nibble - 10)),
        _ => '?',
    }
}

fn setup_fixture(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let dir = std::env::temp_dir().join(format!("logbrew-cli-{name}-{}", std::process::id()));
    if dir.try_exists()? {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn test_env(base_url: impl Into<String>, token: Option<&str>, name: &str) -> CliEnvironment {
    CliEnvironment {
        base_url: base_url.into(),
        token: token.map(str::to_owned),
        home: Some(std::env::temp_dir().join(format!("logbrew-{name}-test"))),
        cwd: None,
    }
}

async fn setup_output(
    args: &[&str],
    cwd: &std::path::Path,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let command = parse_command(args.iter().copied())?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(cwd.with_extension("home")),
        cwd: Some(cwd.to_path_buf()),
    };
    let mut output = Vec::new();
    execute_command(&command, &env, &mut output).await?;
    Ok(output)
}
