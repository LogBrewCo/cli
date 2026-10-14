//! CLI execution tests for local command flows.

use crate::{TestResult, execute_command};
use futures_util::SinkExt as _;
use logbrew_cli::{
    CliEnvironment, Command, RuntimeError, WatchOptions, WatchTarget, parse_command,
    write_runtime_error,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::{Message, handshake::server};

async_test!(authenticated_reads_without_token_explain_login_step -> TestResult, {
    let command = parse_command(["logbrew", "read", "logs", "--release", "api@1", "--json"])?;
    let env = test_env("http://127.0.0.1:1", None, "missing-token");
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .err()
        .ok_or("missing token must fail")?;

    assert!(matches!(error, RuntimeError::MissingToken));
    Ok(())
});

async_test!(
    login_no_open_json_prints_auth_url_without_browser_side_effect -> TestResult,
    {
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
            let command = parse_command(args.iter().copied())?;
            let mut output = Vec::new();

            execute_command(&command, &env, &mut output)
                .await?;

            let body: serde_json::Value =
                serde_json::from_slice(output.as_slice())?;
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
        Ok(())
    }
);

async_test!(login_no_open_human_prints_browser_state_and_next_step -> TestResult, {
    let command = parse_command(["logbrew", "login", "--provider", "bitbucket", "--no-open"])?;
    let env = test_env("https://example.test", None, "login-no-open-human");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "Open this URL to log in: \
             https://example.test/api/auth/cli/login?provider=bitbucket\nProvider: bitbucket\nBrowser: \
             not opened\nNext: open the URL in a browser\n"
    );
    Ok(())
});

async_test!(
    watch_json_streams_websocket_events_without_leaking_ticket -> TestResult,
    {
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
        let (base_url, server) =
            spawn_feed_server("ticket value", "ticket=ticket%20value", messages).await?;
        let command = parse_command(["logbrew", "watch", "--json"])?;
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
            .await?;
        server.await??;

        let text = String::from_utf8(output)?;
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[0])?["type"],
            "native_log"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[1])?["type"],
            "native_action"
        );
        assert!(!text.contains("ticket value"));
        Ok(())
    }
);

async_test!(watch_json_refreshes_local_auth_before_requesting_a_ticket -> TestResult, {
    let (base_url, server) = spawn_refreshing_feed_server().await?;
    let home = crate::isolated_home("logbrew-cli", "watch-refresh")?;
    let _session_path =
        crate::write_test_session(&home, &base_url, "watch-expired", "watch-refresh")?;
    let command = parse_command(["logbrew", "watch", "logs", "--json"])?;
    let env = CliEnvironment {
        base_url,
        token: None,
        home: Some(home),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;
    server.await??;

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
});

async_test!(watch_json_filters_error_and_critical_events_client_side -> TestResult, {
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
    let (base_url, server) = spawn_feed_server(
        "ticket/with spaces", "ticket=ticket%2Fwith%20spaces", messages,
    ).await?;
    let command = parse_command(["logbrew", "watch", "--severity", "error,critical", "--json"])?;
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
        .await?;
    server.await??;

    let text = String::from_utf8(output)?;
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("log_error"));
    assert!(lines[1].contains("issue_critical"));
    assert!(!text.contains("log_warn"));
    assert!(!text.contains("action_1"));
    assert!(!text.contains("ticket/with spaces"));
    Ok(())
});

async_test!(
    watch_json_reconnects_with_fresh_ticket_after_transient_disconnect -> TestResult,
    {
        tokio::time::pause();
        let sessions = vec![
            FeedSession {
                ticket: "first ticket",
                query: "ticket=first%20ticket",
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
                query: "ticket=second%20ticket",
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
        let (base_url, server, completed_sessions) = spawn_feed_server_sessions(sessions).await?;
        let command =
            parse_command(["logbrew", "watch", "logs", "--json"])?;
        let env = test_env(base_url, Some("fixture-token"), "watch-reconnect");
        let mut output = Vec::new();

        let advance_reconnect = async {
            while completed_sessions.load(std::sync::atomic::Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
            while completed_sessions.load(std::sync::atomic::Ordering::Acquire) < 2 {
                tokio::time::advance(std::time::Duration::from_millis(100)).await;
                tokio::task::yield_now().await;
            }
        };
        let (result, ()) = futures_util::future::join(
            execute_command(&command, &env, &mut output),
            advance_reconnect,
        )
        .await;
        result?;
        server.await??;

        let text = String::from_utf8(output)?;
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("log_before_disconnect"));
        assert!(lines[1].contains("log_after_reconnect"));
        assert!(!text.contains("first ticket"));
        assert!(!text.contains("second ticket"));
        Ok(())
    }
);

async_test!(watch_human_requires_json_for_live_stream -> TestResult, {
    let command = parse_command(["logbrew", "follow", "events"])?;
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
        .err()
        .ok_or("human watch must require JSON")?;
    write_runtime_error(&error, command.wants_json(), &mut output)?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "watch streams JSON for agents\nNext: run logbrew watch --json\n"
    );
    Ok(())
});

async fn spawn_feed_server(
    ticket: &'static str,
    query: &'static str,
    messages: Vec<String>,
) -> std::io::Result<(String, tokio::task::JoinHandle<std::io::Result<()>>)> {
    let (url, server, _completed) = spawn_feed_server_sessions(vec![FeedSession {
        ticket,
        query,
        messages,
        close: FeedClose::Clean,
    }])
    .await?;
    Ok((url, server))
}

async fn spawn_refreshing_feed_server()
-> std::io::Result<(String, tokio::task::JoinHandle<std::io::Result<()>>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut expired_stream, _) = listener.accept().await?;
        let expired_request = read_http_request(&mut expired_stream).await?;
        assert_ticket_request(&expired_request, "watch-expired");
        expired_stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            )
            .await?;

        let (mut refresh_stream, _) = listener.accept().await?;
        let refresh_request = read_http_request(&mut refresh_stream).await?;
        assert!(
            refresh_request.starts_with("POST /api/auth/refresh "),
            "expected refresh request"
        );
        assert!(
            refresh_request.contains(r#"{"refresh_token":"watch-refresh"}"#),
            "refresh must use the stored refresh credential"
        );
        write_json_response(
            &mut refresh_stream,
            serde_json::json!({
                "access_token": "watch-fresh",
                "refresh_token": "watch-next-refresh"
            }),
        )
        .await?;

        let (mut ticket_stream, _) = listener.accept().await?;
        let ticket_request = read_http_request(&mut ticket_stream).await?;
        assert_ticket_request(&ticket_request, "watch-fresh");
        write_json_response(
            &mut ticket_stream,
            serde_json::json!({ "ticket": "watch-ticket" }),
        )
        .await?;

        let (live_stream, _) = listener.accept().await?;
        let mut websocket = accept_hdr_async(live_stream, FeedHandshake("ticket=watch-ticket"))
            .await
            .map_err(std::io::Error::other)?;
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
            .map_err(std::io::Error::other)?;
        websocket.close(None).await.map_err(std::io::Error::other)
    });
    Ok((format!("http://{address}"), server))
}

