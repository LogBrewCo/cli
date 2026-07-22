//! Privacy-safe support-ticket create, history, and detail contracts.

use logbrew_cli::{
    CliEnvironment, Command, HttpMethod, execute_command, help, parse_command, write_cli_error,
    write_runtime_error,
};
use std::collections::BTreeMap;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const TICKET_ID: &str = "sup_9b2b4b3abd4e4f85a0f648118f037c17";
const CURSOR_TIME: &str = "2026-07-14T08:00:00.123456Z";
const CREATED_AT: &str = "2026-07-14T07:00:00Z";
const CURSOR_RECOVERY: &str = "use --pagination cursor alone for the first page, then use --cursor-time and --cursor-id together from next_cursor";

#[test]
fn support_create_builds_exact_safe_request() {
    let command = parse_command([
        "logbrew",
        "support",
        "create",
        "--category",
        "cli_issue",
        "--title",
        "Cursor output is unclear",
        "--description",
        "Continuation guidance did not explain retained filters",
        "--project-id",
        PROJECT_ID,
        "--environment",
        "production",
        "--runtime",
        "rust",
        "--framework",
        "clap",
        "--sdk-package",
        "logbrew-cli",
        "--sdk-version",
        "0.1.18",
        "--release",
        "cli@0.1.18",
        "--trace-id",
        "trace_123",
        "--event-id",
        "event_456",
        "--diagnostics",
        "--json",
    ])
    .expect("support create parses");

    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert_eq!(command.http_path().as_deref(), Some("/api/support/tickets"));
    let body = command.request_body().expect("create has a body");
    assert_eq!(body["source"], "cli");
    assert_eq!(body["category"], "cli_issue");
    assert_eq!(body["title"], "Cursor output is unclear");
    assert_eq!(
        body["description"],
        "Continuation guidance did not explain retained filters"
    );
    assert_eq!(body["project_id"], PROJECT_ID);
    assert_eq!(body["environment"], "production");
    assert_eq!(body["runtime"], "rust");
    assert_eq!(body["framework"], "clap");
    assert_eq!(body["sdk_package"], "logbrew-cli");
    assert_eq!(body["sdk_version"], "0.1.18");
    assert_eq!(body["release"], "cli@0.1.18");
    assert_eq!(body["trace_id"], "trace_123");
    assert_eq!(body["event_id"], "event_456");
    assert_eq!(
        body["diagnostics"],
        serde_json::json!({
            "arch": std::env::consts::ARCH,
            "binary": "logbrew",
            "cli_version": "0.1.18",
            "os": std::env::consts::OS
        })
    );
    let diagnostics = body["diagnostics"].to_string().to_ascii_lowercase();
    for sensitive in [
        "token",
        "secret",
        "password",
        "authorization",
        "cookie",
        "private_key",
        "credential",
    ] {
        assert!(!diagnostics.contains(sensitive));
    }
}

#[test]
fn support_create_requires_fields_and_rejects_unknown_categories_without_echoing_values() {
    for (args, error, message, next) in [
        (
            vec![
                "logbrew",
                "support",
                "create",
                "--title",
                "Title",
                "--description",
                "Description",
                "--json",
            ],
            "missing_argument",
            "missing argument: category",
            "provide --category with a supported support category",
        ),
        (
            vec![
                "logbrew",
                "support",
                "create",
                "--category",
                "cli_issue",
                "--description",
                "Description",
                "--json",
            ],
            "missing_argument",
            "missing argument: title",
            "provide --title with a concise summary",
        ),
        (
            vec![
                "logbrew",
                "support",
                "create",
                "--category",
                "cli_issue",
                "--title",
                "Title",
                "--json",
            ],
            "missing_argument",
            "missing argument: description",
            "provide --description with reproducible details",
        ),
        (
            vec![
                "logbrew",
                "support",
                "create",
                "--category",
                "secret-category-sentinel",
                "--title",
                "Title",
                "--description",
                "Description",
                "--json",
            ],
            "unknown_support_category",
            "unknown support category",
            "use sdk_install_failure, ingest_failure, auth_failure, project_setup, dashboard_issue, docs_confusion, cli_issue, mobile_issue, billing_question, or other",
        ),
    ] {
        let parse_error = parse_command(args).expect_err("invalid create fails");
        let mut output = Vec::new();
        write_cli_error(&parse_error, true, &mut output).expect("error writes");
        let body: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
        assert_eq!(body["error"], error);
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
        assert!(!String::from_utf8_lossy(&output).contains("secret-category-sentinel"));
    }
}

