//! Account usage command and response-contract tests.

use crate::{
    Mock, MockServer, ResponseTemplate, TestResult, execute_command, failed_command, render_error,
    runtime_error_text,
};
use logbrew_cli::{CliEnvironment, CliError, Command, RuntimeError, parse_command};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn parses_usage_aliases_as_one_authenticated_read() -> TestResult {
    let cases = [
        (["logbrew", "usage"].as_slice(), false),
        (["logbrew", "usage", "--json"].as_slice(), true),
        (["logbrew", "--json", "usage"].as_slice(), true),
        (["logbrew", "account", "usage"].as_slice(), false),
        (["logbrew", "account", "usage", "--json"].as_slice(), true),
    ];

    for (args, json) in cases {
        assert_eq!(
            parse_command(args.iter().copied())?,
            Command::Usage { json }
        );
    }
    Ok(())
}

#[test]
fn usage_parser_rejects_closed_grammar_without_reflection() -> TestResult {
    let cases = [
        ["logbrew", "usage", "--hostile-secret"].as_slice(),
        ["logbrew", "usage", "customer-value"].as_slice(),
        ["logbrew", "account", "usage", "--json=true"].as_slice(),
    ];

    for args in cases {
        assert_eq!(
            parse_command(args.iter().copied()),
            Err(CliError::InvalidUsageCommand)
        );
        let text = render_error(args, true)?;
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("customer-value"));
        assert!(text.contains("invalid_usage_command"));
        assert!(text.contains("use logbrew usage with optional --json"));
    }
    Ok(())
}

async_test!(usage_json_preserves_the_exact_validated_server_object -> TestResult, {
    let server = MockServer::start().await?;
    let body = usage_response();
    let raw = body.to_string();
    Mock::auth("GET", "/api/account/usage", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_raw(raw.clone(), "application/json"))
        .expect(1)
        .mount(&server);

    let command = parse_command(["logbrew", "usage", "--json"])?;
    let mut output = Vec::new();
    execute_command(&command, &environment(&server), &mut output).await?;

    assert_eq!(String::from_utf8(output)?, format!("{raw}\n"));
    let requests = server.received_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.query().is_none());
    assert_eq!(requests[0].body, Vec::<u8>::new());
    Ok(())
});

async_test!(usage_human_output_is_bounded_and_uses_cli_owned_guidance -> TestResult, {
    let server = MockServer::start().await?;
    let mut body = usage_response();
    body["next"] = serde_json::Value::String(String::from("server-owned-next-text"));
    Mock::route("GET", "/api/account/usage")
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server);

    let command = parse_command(["logbrew", "account", "usage"])?;
    let mut output = Vec::new();
    execute_command(&command, &environment(&server), &mut output).await?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("Plan: starter (free)"));
    assert!(text.contains("State: warning"));
    assert!(text.contains("Period: 2026-07-01T00:00:00Z to 2026-08-01T00:00:00Z"));
    assert!(text.contains("Reset: 2026-08-01T00:00:00Z"));
    assert!(text.contains("Events: 900 / 1000"));
    assert!(text.contains("Bytes: 4096 / unlimited"));
    assert!(text.contains("Projects: 2 / 3"));
    assert!(text.contains("Driving limit: events"));
    assert!(text.contains("Next: reduce telemetry usage or review account options"));
    assert!(!text.contains(PROJECT_ID));
    assert!(!text.contains("server-owned-next-text"));
    assert!(!text.contains("logs"));
    assert!(text.lines().count() <= 9);
    Ok(())
});

