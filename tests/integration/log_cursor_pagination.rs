//! Log cursor pagination command, response, and recovery contracts.

use logbrew_cli::{
    CliEnvironment, Command, execute_command, help, parse_command, write_cli_error,
    write_runtime_error,
};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const CURSOR_ID: &str = "9b2b4b3a-bd4e-4f85-a0f6-48118f037c17";
const CURSOR_TIME: &str = "2026-07-12T08:00:00.123456Z";
const CURSOR_RECOVERY: &str = "send pagination=cursor alone for the first page, then send cursor_time and cursor_id together from next_cursor";
const CLI_CURSOR_RECOVERY: &str = "use --pagination cursor alone for the first page, then use --cursor-time and --cursor-id together from next_cursor";

#[test]
fn log_cursor_pages_repeat_exact_active_query_filters() {
    let initial = parse_command([
        "logbrew",
        "logs",
        "--project-id",
        PROJECT_ID,
        "--level",
        "error",
        "--search",
        "checkout failed",
        "--trace-id",
        "trace_123",
        "--service-name",
        "checkout-api",
        "--release",
        "checkout@1.2.3",
        "--env",
        "production",
        "--since",
        "24h",
        "--pagination",
        "cursor",
        "--limit",
        "2",
        "--json",
    ])
    .expect("initial log cursor page parses");
    assert_eq!(
        initial.http_path().expect("log page has endpoint"),
        "/api/logs?service_name=checkout-api&severity=error&search=checkout%20failed&since=24h&trace_id=trace_123&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&limit=2"
    );

    let continuation = parse_command([
        "logbrew",
        "logs",
        "--project",
        PROJECT_ID,
        "--severity",
        "error",
        "--search",
        "checkout failed",
        "--trace",
        "trace_123",
        "--service",
        "checkout-api",
        "--release",
        "checkout@1.2.3",
        "--environment",
        "production",
        "--since",
        "24h",
        "--pagination",
        "cursor",
        "--cursor-time",
        CURSOR_TIME,
        "--cursor-id",
        CURSOR_ID,
        "--limit",
        "2",
        "--json",
    ])
    .expect("continuation log cursor page parses");
    assert_eq!(
        continuation.http_path().expect("log page has endpoint"),
        "/api/logs?service_name=checkout-api&severity=error&search=checkout%20failed&since=24h&trace_id=trace_123&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&cursor_time=2026-07-12T08%3A00%3A00.123456Z&cursor_id=9b2b4b3a-bd4e-4f85-a0f6-48118f037c17&limit=2"
    );
}

#[test]
fn log_cursor_flags_fail_closed_with_value_safe_recovery() {
    for (args, error, message) in [
        (
            &[
                "logbrew",
                "logs",
                "--cursor-time",
                CURSOR_TIME,
                "--cursor-id",
                CURSOR_ID,
                "--json",
            ][..],
            "invalid_log_cursor",
            "invalid log cursor: cursor fields require --pagination cursor",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--pagination",
                "cursor",
                "--cursor-time",
                CURSOR_TIME,
                "--json",
            ],
            "invalid_log_cursor",
            "invalid log cursor: --cursor-time and --cursor-id must be used together",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--pagination",
                "secret-pagination-sentinel",
                "--json",
            ],
            "unknown_pagination",
            "unknown pagination mode",
        ),
    ] {
        let parse_error = parse_command(args.iter().copied()).expect_err("cursor input fails");
        let mut output = Vec::new();

        write_cli_error(&parse_error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid JSON");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], error);
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], CLI_CURSOR_RECOVERY);
        assert!(!String::from_utf8_lossy(output.as_slice()).contains("secret-pagination-sentinel"));
    }
}

#[test]
fn log_cursor_help_documents_first_and_continuation_contract() {
    let command = parse_command(["logbrew", "logs", "--help"]).expect("log help parses");
    let Command::Help { topic, .. } = command else {
        panic!("logs help should resolve");
    };
    let text = help::help_text(topic);

    assert!(text.contains("--pagination cursor"));
    assert!(text.contains("--cursor-time <RFC3339>"));
    assert!(text.contains("--cursor-id <uuid>"));
    assert!(text.contains("next_cursor"));
    assert!(text.contains("Keep the same active filters"));
}

#[tokio::test]
async fn log_cursor_json_preserves_legacy_array_and_cursor_envelope()
-> Result<(), Box<dyn std::error::Error>> {
    let legacy_server = MockServer::start().await;
    let cursor_server = MockServer::start().await;
    let log = log_value("checkout failed");
    let envelope = serde_json::json!({
        "logs": [log.clone()],
        "next_cursor": {
            "time": CURSOR_TIME,
            "id": CURSOR_ID
        }
    });
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([log.clone()])))
        .mount(&legacy_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("limit", "1"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope.clone()))
        .mount(&cursor_server)
        .await;

    let legacy = run_command(
        &legacy_server,
        ["logbrew", "logs", "--json"],
        "logs-legacy-json",
    )
    .await?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&legacy)?,
        serde_json::json!([log])
    );

    let cursor = run_command(
        &cursor_server,
        [
            "logbrew",
            "logs",
            "--pagination",
            "cursor",
            "--limit",
            "1",
            "--json",
        ],
        "logs-cursor-json",
    )
    .await?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&cursor)?,
        envelope
    );
    Ok(())
}