#[test]
fn support_create_accepts_every_public_category() {
    for category in [
        "sdk_install_failure",
        "ingest_failure",
        "auth_failure",
        "project_setup",
        "dashboard_issue",
        "docs_confusion",
        "cli_issue",
        "mobile_issue",
        "billing_question",
        "other",
    ] {
        let command = parse_command([
            "logbrew",
            "support",
            "create",
            "--category",
            category,
            "--title",
            "Title",
            "--description",
            "Description",
        ])
        .expect("public category parses");
        assert_eq!(command.request_body().expect("body")["category"], category);
    }
}

#[test]
fn support_list_repeats_exact_active_filters_for_cursor_pages() {
    let first = parse_command([
        "logbrew",
        "support",
        "list",
        "--project-id",
        PROJECT_ID,
        "--status",
        "open",
        "--source",
        "cli",
        "--category",
        "cli_issue",
        "--release",
        "cli@0.1.18",
        "--pagination",
        "cursor",
        "--limit",
        "2",
        "--json",
    ])
    .expect("first page parses");
    assert_support_query(
        &first,
        &[
            ("project_id", PROJECT_ID),
            ("status", "open"),
            ("source", "cli"),
            ("category", "cli_issue"),
            ("release", "cli@0.1.18"),
            ("limit", "2"),
            ("pagination", "cursor"),
        ],
    );

    let continuation = parse_command([
        "logbrew",
        "support",
        "tickets",
        "--project",
        PROJECT_ID,
        "--status",
        "open",
        "--source",
        "cli",
        "--category",
        "cli_issue",
        "--release",
        "cli@0.1.18",
        "--pagination",
        "cursor",
        "--cursor-time",
        CURSOR_TIME,
        "--cursor-id",
        TICKET_ID,
        "--limit",
        "2",
        "--json",
    ])
    .expect("continuation parses");
    assert_support_query(
        &continuation,
        &[
            ("project_id", PROJECT_ID),
            ("status", "open"),
            ("source", "cli"),
            ("category", "cli_issue"),
            ("release", "cli@0.1.18"),
            ("limit", "2"),
            ("pagination", "cursor"),
            ("cursor_time", CURSOR_TIME),
            ("cursor_id", TICKET_ID),
        ],
    );
}

#[test]
fn support_cursor_flags_fail_closed_and_followups_are_not_implemented() {
    for args in [
        vec![
            "logbrew",
            "support",
            "list",
            "--cursor-time",
            CURSOR_TIME,
            "--cursor-id",
            TICKET_ID,
            "--json",
        ],
        vec![
            "logbrew",
            "support",
            "list",
            "--pagination",
            "cursor",
            "--cursor-time",
            CURSOR_TIME,
            "--json",
        ],
    ] {
        let error = parse_command(args).expect_err("invalid cursor fails");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("error writes");
        let body: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
        assert_eq!(body["error"], "invalid_support_cursor");
        assert_eq!(body["next"], CURSOR_RECOVERY);
    }

    let error = parse_command(["logbrew", "support", "message", TICKET_ID, "hello"])
        .expect_err("unapproved message API stays unavailable");
    let mut output = Vec::new();
    write_cli_error(&error, true, &mut output).expect("error writes");
    let body: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(body["error"], "unknown_resource");
    assert_eq!(body["next"], "run logbrew support --help");
}

