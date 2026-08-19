//! Issue cursor pagination command, response, and recovery contracts.

use logbrew_cli::{
    CliEnvironment, Command, execute_command, help, parse_command, write_cli_error,
    write_runtime_error,
};
use std::collections::BTreeMap;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const CURSOR_ID: &str = "9b2b4b3a-bd4e-4f85-a0f6-48118f037c17";
const ISSUE_ID: &str = CURSOR_ID;
const CURSOR_TIME: &str = "2026-07-13T08:00:00.123456Z";
const CURSOR_RECOVERY: &str = "send pagination=cursor alone for the first page, then send cursor_time and cursor_id together from next_cursor";
const CLI_CURSOR_RECOVERY: &str = "use --pagination cursor alone for the first page, then use --cursor-time and --cursor-id together from next_cursor";

#[test]
fn issue_cursor_pages_repeat_exact_active_query_filters() {
    let initial = parse_command([
        "logbrew",
        "issues",
        "--project-id",
        PROJECT_ID,
        "--status",
        "open",
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
    .expect("initial issue cursor page parses");
    assert_issue_query(
        &initial,
        &[
            ("service_name", "checkout-api"),
            ("since", "24h"),
            ("status", "unresolved"),
            ("project_id", PROJECT_ID),
            ("release", "checkout@1.2.3"),
            ("environment", "production"),
            ("pagination", "cursor"),
            ("limit", "2"),
        ],
    );

    let continuation = parse_command([
        "logbrew",
        "issues",
        "--project",
        PROJECT_ID,
        "--status",
        "unresolved",
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
    .expect("continuation issue cursor page parses");
    assert_issue_query(
        &continuation,
        &[
            ("service_name", "checkout-api"),
            ("since", "24h"),
            ("status", "unresolved"),
            ("project_id", PROJECT_ID),
            ("release", "checkout@1.2.3"),
            ("environment", "production"),
            ("pagination", "cursor"),
            ("cursor_time", CURSOR_TIME),
            ("cursor_id", CURSOR_ID),
            ("limit", "2"),
        ],
    );
}

#[test]
fn issue_cursor_flags_fail_closed_with_value_safe_recovery() {
    for (args, error, message) in [
        (
            &[
                "logbrew",
                "issues",
                "--cursor-time",
                CURSOR_TIME,
                "--cursor-id",
                CURSOR_ID,
                "--json",
            ][..],
            "invalid_issue_cursor",
            "invalid issue cursor: cursor fields require --pagination cursor",
        ),
        (
            &[
                "logbrew",
                "issues",
                "--pagination",
                "cursor",
                "--cursor-time",
                CURSOR_TIME,
                "--json",
            ],
            "invalid_issue_cursor",
            "invalid issue cursor: --cursor-time and --cursor-id must be used together",
        ),
        (
            &[
                "logbrew",
                "issues",
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
fn issue_cursor_help_documents_first_and_continuation_contract() {
    let command = parse_command(["logbrew", "issues", "--help"]).expect("issue help parses");
    let Command::Help { topic, .. } = command else {
        panic!("issues help should resolve");
    };
    let text = help::help_text(topic);

    assert!(text.contains("--pagination cursor"));
    assert!(text.contains("--cursor-time <RFC3339>"));
    assert!(text.contains("--cursor-id <uuid>"));
    assert!(text.contains("next_cursor"));
    assert!(text.contains("Keep the same active filters"));
}

#[tokio::test]
async fn issue_cursor_json_preserves_legacy_array_and_cursor_envelope()
-> Result<(), Box<dyn std::error::Error>> {
    let legacy_server = MockServer::start().await;
    let cursor_server = MockServer::start().await;
    let issue = issue_value("PaymentError");
    let envelope = serde_json::json!({
        "issues": [issue.clone()],
        "next_cursor": {
            "time": CURSOR_TIME,
            "id": CURSOR_ID
        }
    });
    Mock::given(method("GET"))
        .and(path("/api/telemetry/issues"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([issue.clone()])))
        .mount(&legacy_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/issues"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("limit", "1"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope.clone()))
        .mount(&cursor_server)
        .await;

    let legacy = run_command(
        &legacy_server,
        ["logbrew", "issues", "--json"],
        "issues-legacy-json",
    )
    .await?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&legacy)?,
        serde_json::json!([issue])
    );

    let cursor = run_command(
        &cursor_server,
        [
            "logbrew",
            "issues",
            "--pagination",
            "cursor",
            "--limit",
            "1",
            "--json",
        ],
        "issues-cursor-json",
    )
    .await?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&cursor)?,
        envelope
    );
    Ok(())
}

#[tokio::test]
async fn issue_cursor_human_output_keeps_rows_and_gives_continuation_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/issues"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("limit", "1"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issues": [issue_value("PaymentError")],
            "next_cursor": {
                "time": CURSOR_TIME,
                "id": CURSOR_ID
            }
        })))
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        [
            "logbrew",
            "issues",
            "--pagination",
            "cursor",
            "--limit",
            "1",
        ],
        "issues-cursor-human",
    )
    .await?;

    assert_eq!(
        human,
        "Issues (1)\n- 9b2b4b3a-bd4e-4f85-a0f6-48118f037c17 unresolved error PaymentError occurrences=2 last_seen=2026-07-13T08:00:00.123456Z service=checkout-api trace=trace_123 [checkout@1.2.3 / production]\nNext page: set --cursor-time 2026-07-13T08:00:00.123456Z --cursor-id 9b2b4b3a-bd4e-4f85-a0f6-48118f037c17 on the same command; keep --pagination cursor, --limit, and active filters unchanged.\nRetry: rerun that same command; the rows above remain visible.\n"
    );
    assert!(!human.contains("raw issue message"));
    assert!(!human.contains("stack frame sentinel"));
    assert!(!human.contains("attribute sentinel"));
    Ok(())
}

