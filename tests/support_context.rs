//! Privacy-safe support context history and reply contracts.

use logbrew_cli::{
    CliEnvironment, HttpMethod, execute_command, parse_command, write_cli_error,
    write_runtime_error,
};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TICKET_ID: &str = "sup_9b2b4b3abd4e4f85a0f648118f037c17";
const CONTEXT_ID: &str = "ctx_4b2b4b3abd4e4f85a0f648118f037c29";
const SECOND_CONTEXT_ID: &str = "ctx_5b2b4b3abd4e4f85a0f648118f037c30";
const CREATED_AT: &str = "2026-07-14T11:00:00Z";
const RETRY_KEY: &str = "support-context-20260714-1";
const CONTEXT_TEXT: &str = "The CLI stopped after the second request";
const CONTEXT_INPUT: &str = "  The CLI stopped after the second request\n";

#[test]
fn support_context_parser_builds_exact_paths_bodies_and_closed_grammar() {
    let history = parse_command(["logbrew", "support", "context", TICKET_ID, "--json"])
        .expect("support context history parses");
    assert_eq!(history.http_method(), Some(HttpMethod::Get));
    assert_eq!(
        history.http_path().as_deref(),
        Some(format!("/api/support/tickets/{TICKET_ID}/context").as_str())
    );
    assert_eq!(history.request_body(), None);

    let reply = parse_command([
        "logbrew",
        "support",
        "reply",
        TICKET_ID,
        "--context",
        CONTEXT_INPUT,
        "--retry-key",
        RETRY_KEY,
        "--diagnostics",
        "--json",
    ])
    .expect("support context reply parses");
    assert_eq!(reply.http_method(), Some(HttpMethod::Post));
    assert_eq!(
        reply.http_path().as_deref(),
        Some(format!("/api/support/tickets/{TICKET_ID}/context").as_str())
    );
    assert_eq!(
        reply.request_body(),
        Some(serde_json::json!({
            "context": CONTEXT_TEXT,
            "diagnostics": {
                "arch": std::env::consts::ARCH,
                "binary": "logbrew",
                "cli_version": "0.1.18",
                "os": std::env::consts::OS
            }
        }))
    );

    let hyphen_values = parse_command([
        "logbrew",
        "support",
        "reply",
        TICKET_ID,
        "--context=- second request failed",
        "--retry-key=-retry-1",
    ])
    .expect("leading hyphen values remain valid");
    assert_eq!(
        hyphen_values.request_body(),
        Some(serde_json::json!({"context": "- second request failed"}))
    );

    for action in ["context", "reply"] {
        let error = parse_command([
            "logbrew",
            "support",
            action,
            "sup_9B2B4B3ABD4E4F85A0F648118F037C17",
        ])
        .expect_err("non-public support id fails locally");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("error writes");
        let text = String::from_utf8(output).expect("UTF-8");
        assert!(text.contains("invalid_support_ticket_id"));
        assert!(!text.contains("9B2B4B3A"));
    }

    for action in ["message", "messages", "chat", "note", "internal-note"] {
        assert!(
            parse_command(["logbrew", "support", action, TICKET_ID]).is_err(),
            "{action} remains closed"
        );
    }

    for args in [
        vec![
            "logbrew",
            "support",
            "reply",
            TICKET_ID,
            "--context",
            CONTEXT_TEXT,
        ],
        vec![
            "logbrew",
            "support",
            "reply",
            TICKET_ID,
            "--retry-key",
            RETRY_KEY,
        ],
    ] {
        assert!(parse_command(args).is_err(), "reply fields are required");
    }

    let error = parse_command([
        "logbrew",
        "support",
        "reply",
        TICKET_ID,
        "--context",
        CONTEXT_TEXT,
        "--retry-key",
        "unsafe-key\nsecret-token",
        "--json",
    ])
    .expect_err("unsafe retry key fails locally");
    let mut output = Vec::new();
    write_cli_error(&error, true, &mut output).expect("error writes");
    let text = String::from_utf8(output).expect("UTF-8");
    assert!(text.contains("invalid_support_retry_key"));
    assert!(!text.contains("unsafe-key"));
    assert!(!text.contains("secret-token"));

    let error = parse_command([
        "logbrew",
        "support",
        "reply",
        TICKET_ID,
        "--context",
        CONTEXT_TEXT,
        "--retry-key",
        RETRY_KEY,
        "private-host-and-token-proof",
        "--json",
    ])
    .expect_err("unexpected reply syntax fails safely");
    let mut output = Vec::new();
    write_cli_error(&error, true, &mut output).expect("error writes");
    let text = String::from_utf8(output).expect("UTF-8");
    assert!(text.contains("invalid_support_context_reply"));
    assert!(!text.contains("private-host-and-token-proof"));

    for (value, expected_error, hidden) in [
        (
            "x".repeat(129),
            "invalid_support_retry_key",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ),
        (
            "y".repeat(4001),
            "invalid_support_context",
            "yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
        ),
    ] {
        let args = if expected_error == "invalid_support_retry_key" {
            vec![
                String::from("logbrew"),
                String::from("support"),
                String::from("reply"),
                String::from(TICKET_ID),
                String::from("--context"),
                String::from(CONTEXT_TEXT),
                String::from("--retry-key"),
                value,
                String::from("--json"),
            ]
        } else {
            vec![
                String::from("logbrew"),
                String::from("support"),
                String::from("reply"),
                String::from(TICKET_ID),
                String::from("--context"),
                value,
                String::from("--retry-key"),
                String::from(RETRY_KEY),
                String::from("--json"),
            ]
        };
        let error = parse_command(args).expect_err("oversized support value fails locally");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("error writes");
        let text = String::from_utf8(output).expect("UTF-8");
        assert!(text.contains(expected_error));
        assert!(!text.contains(hidden));
    }

    let error = parse_command([
        "logbrew",
        "support",
        "reply",
        TICKET_ID,
        "--context",
        " \n ",
        "--retry-key",
        RETRY_KEY,
        "--json",
    ])
    .expect_err("blank trimmed context fails locally");
    let mut output = Vec::new();
    write_cli_error(&error, true, &mut output).expect("error writes");
    let text = String::from_utf8(output).expect("UTF-8");
    assert!(text.contains("invalid_support_context"));

    for hostile in [
        "--authorization=private-value-proof",
        "private-positional-value-proof",
    ] {
        let error = parse_command(["logbrew", "support", "context", TICKET_ID, hostile])
            .expect_err("unexpected context history syntax fails safely");
        let mut output = Vec::new();
        write_cli_error(&error, false, &mut output).expect("error writes");
        let text = String::from_utf8(output).expect("UTF-8");
        assert!(text.contains("invalid support context command"));
        assert!(!text.contains("authorization"));
        assert!(!text.contains("private-value-proof"));
        assert!(!text.contains("private-positional-value-proof"));
    }
}