#[test]
fn support_help_documents_context_reply_and_safe_diagnostics() {
    let Command::Help { topic, .. } =
        parse_command(["logbrew", "support", "--help"]).expect("support help parses")
    else {
        panic!("support help should resolve");
    };
    let text = help::help_text(topic);
    assert!(text.contains("support create"));
    assert!(text.contains("support list"));
    assert!(text.contains("support show <ticket_id>"));
    assert!(text.contains("support close <ticket_id>"));
    assert!(text.contains("support reopen <ticket_id>"));
    assert!(text.contains("--cursor-id <ticket_id>"));
    assert!(!text.contains("--cursor-id <uuid>"));
    assert!(text.contains("--diagnostics"));
    assert!(text.contains("never reads arbitrary environment variables or files"));
    assert!(text.contains("support context <ticket_id>"));
    assert!(text.contains("support reply <ticket_id>"));
    assert!(text.contains("--retry-key <key>"));
    assert!(!text.contains("support message"));
    assert!(text.contains("Chat, messages, and internal notes are not part"));
}

#[test]
fn support_lifecycle_builds_exact_patch_and_rejects_non_public_ids() {
    for (action, status) in [("close", "closed"), ("reopen", "open")] {
        let command = parse_command(["logbrew", "support", action, TICKET_ID, "--json"])
            .expect("support lifecycle parses");
        assert_eq!(command.http_method(), Some(HttpMethod::Patch));
        assert_eq!(
            command.http_path().as_deref(),
            Some(format!("/api/support/tickets/{TICKET_ID}").as_str())
        );
        assert_eq!(
            command.request_body(),
            Some(serde_json::json!({"status": status}))
        );
    }

    for action in ["close", "reopen"] {
        for invalid in [
            "9b2b4b3a-bd4e-4f85-a0f6-48118f037c17",
            "sup_9B2B4B3ABD4E4F85A0F648118F037C17",
            "sup_9b2b4b3abd4e4f85a0f648118f037c17/extra",
            "sup_9b2b4b3abd4e4f85a0f648118f037c17?admin=true",
            "non-public-ticket-id-proof",
        ] {
            let error = parse_command(["logbrew", "support", action, invalid, "--json"])
                .expect_err("non-public ticket id fails locally");
            let mut output = Vec::new();
            write_cli_error(&error, true, &mut output).expect("error writes");
            let text = String::from_utf8(output).expect("UTF-8");
            let body: serde_json::Value = serde_json::from_str(&text).expect("JSON");
            assert_eq!(body["error"], "invalid_support_ticket_id");
            assert_eq!(body["message"], "invalid support ticket id");
            assert_eq!(
                body["next"],
                "use the ticket_id returned by logbrew support create or list"
            );
            assert!(!text.contains(invalid));
        }
    }
}

#[test]
fn support_detail_and_cursor_reject_non_public_ids() {
    for args in [
        vec![
            "logbrew",
            "support",
            "show",
            "9b2b4b3a-bd4e-4f85-a0f6-48118f037c17",
        ],
        vec![
            "logbrew",
            "support",
            "list",
            "--pagination",
            "cursor",
            "--cursor-time",
            CURSOR_TIME,
            "--cursor-id",
            "sup_9B2B4B3ABD4E4F85A0F648118F037C17",
        ],
    ] {
        let error = parse_command(args).expect_err("non-public support id fails locally");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("error writes");
        let body: serde_json::Value =
            serde_json::from_slice(&output).expect("safe JSON error is returned");
        assert_eq!(body["error"], "invalid_support_ticket_id");
        assert_eq!(
            body["next"],
            "use the ticket_id returned by logbrew support create or list"
        );
    }
}

