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

#[tokio::test]
async fn authenticated_reads_without_token_explain_login_step() {
    let command = parse_command(["logbrew", "read", "logs", "--release", "api@1", "--json"])
        .expect("command parses");
    let env = CliEnvironment {
        base_url: "http://127.0.0.1:1".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-missing-token-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("missing token fails");

    assert!(matches!(error, RuntimeError::MissingToken));
}

#[tokio::test]
async fn login_no_open_json_prints_auth_url_without_browser_side_effect() {
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-login-no-open-test")),
        cwd: None,
    };
    for args in [
        &["logbrew", "login", "--no-open", "--json"][..],
        &["logbrew", "--json", "login"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("command");
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output)
            .await
            .expect("login succeeds");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], true);
        assert_eq!(body["auth_url"], "https://example.test/api/auth/cli/login");
        assert_eq!(body["browser_opened"], false);
        assert_eq!(body["next"], "open auth_url in a browser");
    }
}

#[tokio::test]
async fn login_no_open_human_prints_browser_state_and_next_step() {
    let command = parse_command(["logbrew", "login", "--no-open"]).expect("command");
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-login-no-open-human-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("login succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "Open this URL to log in: https://example.test/api/auth/cli/login\nBrowser: not \
         opened\nNext: open the URL in a browser\n"
    );
}

#[tokio::test]
async fn setup_json_detects_node_project_without_claiming_install()
-> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-node")?;
    fs::write(project_dir.join("package.json"), "{}")?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-node-home")),
        cwd: Some(project_dir),
    };

    for args in [
        &["logbrew", "setup", "--auto", "--yes", "--json"][..],
        &["logbrew", "--json", "setup", "--auto", "--yes"][..],
    ] {
        let command = parse_command(args.iter().copied())?;
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output).await?;

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
            "install the matching LogBrew SDK package when packages are ready; send release and \
             environment with logs, issues, actions, and traces"
        );
    }
    Ok(())
}

#[tokio::test]
async fn setup_json_detects_parent_project_when_run_from_subdirectory()
-> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-parent-node")?;
    fs::write(project_dir.join("package.json"), "{}")?;
    let source_dir = project_dir.join("src");
    fs::create_dir_all(source_dir.as_path())?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-parent-node-home")),
        cwd: Some(source_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["detected"][0]["runtime"], "node");
    assert_eq!(body["detected"][0]["package_manager"], "npm");
    assert_eq!(body["detected"][0]["manifest"], "../package.json");
    Ok(())
}

#[tokio::test]
async fn setup_json_detects_xcodegen_ios_project() -> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-xcodegen-ios")?;
    fs::write(project_dir.join("project.yml"), "name: Checkout\n")?;
    let command = parse_command(["logbrew", "setup", "--json"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-xcodegen-ios-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["install_ready"], false);
    assert_eq!(body["detected"][0]["runtime"], "swift-ios");
    assert_eq!(body["detected"][0]["package_manager"], "xcodegen");
    assert_eq!(body["detected"][0]["manifest"], "project.yml");
    Ok(())
}

#[tokio::test]
async fn setup_human_explains_empty_project_next_step() -> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-empty")?;
    let command = parse_command(["logbrew", "setup"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-empty-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "LogBrew setup plan\nMode: non-mutating plan\nNo files changed.\nInstall: not ready\nNo \
         supported project manifest found.\nNext: run logbrew setup from a project containing \
         package.json, pyproject.toml, Pipfile, Cargo.toml, Package.swift, project.yml, \
         project.yaml, .xcodeproj, .xcworkspace, go.mod, or composer.json.\n"
    );
    Ok(())
}

#[tokio::test]
async fn setup_human_detects_project_without_claiming_install()
-> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-rust-human")?;
    fs::write(project_dir.join("Cargo.toml"), "")?;
    let command = parse_command(["logbrew", "setup"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-rust-human-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "LogBrew setup plan\nMode: non-mutating plan\nNo files changed.\nInstall: not \
         ready\nDetected runtimes:\n- Rust (cargo) at Cargo.toml\nNext: install the matching \
         LogBrew SDK package when packages are ready; send release and environment with logs, \
         issues, actions, and traces\n"
    );
    Ok(())
}

#[tokio::test]
async fn setup_human_echoes_non_mutating_preferences() -> Result<(), Box<dyn std::error::Error>> {
    let project_dir = setup_fixture("setup-rust-human-prefs")?;
    fs::write(project_dir.join("Cargo.toml"), "")?;
    let command = parse_command(["logbrew", "setup", "--auto", "--yes"])?;
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-setup-rust-human-prefs-home")),
        cwd: Some(project_dir),
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Mode: non-mutating plan\n"));
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
    let env = CliEnvironment {
        base_url,
        token: Some("fixture-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-watch-stream-test")),
        cwd: None,
    };
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
    let env = CliEnvironment {
        base_url,
        token: Some("fixture-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-watch-filter-test")),
        cwd: None,
    };
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
    let env = CliEnvironment {
        base_url,
        token: Some("fixture-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-watch-reconnect-test")),
        cwd: None,
    };
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
    let env = CliEnvironment {
        base_url: "https://example.test".to_owned(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-watch-human-test")),
        cwd: None,
    };
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
        let body = serde_json::json!({ "ticket": ticket }).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        ticket_stream
            .write_all(response.as_bytes())
            .await
            .expect("write ticket response");

        let (live_stream, _) = listener.accept().await.expect("websocket connection");
        let callback =
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
             response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(request.uri().path(), "/api/feed/live");
                assert_eq!(request.uri().query(), Some(expected_ticket_query.as_str()));
                Ok(response)
            };
        let mut websocket = accept_hdr_async(live_stream, callback)
            .await
            .expect("accept websocket");
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
            let body = serde_json::json!({ "ticket": session.ticket }).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            ticket_stream
                .write_all(response.as_bytes())
                .await
                .expect("write ticket response");

            let expected_ticket_query = format!("ticket={}", percent_encode(session.ticket));
            let (live_stream, _) = listener.accept().await.expect("websocket connection");
            let callback =
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                 response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                    assert_eq!(request.uri().path(), "/api/feed/live");
                    assert_eq!(request.uri().query(), Some(expected_ticket_query.as_str()));
                    Ok(response)
                };
            let mut websocket = accept_hdr_async(live_stream, callback)
                .await
                .expect("accept websocket");
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
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(request).expect("request is utf8")
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
    remove_dir_if_exists(dir.as_path())?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn remove_dir_if_exists(path: &std::path::Path) -> Result<(), std::io::Error> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