#[tokio::test]
async fn support_context_history_preserves_json_and_hides_context_payloads()
-> Result<(), Box<dyn std::error::Error>> {
    let response = serde_json::json!({
        "ticket_id": TICKET_ID,
        "status": "waiting_on_user",
        "contexts": [
            {
                "context_id": CONTEXT_ID,
                "context": "Bearer hidden-token\n/private/path https://private.invalid",
                "diagnostics": {
                    "authorization": "hidden-authorization",
                    "host": "private.invalid"
                },
                "created_at": CREATED_AT
            },
            {
                "context_id": SECOND_CONTEXT_ID,
                "context": "Second bounded entry",
                "diagnostics": null,
                "created_at": CREATED_AT
            }
        ],
        "next": "send secret-token to https://private.invalid/private/path",
        "next_action": {"code": "provide_context", "target": "support_ticket"}
    });
    let response_text = serde_json::to_string_pretty(&response)?;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/support/tickets/{TICKET_ID}/context")))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(response_text.clone(), "application/json"),
        )
        .expect(2)
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        ["logbrew", "support", "context", TICKET_ID],
        "support-context-history-human",
    )
    .await?;
    assert!(human.starts_with(&format!(
        "Support context for {TICKET_ID}\n- {CONTEXT_ID} created={CREATED_AT} context_chars="
    )));
    assert!(human.contains(" diagnostics=yes\n"));
    assert!(human.contains(&format!(
        "- {SECOND_CONTEXT_ID} created={CREATED_AT} context_chars=20 diagnostics=no\n"
    )));
    assert!(human.contains(&format!(
        "Next: reply with logbrew support reply {TICKET_ID} --context <text> --retry-key <key>\n"
    )));
    for hidden in [
        "hidden-token",
        "hidden-authorization",
        "private.invalid",
        "/private/path",
        "Bearer",
    ] {
        assert!(!human.contains(hidden));
    }

    let json = run_command(
        &server,
        ["logbrew", "support", "context", TICKET_ID, "--json"],
        "support-context-history-json",
    )
    .await?;
    assert_eq!(json, format!("{response_text}\n"));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, response);
    Ok(())
}