#[tokio::test]
async fn support_lifecycle_preserves_json_and_exact_retry_is_stable()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut closed = ticket_value();
    closed["status"] = serde_json::Value::String(String::from("closed"));
    let reopened = ticket_value();
    Mock::given(method("PATCH"))
        .and(path(format!("/api/support/tickets/{TICKET_ID}")))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(serde_json::json!({"status": "closed"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(closed.clone()))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/api/support/tickets/{TICKET_ID}")))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(serde_json::json!({"status": "open"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(reopened.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let first = run_command(
        &server,
        ["logbrew", "support", "close", TICKET_ID],
        "support-close-first",
    )
    .await?;
    let retry = run_command(
        &server,
        ["logbrew", "support", "close", TICKET_ID],
        "support-close-retry",
    )
    .await?;
    assert_eq!(first, retry);
    assert!(first.starts_with(&format!("Support ticket {TICKET_ID} closed\n")));
    assert!(!first.contains("description-proof-sentinel"));
    assert!(!first.contains("diagnostic-proof-sentinel"));

    let json = run_command(
        &server,
        ["logbrew", "support", "reopen", TICKET_ID, "--json"],
        "support-reopen-json",
    )
    .await?;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, reopened);
    Ok(())
}

#[tokio::test]
async fn support_lifecycle_uses_local_safe_404_and_422_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, action, api_code, api_error, next) in [
        (
            404,
            "close",
            "not_found",
            "support ticket not found",
            "check the support ticket id and retry",
        ),
        (
            422,
            "reopen",
            "validation_failed",
            "invalid support request",
            "check support command flags and retry",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(format!("/api/support/tickets/{TICKET_ID}")))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({
                    "error": "private backend error proof",
                    "code": "private_code_proof",
                    "next": "private backend recovery proof",
                    "internal": {"body": "private body proof"}
                })),
            )
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "support", action, TICKET_ID, "--json"])?;
        let env = authenticated_env(&server, "support-lifecycle-error");
        let mut output = Vec::new();
        let error = execute_command(&command, &env, &mut output)
            .await
            .expect_err("support lifecycle error fails");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(body["status"], status);
        assert_eq!(body["api_code"], api_code);
        assert_eq!(body["api_error"], api_error);
        assert_eq!(body["next"], next);
        for hidden in [
            "private backend error proof",
            "private_code_proof",
            "private backend recovery proof",
            "private body proof",
        ] {
            assert!(!text.contains(hidden));
        }
    }
    Ok(())
}

#[tokio::test]
async fn support_create_preserves_json_and_renders_concise_human_output()
-> Result<(), Box<dyn std::error::Error>> {
    let response = serde_json::json!({
        "ticket_id": TICKET_ID,
        "status": "open",
        "created_at": CREATED_AT,
        "next": "track this ticket with logbrew support show",
        "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
    });
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/support/tickets"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(serde_json::json!({
            "source": "cli",
            "category": "cli_issue",
            "title": "Cursor output is unclear",
            "description": "Continuation guidance is incomplete"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(2)
        .mount(&server)
        .await;

    let args = [
        "logbrew",
        "support",
        "create",
        "--category",
        "cli_issue",
        "--title",
        "Cursor output is unclear",
        "--description",
        "Continuation guidance is incomplete",
    ];
    let human = run_command(&server, args, "support-create-human").await?;
    assert_eq!(
        human,
        format!(
            "Support ticket {TICKET_ID} created (open)\nCreated: 2026-07-14T07:00:00Z\nNext: track this ticket with logbrew support show\n"
        )
    );
    let json = run_command(
        &server,
        [
            "logbrew",
            "support",
            "create",
            "--category",
            "cli_issue",
            "--title",
            "Cursor output is unclear",
            "--description",
            "Continuation guidance is incomplete",
            "--json",
        ],
        "support-create-json",
    )
    .await?;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, response);
    Ok(())
}