async_test!(accepts_all_deployed_usage_states_and_action_pairs -> TestResult, {
    let cases = [
        (
            "ok",
            false,
            false,
            None,
            "continue_sending_telemetry",
            "telemetry_ingest",
        ),
        (
            "warning",
            true,
            false,
            Some("events"),
            "reduce_usage_or_upgrade",
            "account_usage",
        ),
        (
            "warning",
            true,
            false,
            Some("projects"),
            "archive_project_or_upgrade",
            "projects",
        ),
        (
            "blocked",
            true,
            true,
            Some("bytes"),
            "wait_until_reset_or_upgrade",
            "pricing",
        ),
        (
            "ok",
            false,
            false,
            None,
            "check_usage_limits",
            "account_usage",
        ),
    ];

    for (state, warning, blocked, limit, code, target) in cases {
        let server = MockServer::start().await?;
        let mut body = usage_response();
        body["state"] = serde_json::json!(state);
        body["warning"] = serde_json::json!(warning);
        body["blocked"] = serde_json::json!(blocked);
        body["limit"] = serde_json::json!(limit);
        body["next_action"]["code"] = serde_json::json!(code);
        body["next_action"]["target"] = serde_json::json!(target);
        body["next_action"]["state"] = serde_json::json!(state);
        body["next_action"]["limit"] = serde_json::json!(limit);
        Mock::route("GET", "/api/account/usage")
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server);

        let command = parse_command(["logbrew", "usage", "--json"])?;
        execute_command(&command, &environment(&server), &mut Vec::new()).await?;
    }
    Ok(())
});

async_test!(malformed_or_mismatched_usage_success_fails_closed -> TestResult, {
    let mut cases = Vec::new();

    let mut extra = usage_response();
    extra["private_customer"] = serde_json::Value::String(String::from("hidden-value"));
    cases.push(extra);

    for (pointer, value) in [
        ("/percent_used", serde_json::json!(100.1)),
        ("/next_action/state", serde_json::json!("blocked")),
        ("/next_action/limit", serde_json::json!("bytes")),
        ("/next_action/reset_at", serde_json::json!("2026-09-01T00:00:00Z")),
        ("/next_action/target", serde_json::json!("pricing")),
        ("/next_action/code", serde_json::json!("private_action")),
        ("/by_project/0/project_id", serde_json::json!("private-customer-id")),
        ("/period_start", serde_json::json!("not-a-time")),
    ] {
        let mut body = usage_response();
        *body.pointer_mut(pointer).ok_or("fixture field must exist")? = value;
        cases.push(body);
    }

    let mut contradictory_flags = usage_response();
    contradictory_flags["state"] = serde_json::json!("ok");
    contradictory_flags["warning"] = serde_json::json!(true);
    contradictory_flags["blocked"] = serde_json::json!(true);
    contradictory_flags["next_action"]["state"] = serde_json::json!("ok");
    cases.push(contradictory_flags);

    let mut mismatched_action_state = usage_response();
    mismatched_action_state["next_action"]["code"] =
        serde_json::json!("continue_sending_telemetry");
    mismatched_action_state["next_action"]["target"] = serde_json::json!("telemetry_ingest");
    cases.push(mismatched_action_state);

    for body in cases {
        assert_invalid_usage_response(200, &body.to_string()).await?;
    }
    Ok(())
});

async_test!(nonfinite_usage_numbers_fail_closed -> TestResult, {
    let raw = usage_response()
        .to_string()
        .replace("\"warning_threshold\":80.0", "\"warning_threshold\":1e400");
    assert_invalid_usage_response(200, &raw).await?;
    Ok(())
});

async_test!(malformed_typed_errors_and_missing_auth_fail_closed -> TestResult, {
    for (status, code, action, target) in [
        (401, "unauthorized", "retry", "request"),
        (500, "undeployed_error", "sign_in", "auth"),
    ] {
        let body = serde_json::json!({
            "error": "private-host-secret-body",
            "code": code,
            "next": "send token",
            "next_action": {"code": action, "target": target}
        });
        assert_invalid_usage_response(status, &body.to_string()).await?;
    }

    let no_auth_server = MockServer::start().await?;
    let no_auth = super::test_env(&no_auth_server, None, None);
    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = failed_command(&command, &no_auth).await?;
    let text = runtime_error_text(&error)?;
    assert!(text.contains("not_logged_in"));
    assert!(text.contains("run logbrew login"));
    assert!(no_auth_server.received_requests().is_empty());
    Ok(())
});

async_test!(oversized_usage_response_fails_before_body_reflection -> TestResult, {
    let oversized = format!("{{\"private_secret\":\"{}\"}}", "x".repeat(300_000));
    assert_invalid_usage_response(200, &oversized).await?;
    Ok(())
});