#[tokio::test]
async fn terminal_issue_cursor_page_is_explicit() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/issues"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("cursor_time", CURSOR_TIME))
        .and(query_param("cursor_id", CURSOR_ID))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issues": [issue_value("PaymentError")],
            "next_cursor": null
        })))
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        [
            "logbrew",
            "issues",
            "--pagination",
            "cursor",
            "--cursor-time",
            CURSOR_TIME,
            "--cursor-id",
            CURSOR_ID,
        ],
        "issues-cursor-terminal",
    )
    .await?;

    assert!(human.starts_with(
        "Issues (1)\n- 9b2b4b3a-bd4e-4f85-a0f6-48118f037c17 unresolved error PaymentError"
    ));
    assert!(human.ends_with("End of issue history.\n"));
    assert!(!human.contains("Next page:"));
    Ok(())
}

#[tokio::test]
async fn malformed_issue_cursor_envelope_has_value_safe_human_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let mut missing_title = issue_value("PaymentError");
    drop(
        missing_title
            .as_object_mut()
            .expect("issue fixture is an object")
            .remove("title"),
    );
    let mut invalid_count = issue_value("PaymentError");
    invalid_count["occurrence_count"] = serde_json::json!("two");
    let mut invalid_last_seen = issue_value("PaymentError");
    invalid_last_seen["last_seen_at"] = serde_json::json!("not-rfc3339");
    let malformed = [
        serde_json::json!({
            "issues": [issue_value("PaymentError")]
        }),
        serde_json::json!({
            "issues": [issue_value("PaymentError")],
            "next_cursor": {"time": 123, "id": CURSOR_ID}
        }),
        serde_json::json!({
            "issues": [issue_value("PaymentError")],
            "next_cursor": {"time": "not-rfc3339", "id": CURSOR_ID}
        }),
        serde_json::json!({
            "issues": [issue_value("PaymentError")],
            "next_cursor": {
                "time": CURSOR_TIME,
                "id": "invalid-id\nNext: unsafe cursor text"
            }
        }),
        serde_json::json!({
            "issues": [missing_title],
            "next_cursor": null
        }),
        serde_json::json!({
            "issues": [invalid_count],
            "next_cursor": null
        }),
        serde_json::json!({
            "issues": [invalid_last_seen],
            "next_cursor": null
        }),
    ];

    for (index, body) in malformed.into_iter().enumerate() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/telemetry/issues"))
            .and(query_param("pagination", "cursor"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let home_name = format!("issues-cursor-malformed-{index}");

        let human = run_command(
            &server,
            ["logbrew", "issues", "--pagination", "cursor"],
            home_name.as_str(),
        )
        .await?;

        assert_eq!(
            human,
            "Issues response could not be rendered safely.\nNext: retry the same command with --json and inspect next_cursor.\n"
        );
        assert!(!human.contains("unsafe cursor text"));
        assert!(!human.contains("End of issue history."));
    }
    Ok(())
}

#[tokio::test]
async fn non_json_issue_cursor_response_has_value_safe_human_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/issues"))
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
        ["logbrew", "issues", "--pagination", "cursor"],
        "issues-cursor-non-json",
    )
    .await?;

    assert_eq!(
        human,
        "Issues response could not be rendered safely.\nNext: retry the same command with --json and inspect next_cursor.\n"
    );
    assert!(!human.contains("unsafe response text"));
    Ok(())
}

#[tokio::test]
async fn issue_cursor_preserves_backend_validation_without_echoing_values()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/issues"))
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
        "issues",
        "--pagination",
        "cursor",
        "--cursor-time",
        "not-a-time",
        "--cursor-id",
        CURSOR_ID,
        "--json",
    ])?;
    let env = authenticated_env(&server, "issue-cursor-invalid");
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

fn issue_value(title: &str) -> serde_json::Value {
    serde_json::json!({
        "id": ISSUE_ID,
        "project_id": PROJECT_ID,
        "fingerprint": "payment-error",
        "status": "unresolved",
        "severity": "error",
        "title": title,
        "message": "raw issue message",
        "stack_trace": "stack frame sentinel",
        "attributes": {"debug": "attribute sentinel"},
        "occurrence_count": 2,
        "service_name": "checkout-api",
        "trace_id": "trace_123",
        "release": "checkout@1.2.3",
        "environment": "production",
        "first_seen_at": "2026-07-13T07:00:00Z",
        "last_seen_at": CURSOR_TIME,
        "next_action": {
            "code": "inspect_trace",
            "target": "trace_summary"
        }
    })
}

fn assert_issue_query(command: &Command, expected: &[(&str, &str)]) {
    let path = command.http_path().expect("issue page has endpoint");
    let url = reqwest::Url::parse(format!("https://example.test{path}").as_str())
        .expect("issue path is a valid URL");
    let actual = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<BTreeMap<_, _>>();
    let expected = expected
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(url.path(), "/api/telemetry/issues");
    assert_eq!(actual, expected);
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