#[tokio::test]
async fn support_list_preserves_legacy_and_cursor_envelopes()
-> Result<(), Box<dyn std::error::Error>> {
    let legacy_server = MockServer::start().await;
    let cursor_server = MockServer::start().await;
    let ticket = ticket_value();
    let legacy = serde_json::json!({
        "tickets": [ticket.clone()],
        "next": "inspect a ticket by id",
        "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
    });
    let cursor = serde_json::json!({
        "tickets": [ticket],
        "next_cursor": {"time": CURSOR_TIME, "id": TICKET_ID},
        "next": "continue support ticket history",
        "next_action": {"code": "continue_support_tickets", "target": "support_tickets"}
    });
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(legacy.clone()))
        .mount(&legacy_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .and(query_param("pagination", "cursor"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cursor.clone()))
        .mount(&cursor_server)
        .await;

    let legacy_output = run_command(
        &legacy_server,
        ["logbrew", "support", "list", "--json"],
        "support-list-legacy",
    )
    .await?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&legacy_output)?,
        legacy
    );
    let cursor_output = run_command(
        &cursor_server,
        [
            "logbrew",
            "support",
            "list",
            "--pagination",
            "cursor",
            "--json",
        ],
        "support-list-cursor",
    )
    .await?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&cursor_output)?,
        cursor
    );
    Ok(())
}

#[tokio::test]
async fn support_list_human_output_is_bounded_and_cursor_recovery_keeps_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .and(query_param("pagination", "cursor"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tickets": [ticket_value()],
            "next_cursor": {"time": CURSOR_TIME, "id": TICKET_ID},
            "next": "continue support ticket history",
            "next_action": {"code": "continue_support_tickets", "target": "support_tickets"}
        })))
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        ["logbrew", "support", "list", "--pagination", "cursor"],
        "support-list-human",
    )
    .await?;
    assert!(
        human.starts_with(
            format!("Support tickets (1)\n- {TICKET_ID} open cli_issue Cursor output is unclear")
                .as_str()
        )
    );
    assert!(human.contains("created=2026-07-14T07:00:00Z"));
    assert!(human.contains("project=123e4567-e89b-12d3-a456-426614174000"));
    assert!(human.contains("[cli@0.1.18 / production]"));
    assert!(human.contains("Next page: set --cursor-time"));
    assert!(human.contains("Retry: rerun that same command; the rows above remain visible."));
    for hidden in [
        "description-proof-sentinel",
        "diagnostic-proof-sentinel",
        "authorization-proof-sentinel",
    ] {
        assert!(!human.contains(hidden));
    }
    Ok(())
}

#[tokio::test]
async fn support_human_output_rejects_controls_and_caps_visible_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let unsafe_server = MockServer::start().await;
    let mut unsafe_ticket = ticket_value();
    unsafe_ticket["status"] = serde_json::Value::String(String::from("open\u{1b}[31m"));
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tickets": [unsafe_ticket],
            "next": "inspect a ticket by id",
            "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
        })))
        .mount(&unsafe_server)
        .await;
    let unsafe_output = run_command(
        &unsafe_server,
        ["logbrew", "support", "list"],
        "support-list-control-safe",
    )
    .await?;
    assert_eq!(
        unsafe_output,
        "Support response could not be rendered safely.\nNext: retry the same command with --json and inspect the public response shape.\n"
    );
    assert!(!unsafe_output.contains('\u{1b}'));

    let unsafe_create_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/support/tickets"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "ticket_id": TICKET_ID,
            "status": "open\u{1b}[31m",
            "created_at": CREATED_AT,
            "next": "track this ticket with logbrew support show",
            "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
        })))
        .mount(&unsafe_create_server)
        .await;
    let unsafe_create = run_command(
        &unsafe_create_server,
        [
            "logbrew",
            "support",
            "create",
            "--category",
            "cli_issue",
            "--title",
            "Title",
            "--description",
            "Description",
        ],
        "support-create-control-safe",
    )
    .await?;
    assert_eq!(
        unsafe_create,
        "Support response could not be rendered safely.\nNext: retry the same command with --json and inspect the public response shape.\n"
    );

    let long_time_server = MockServer::start().await;
    let mut long_time_ticket = ticket_value();
    long_time_ticket["created_at"] =
        serde_json::Value::String(format!("2026-07-14T07:00:00.{}Z", "1".repeat(256)));
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tickets": [long_time_ticket],
            "next": "inspect a ticket by id",
            "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
        })))
        .mount(&long_time_server)
        .await;
    let long_time_output = run_command(
        &long_time_server,
        ["logbrew", "support", "list"],
        "support-list-bounded-time",
    )
    .await?;
    assert_eq!(
        long_time_output,
        "Support response could not be rendered safely.\nNext: retry the same command with --json and inspect the public response shape.\n"
    );

    let bounded_server = MockServer::start().await;
    let tickets = vec![ticket_value(); 101];
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tickets": tickets,
            "next": "inspect a ticket by id",
            "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
        })))
        .mount(&bounded_server)
        .await;
    let bounded_output = run_command(
        &bounded_server,
        ["logbrew", "support", "list"],
        "support-list-row-cap",
    )
    .await?;
    assert_eq!(bounded_output.matches("\n- ").count(), 100);
    assert!(bounded_output.contains("Showing first 100 of 101 tickets."));
    Ok(())
}