#[tokio::test]
async fn log_cursor_human_output_keeps_rows_and_gives_continuation_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("limit", "1"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "logs": [log_value("checkout failed")],
            "next_cursor": {
                "time": CURSOR_TIME,
                "id": CURSOR_ID
            }
        })))
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        ["logbrew", "logs", "--pagination", "cursor", "--limit", "1"],
        "logs-cursor-human",
    )
    .await?;

    assert_eq!(
        human,
        "Logs (1)\n- error checkout failed service=checkout-api trace=trace_123 [checkout@1.2.3 / production]\nNext page: set --cursor-time 2026-07-12T08:00:00.123456Z --cursor-id 9b2b4b3a-bd4e-4f85-a0f6-48118f037c17 on the same command; keep --pagination cursor, --limit, and active filters unchanged.\nRetry: rerun that same command; the rows above remain visible.\n"
    );
    Ok(())
}

#[tokio::test]
async fn terminal_log_cursor_page_is_explicit() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("cursor_time", CURSOR_TIME))
        .and(query_param("cursor_id", CURSOR_ID))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "logs": [log_value("payment retried")],
            "next_cursor": null
        })))
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        [
            "logbrew",
            "logs",
            "--pagination",
            "cursor",
            "--cursor-time",
            CURSOR_TIME,
            "--cursor-id",
            CURSOR_ID,
        ],
        "logs-cursor-terminal",
    )
    .await?;

    assert!(human.starts_with("Logs (1)\n- error payment retried"));
    assert!(human.ends_with("End of log history.\n"));
    assert!(!human.contains("Next page:"));
    Ok(())
}

#[tokio::test]
async fn malformed_log_cursor_envelope_has_value_safe_human_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let malformed = [
        serde_json::json!({
            "logs": [log_value("payment retried")]
        }),
        serde_json::json!({
            "logs": [log_value("payment retried")],
            "next_cursor": {"time": 123, "id": CURSOR_ID}
        }),
        serde_json::json!({
            "logs": [log_value("payment retried")],
            "next_cursor": {"time": "not-rfc3339", "id": CURSOR_ID}
        }),
        serde_json::json!({
            "logs": [log_value("payment retried")],
            "next_cursor": {
                "time": CURSOR_TIME,
                "id": "invalid-id\nNext: unsafe cursor text"
            }
        }),
    ];

    for (index, body) in malformed.into_iter().enumerate() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/logs"))
            .and(query_param("pagination", "cursor"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let home_name = format!("logs-cursor-malformed-{index}");

        let human = run_command(
            &server,
            ["logbrew", "logs", "--pagination", "cursor"],
            home_name.as_str(),
        )
        .await?;

        assert_eq!(
            human,
            "Logs response could not be rendered safely.\nNext: retry the same command with --json and inspect next_cursor.\n"
        );
        assert!(!human.contains("unsafe cursor text"));
        assert!(!human.contains("End of log history."));
    }
    Ok(())
}

#[tokio::test]
async fn non_json_log_cursor_response_has_value_safe_human_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("pagination", "cursor"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("not-json\nNext page: unsafe response text", "text/plain"),
        )
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        ["logbrew", "logs", "--pagination", "cursor"],
        "logs-cursor-non-json",
    )
    .await?;

    assert_eq!(
        human,
        "Logs response could not be rendered safely.\nNext: retry the same command with --json and inspect next_cursor.\n"
    );
    assert!(!human.contains("unsafe response text"));
    Ok(())
}

#[tokio::test]
async fn log_cursor_preserves_backend_validation_without_echoing_values()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("cursor_time", "not-a-time"))
        .and(query_param("cursor_id", CURSOR_ID))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "error": "invalid cursor pagination",
            "code": "validation_failed",
            "next": CURSOR_RECOVERY,
            "next_action": {
                "code": "fix_request",
                "target": "request"
            }
        })))
        .mount(&server)
        .await;
    let command = parse_command([
        "logbrew",
        "logs",
        "--pagination",
        "cursor",
        "--cursor-time",
        "not-a-time",
        "--cursor-id",
        CURSOR_ID,
        "--json",
    ])?;
    let env = authenticated_env(&server, "log-cursor-invalid");
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("invalid cursor fails");
    write_runtime_error(&error, true, &mut output)?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["status"], 422);
    assert_eq!(body["api_code"], "validation_failed");
    assert_eq!(body["api_error"], "invalid cursor pagination");
    assert_eq!(body["next"], CURSOR_RECOVERY);
    assert!(!String::from_utf8_lossy(output.as_slice()).contains("not-a-time"));
    Ok(())
}

fn log_value(message: &str) -> serde_json::Value {
    serde_json::json!({
        "message": message,
        "severity": "error",
        "service_name": "checkout-api",
        "trace_id": "trace_123",
        "release": "checkout@1.2.3",
        "environment": "production"
    })
}

async fn run_command<const N: usize>(
    server: &MockServer,
    args: [&'static str; N],
    home_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let command = parse_command(args)?;
    let env = authenticated_env(server, home_name);
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

fn authenticated_env(server: &MockServer, home_name: &str) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some("test-token".to_owned()),
        home: Some(std::env::temp_dir().join(format!("logbrew-{home_name}"))),
        cwd: None,
    }
}