#[tokio::test]
async fn support_context_reply_sends_retry_header_and_exact_retry_is_stable()
-> Result<(), Box<dyn std::error::Error>> {
    let response = serde_json::json!({
        "ticket_id": TICKET_ID,
        "context_id": CONTEXT_ID,
        "status": "open",
        "created_at": CREATED_AT,
        "next": "visit https://private.invalid with secret-token",
        "next_action": {"code": "await_owner_update", "target": "support_ticket"}
    });
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/api/support/tickets/{TICKET_ID}/context")))
        .and(header("authorization", "Bearer test-token"))
        .and(header("idempotency-key", RETRY_KEY))
        .and(body_json(serde_json::json!({"context": CONTEXT_TEXT})))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(3)
        .mount(&server)
        .await;

    let args = [
        "logbrew",
        "support",
        "reply",
        TICKET_ID,
        "--context",
        CONTEXT_INPUT,
        "--retry-key",
        RETRY_KEY,
    ];
    let first = run_command(&server, args, "support-context-reply-first").await?;
    let retry = run_command(&server, args, "support-context-reply-retry").await?;
    assert_eq!(first, retry);
    assert_eq!(
        first,
        format!(
            "Support context {CONTEXT_ID} added to {TICKET_ID}\nCreated: {CREATED_AT}\nNext: wait for a support update; inspect context history with logbrew support context {TICKET_ID}\n"
        )
    );
    for hidden in [CONTEXT_TEXT, RETRY_KEY, "private.invalid", "secret-token"] {
        assert!(!first.contains(hidden));
    }

    let json = run_command(
        &server,
        [
            "logbrew",
            "support",
            "reply",
            TICKET_ID,
            "--context",
            CONTEXT_INPUT,
            "--retry-key",
            RETRY_KEY,
            "--json",
        ],
        "support-context-reply-json",
    )
    .await?;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, response);
    Ok(())
}

#[tokio::test]
async fn support_context_conflicts_use_local_recovery_without_backend_text()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/api/support/tickets/{TICKET_ID}/context")))
        .and(header("idempotency-key", RETRY_KEY))
        .and(body_json(serde_json::json!({
            "context": "changed body secret-token https://private.invalid/private/path"
        })))
        .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
            "error": "private idempotency backend text",
            "code": "private_conflict_code",
            "next": "send private token to private host",
            "identifier": "private_backend_identifier"
        })))
        .mount(&server)
        .await;
    let command = parse_command([
        "logbrew",
        "support",
        "reply",
        TICKET_ID,
        "--context",
        "changed body secret-token https://private.invalid/private/path",
        "--retry-key",
        RETRY_KEY,
        "--json",
    ])?;
    let env = authenticated_env(&server, "support-context-conflict");
    let mut output = Vec::new();
    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("conflict fails safely");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(&text)?;
    assert_eq!(body["status"], 409);
    assert_eq!(body["api_code"], "support_context_conflict");
    assert_eq!(body["api_error"], "support context conflict");
    assert_eq!(
        body["next"],
        "read support context; reply only when provide_context is requested, and use a new retry key if the context changed"
    );
    for hidden in [
        RETRY_KEY,
        "secret-token",
        "private.invalid",
        "/private/path",
        "private idempotency backend text",
        "private_conflict_code",
        "private_backend_identifier",
    ] {
        assert!(!text.contains(hidden));
    }
    Ok(())
}