async_test!(duplicate_usage_keys_fail_closed_without_reflection -> TestResult, {
    let valid = usage_response().to_string();
    let duplicate_success = format!(
        "{{\"period_start\":\"private-host-secret-body\",{}",
        valid.strip_prefix('{').ok_or("usage object")?
    );
    let duplicate_nested = valid.replacen(
        "\"plan\":{",
        "\"plan\":{\"tier\":\"private-host-secret-body\",",
        1,
    );
    let duplicate_error = String::from(concat!(
        "{\"error\":\"private-host-secret-body\",",
        "\"error\":\"unauthorized\",\"code\":\"unauthorized\",",
        "\"next\":\"sign in again\",",
        "\"next_action\":{\"code\":\"sign_in\",\"target\":\"auth\"}}"
    ));

    for (status, body) in [
        (200, duplicate_success),
        (200, duplicate_nested),
        (401, duplicate_error),
    ] {
        assert_invalid_usage_response(status, &body).await?;
    }
    Ok(())
});

async_test!(usage_does_not_follow_redirects -> TestResult, {
    let redirect_target = MockServer::start().await?;
    Mock::route("GET", "/capture")
        .respond_with(ResponseTemplate::new(200).set_body_json(&usage_response()))
        .mount(&redirect_target);
    let server = MockServer::start().await?;
    Mock::route("GET", "/api/account/usage")
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/capture", redirect_target.uri())),
        )
        .mount(&server);

    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = failed_command(&command, &environment(&server)).await?;
    assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;
    assert!(redirect_target.received_requests().is_empty());
    Ok(())
});

async_test!(usage_rejects_non_root_api_origins_before_io -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/private-prefix/api/account/usage")
        .respond_with(ResponseTemplate::new(200).set_body_json(&usage_response()))
        .mount(&server);
    let environment = CliEnvironment {
        base_url: format!("{}/private-prefix", server.uri()),
        token: Some(String::from("account-token")),
        home: None,
        cwd: None,
    };

    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = failed_command(&command, &environment).await?;
    assert_safe_usage_error(
        &error,
        "usage request could not be completed",
        "check network connectivity",
    )?;
    assert!(server.received_requests().is_empty());
    Ok(())
});

/// Verifies an invalid response through the authenticated HTTP and JSON boundaries.
async fn assert_invalid_usage_response(status: u16, body: &str) -> TestResult {
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/account/usage", "account-token")
        .respond_with(ResponseTemplate::new(status).set_body_raw(body, "application/json"))
        .expect(1)
        .mount(&server);
    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = failed_command(&command, &environment(&server)).await?;
    assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")
}

fn environment(server: &MockServer) -> CliEnvironment {
    super::authenticated_env(server, "account-token", None)
}

fn usage_response() -> serde_json::Value {
    serde_json::json!({
        "period_start": "2026-07-01T00:00:00Z",
        "period_end": "2026-08-01T00:00:00Z",
        "reset_at": "2026-08-01T00:00:00Z",
        "plan": {"tier": "starter", "status": "free"},
        "limits": {"events": 1000, "bytes": null, "projects": 3, "retention_days": 30},
        "usage": {"events": 900, "bytes": 4096, "projects": 2},
        "state": "warning",
        "warning_threshold": 80.0,
        "percent_used": 90.0,
        "warning": true,
        "blocked": false,
        "limit": "events",
        "next": "reduce usage",
        "next_action": {
            "code": "reduce_usage_or_upgrade",
            "target": "account_usage",
            "state": "warning",
            "limit": "events",
            "reset_at": "2026-08-01T00:00:00Z"
        },
        "by_project": [{"project_id": PROJECT_ID, "events": 700, "bytes": 3072}],
        "by_stream": [{"kind": "logs", "events": 900, "bytes": 4096}]
    })
}

fn assert_safe_usage_error(
    error: &RuntimeError,
    expected: &str,
    expected_next: &str,
) -> TestResult {
    let text = runtime_error_text(error)?;
    assert!(text.contains(expected), "unexpected error: {text}");
    assert!(text.contains(expected_next), "unexpected recovery: {text}");
    for forbidden in [
        "private-host-secret-body",
        "private_secret",
        "send token",
        "account-token",
    ] {
        assert!(
            !text.contains(forbidden),
            "usage errors must not echo private response data"
        );
    }
    Ok(())
}