struct FeedSession {
    ticket: &'static str,
    query: &'static str,
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
) -> std::io::Result<(
    String,
    tokio::task::JoinHandle<std::io::Result<()>>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let completed_sessions = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server_counter = std::sync::Arc::clone(&completed_sessions);
    let server = tokio::spawn(async move {
        for session in sessions {
            let (mut ticket_stream, _) = listener.accept().await?;
            let request = read_http_request(&mut ticket_stream).await?;
            assert_ticket_request(&request, "fixture-token");
            write_json_response(
                &mut ticket_stream,
                serde_json::json!({ "ticket": session.ticket }),
            )
            .await?;

            let (live_stream, _) = listener.accept().await?;
            let mut websocket = accept_hdr_async(live_stream, FeedHandshake(session.query))
                .await
                .map_err(std::io::Error::other)?;
            for message in session.messages {
                websocket
                    .send(Message::Text(message.into()))
                    .await
                    .map_err(std::io::Error::other)?;
            }
            match session.close {
                FeedClose::Clean => websocket.close(None).await.map_err(std::io::Error::other)?,
                FeedClose::Drop => drop(websocket),
            }
            let _previous = server_counter.fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        Ok::<(), std::io::Error>(())
    });
    Ok((format!("http://{address}"), server, completed_sessions))
}

async fn read_http_request<R: tokio::io::AsyncRead + Unpin>(
    stream: &mut R,
) -> std::io::Result<String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "incomplete feed request",
            ));
        }
        request.extend_from_slice(&buffer[..read]);
        if request.len() > 65_536 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "feed request exceeds fixture limit",
            ));
        }
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
            if request.len() - header_end - 4 >= content_length {
                break;
            }
        }
    }
    String::from_utf8(request)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

async_test!(feed_reader_rejects_incomplete_and_oversized_requests -> TestResult, {
    for (bytes, expected) in [
        (b"POST /incomplete".to_vec(), std::io::ErrorKind::UnexpectedEof),
        (
            b"POST / HTTP/1.1\r\ncontent-length: 2\r\n\r\nx".to_vec(),
            std::io::ErrorKind::UnexpectedEof,
        ),
        (
            format!("POST / HTTP/1.1\r\ncontent-length: {}\r\n\r\n", usize::MAX).into_bytes(),
            std::io::ErrorKind::UnexpectedEof,
        ),
        (vec![b'x'; 65_537], std::io::ErrorKind::InvalidData),
    ] {
        let (mut writer, mut reader) = tokio::io::duplex(bytes.len());
        writer.write_all(&bytes).await?;
        drop(writer);
        let error = read_http_request(&mut reader)
            .await
            .err()
            .ok_or("malformed feed request must fail")?;
        assert_eq!(error.kind(), expected);
    }
    Ok(())
});

struct FeedHandshake<'a>(&'a str);

impl server::Callback for FeedHandshake<'_> {
    fn on_request(
        self,
        request: &server::Request,
        response: server::Response,
    ) -> Result<server::Response, server::ErrorResponse> {
        assert_eq!(
            request.uri().path(),
            "/api/feed/live",
            "unexpected feed path"
        );
        assert_eq!(
            request.uri().query(),
            Some(self.0),
            "ticket query must match exactly"
        );
        Ok(response)
    }
}

fn assert_ticket_request(request: &str, token: &str) {
    assert!(
        request.starts_with("POST /api/feed/ticket "),
        "expected ticket request"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {token}")),
        "ticket request must use the expected credential"
    );
}

async fn write_json_response(
    stream: &mut tokio::net::TcpStream,
    body: serde_json::Value,
) -> std::io::Result<()> {
    let body = body.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await
}

fn test_env(base_url: impl Into<String>, token: Option<&str>, name: &str) -> CliEnvironment {
    CliEnvironment {
        base_url: base_url.into(),
        token: token.map(str::to_owned),
        home: Some(std::env::temp_dir().join(format!("logbrew-{name}-test"))),
        cwd: None,
    }
}