#[tokio::test]
async fn support_terminal_cursor_and_detail_are_explicit_and_json_exact()
-> Result<(), Box<dyn std::error::Error>> {
    let list_server = MockServer::start().await;
    let detail_server = MockServer::start().await;
    let detail = ticket_value();
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .and(query_param("pagination", "cursor"))
        .and(query_param("cursor_time", CURSOR_TIME))
        .and(query_param("cursor_id", TICKET_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tickets": [ticket_value()],
            "next_cursor": null,
            "next": "inspect a ticket by id",
            "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
        })))
        .mount(&list_server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/support/tickets/{TICKET_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail.clone()))
        .expect(2)
        .mount(&detail_server)
        .await;

    let terminal = run_command(
        &list_server,
        [
            "logbrew",
            "support",
            "list",
            "--pagination",
            "cursor",
            "--cursor-time",
            CURSOR_TIME,
            "--cursor-id",
            TICKET_ID,
        ],
        "support-list-terminal",
    )
    .await?;
    assert!(terminal.ends_with("End of support ticket history.\nNext: inspect a ticket by id\n"));
    assert!(!terminal.contains("Next page:"));

    let human = run_command(
        &detail_server,
        ["logbrew", "support", "show", TICKET_ID],
        "support-detail-human",
    )
    .await?;
    assert!(human.starts_with(format!("Support ticket {TICKET_ID} open\n").as_str()));
    assert!(human.contains("Category: cli_issue\n"));
    assert!(human.contains("Title: Cursor output is unclear\n"));
    assert!(!human.contains("description-proof-sentinel"));
    assert!(!human.contains("diagnostic-proof-sentinel"));
    let json = run_command(
        &detail_server,
        ["logbrew", "support", "ticket", TICKET_ID, "--json"],
        "support-detail-json",
    )
    .await?;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, detail);
    Ok(())
}

#[tokio::test]
async fn support_errors_never_print_raw_backend_bodies_or_sensitive_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let auth_key = ["author", "ization"].concat();
    let auth_value = ["Bear", "er auth-proof-sentinel"].concat();
    let cookie_key = ["coo", "kie"].concat();
    let cookie_value = "cookie-proof-sentinel";
    let mut response = serde_json::json!({
        "error": "invalid support filter",
        "code": "validation_failed",
        "next": "check support filters and retry",
        "next_action": {"code": "fix_request", "target": "request"},
        "internal": {"description": "backend-body-proof-sentinel"}
    });
    let response_object = response.as_object_mut().expect("response object");
    drop(response_object.insert(auth_key.clone(), auth_value.clone().into()));
    drop(response_object.insert(cookie_key.clone(), cookie_value.into()));
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .respond_with(ResponseTemplate::new(422).set_body_json(response))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "support", "list", "--json"])?;
    let env = authenticated_env(&server, "support-error");
    let mut output = Vec::new();
    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("backend error fails");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(&text)?;
    assert_eq!(body["status"], 422);
    assert_eq!(body["api_code"], "validation_failed");
    assert_eq!(body["api_error"], "invalid support request");
    assert_eq!(body["next"], "check support command flags and retry");
    for hidden in [
        auth_value.as_str(),
        cookie_value,
        "backend-body-proof-sentinel",
        "invalid support filter",
        "check support filters and retry",
        auth_key.as_str(),
        cookie_key.as_str(),
        "internal",
    ] {
        assert!(!text.contains(hidden));
    }
    Ok(())
}