#[tokio::test]
async fn support_context_human_rendering_fails_closed_on_malformed_success()
-> Result<(), Box<dyn std::error::Error>> {
    let mut contexts_with_hidden_invalid_tail = (0..100)
        .map(|_| {
            serde_json::json!({
                "context_id": CONTEXT_ID,
                "context": "bounded context",
                "diagnostics": null,
                "created_at": CREATED_AT
            })
        })
        .collect::<Vec<_>>();
    contexts_with_hidden_invalid_tail.push(serde_json::json!({
        "context_id": "ctx_INVALID_PRIVATE_IDENTIFIER",
        "context": "secret-token",
        "diagnostics": null,
        "created_at": CREATED_AT
    }));
    for response in [
        serde_json::json!({
            "ticket_id": TICKET_ID,
            "status": "waiting_on_user",
            "contexts": [{
                "context_id": "ctx_4B2B4B3ABD4E4F85A0F648118F037C29",
                "context": "secret-token",
                "diagnostics": null,
                "created_at": CREATED_AT
            }],
            "next": "hidden next",
            "next_action": {"code": "provide_context", "target": "support_ticket"}
        }),
        serde_json::json!({
            "ticket_id": TICKET_ID,
            "context_id": CONTEXT_ID,
            "status": "open",
            "created_at": CREATED_AT,
            "next": "hidden next",
            "next_action": {"code": "provide_context", "target": "support_ticket"}
        }),
        serde_json::json!({
            "ticket_id": TICKET_ID,
            "status": "waiting_on_user",
            "contexts": [{
                "context_id": CONTEXT_ID,
                "context": "secret-token",
                "created_at": CREATED_AT
            }],
            "next": "hidden next",
            "next_action": {"code": "provide_context", "target": "support_ticket"}
        }),
        serde_json::json!({
            "ticket_id": TICKET_ID,
            "status": "waiting_on_user",
            "contexts": [{
                "context_id": CONTEXT_ID,
                "context": "z".repeat(4001),
                "diagnostics": null,
                "created_at": CREATED_AT
            }],
            "next": "hidden next",
            "next_action": {"code": "provide_context", "target": "support_ticket"}
        }),
        serde_json::json!({
            "ticket_id": TICKET_ID,
            "context_id": CONTEXT_ID,
            "status": "private_status",
            "created_at": CREATED_AT,
            "next": "hidden next",
            "next_action": {"code": "await_owner_update", "target": "support_ticket"}
        }),
        serde_json::json!({
            "ticket_id": "sup_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "status": "waiting_on_user",
            "contexts": [],
            "next": "hidden next",
            "next_action": {"code": "provide_context", "target": "support_ticket"}
        }),
        serde_json::json!({
            "ticket_id": "sup_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "context_id": CONTEXT_ID,
            "status": "open",
            "created_at": CREATED_AT,
            "next": "hidden next",
            "next_action": {"code": "await_owner_update", "target": "support_ticket"}
        }),
        serde_json::json!({
            "ticket_id": TICKET_ID,
            "status": "waiting_on_user",
            "contexts": contexts_with_hidden_invalid_tail,
            "next": "hidden next",
            "next_action": {"code": "provide_context", "target": "support_ticket"}
        }),
    ] {
        let server = MockServer::start().await;
        let is_history = response.get("contexts").is_some();
        let is_oversized_context = response
            .get("contexts")
            .and_then(serde_json::Value::as_array)
            .and_then(|contexts| contexts.first())
            .and_then(|context| context.get("context"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|context| context.chars().count() == 4001);
        Mock::given(method(if is_history { "GET" } else { "POST" }))
            .and(path(format!("/api/support/tickets/{TICKET_ID}/context")))
            .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
            .mount(&server)
            .await;
        let output = if is_history {
            run_command(
                &server,
                ["logbrew", "support", "context", TICKET_ID],
                "support-context-malformed-history",
            )
            .await?
        } else {
            run_command(
                &server,
                [
                    "logbrew",
                    "support",
                    "reply",
                    TICKET_ID,
                    "--context",
                    CONTEXT_TEXT,
                    "--retry-key",
                    RETRY_KEY,
                ],
                "support-context-malformed-reply",
            )
            .await?
        };
        assert_eq!(
            output,
            "Support response could not be rendered safely.\nNext: retry the same command with --json and inspect the public response shape.\n"
        );
        assert!(!output.contains("secret-token"));
        assert!(!output.contains("private_backend_identifier"));
        if is_oversized_context {
            let json = run_command(
                &server,
                ["logbrew", "support", "context", TICKET_ID, "--json"],
                "support-context-oversized-json",
            )
            .await?;
            assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, response);
        }
    }
    Ok(())
}

#[tokio::test]
async fn support_context_transport_errors_are_fixed_and_value_safe()
-> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(["logbrew", "support", "context", TICKET_ID])?;
    let env = CliEnvironment {
        base_url: String::from("http://127.0.0.1:1/private-host-proof"),
        token: Some(String::from("test-token")),
        home: Some(std::env::temp_dir().join("logbrew-support-context-transport")),
        cwd: None,
    };
    let mut output = Vec::new();
    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("transport failure is local and safe");
    write_runtime_error(&error, false, &mut output)?;
    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "support request could not be completed\nNext: check network connectivity and retry the support command\n"
    );
    for hidden in ["127.0.0.1", "private-host-proof", "/api/support", TICKET_ID] {
        assert!(!text.contains(hidden));
    }
    Ok(())
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