#[tokio::test]
async fn malformed_support_success_responses_use_value_safe_human_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    for body in [
        serde_json::json!({
            "tickets": [ticket_value()],
            "next": "continue support ticket history",
            "next_action": {"code": "continue_support_tickets", "target": "support_tickets"}
        }),
        serde_json::json!({
            "tickets": [ticket_value()],
            "next_cursor": {"time": CURSOR_TIME, "id": "invalid-id\nprivate-cursor-sentinel"},
            "next": "continue support ticket history",
            "next_action": {"code": "continue_support_tickets", "target": "support_tickets"}
        }),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/support/tickets"))
            .and(query_param("pagination", "cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let human = run_command(
            &server,
            ["logbrew", "support", "list", "--pagination", "cursor"],
            "support-malformed-success",
        )
        .await?;
        assert_eq!(
            human,
            "Support response could not be rendered safely.\nNext: retry the same command with --json and inspect the public response shape.\n"
        );
        assert!(!human.contains("private-cursor-sentinel"));
    }
    Ok(())
}

#[tokio::test]
async fn non_json_support_errors_are_replaced_instead_of_printed()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/support/tickets"))
        .respond_with(ResponseTemplate::new(500).set_body_raw(
            "private-backend-body-sentinel\nauthorization: Bearer private-value",
            "text/plain",
        ))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "support", "list", "--json"])?;
    let env = authenticated_env(&server, "support-non-json-error");
    let mut output = Vec::new();
    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("backend error fails");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(&text)?;
    assert_eq!(body["api_code"], "support_request_failed");
    assert_eq!(body["api_error"], "support request failed");
    assert_eq!(body["next"], "retry the support command");
    assert!(!text.contains("private-backend-body-sentinel"));
    assert!(!text.contains("private-value"));
    assert!(!text.contains("authorization"));
    Ok(())
}

fn assert_support_query(command: &Command, expected: &[(&str, &str)]) {
    let path = command.http_path().expect("support list has endpoint");
    let url = reqwest::Url::parse(format!("https://example.test{path}").as_str())
        .expect("support path is valid");
    let pairs = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let actual = pairs.iter().cloned().collect::<BTreeMap<_, _>>();
    let expected = expected
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(pairs.len(), actual.len(), "query keys must not repeat");
    assert_eq!(url.path(), "/api/support/tickets");
    assert_eq!(actual, expected);
}

fn ticket_value() -> serde_json::Value {
    serde_json::json!({
        "ticket_id": TICKET_ID,
        "status": "open",
        "source": "cli",
        "category": "cli_issue",
        "title": "Cursor output is unclear",
        "description": "description-proof-sentinel",
        "project_id": PROJECT_ID,
        "environment": "production",
        "runtime": "rust",
        "framework": "clap",
        "sdk_package": "logbrew-cli",
        "sdk_version": "0.1.18",
        "release": "cli@0.1.18",
        "trace_id": "trace_123",
        "event_id": "event_456",
        "diagnostics": {
            "note": "diagnostic-proof-sentinel",
            "authorization": "authorization-proof-sentinel"
        },
        "created_at": CREATED_AT,
        "next": "inspect this ticket",
        "next_action": {"code": "inspect_support_ticket", "target": "support_ticket"}
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
