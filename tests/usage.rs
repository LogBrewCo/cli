//! Account usage command and response-contract tests.

use logbrew_cli::{
    CliEnvironment, CliError, Command, RuntimeError, execute_command, parse_command,
    write_cli_error, write_runtime_error,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn parses_usage_aliases_as_one_authenticated_read() {
    let cases = [
        (["logbrew", "usage"].as_slice(), false),
        (["logbrew", "usage", "--json"].as_slice(), true),
        (["logbrew", "--json", "usage"].as_slice(), true),
        (["logbrew", "account", "usage"].as_slice(), false),
        (["logbrew", "account", "usage", "--json"].as_slice(), true),
    ];

    for (args, json) in cases {
        assert_eq!(
            parse_command(args.iter().copied()).expect("usage command parses"),
            Command::Usage { json }
        );
    }
}

#[test]
fn usage_parser_rejects_closed_grammar_without_reflection() {
    let cases = [
        ["logbrew", "usage", "--hostile-secret"].as_slice(),
        ["logbrew", "usage", "customer-value"].as_slice(),
        ["logbrew", "account", "usage", "--json=true"].as_slice(),
    ];

    for args in cases {
        let error = parse_command(args.iter().copied()).expect_err("usage grammar fails closed");
        assert_eq!(error, CliError::InvalidUsageCommand);
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("error renders");
        let text = String::from_utf8(output).expect("utf8 output");
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("customer-value"));
        assert!(text.contains("invalid_usage_command"));
        assert!(text.contains("use logbrew usage with optional --json"));
    }
}

#[tokio::test]
async fn usage_json_preserves_the_exact_validated_server_object()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let body = usage_response();
    let raw = body.to_string();
    Mock::given(method("GET"))
        .and(path("/api/account/usage"))
        .and(header("authorization", "Bearer account-token"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(raw.clone(), "application/json"))
        .expect(1)
        .mount(&server)
        .await;

    let command = parse_command(["logbrew", "usage", "--json"])?;
    let mut output = Vec::new();
    execute_command(&command, &environment(&server), &mut output).await?;

    assert_eq!(String::from_utf8(output)?, format!("{raw}\n"));
    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.query().is_none());
    assert_eq!(requests[0].body, Vec::<u8>::new());
    Ok(())
}

#[tokio::test]
async fn usage_human_output_is_bounded_and_uses_cli_owned_guidance()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut body = usage_response();
    body["next"] = serde_json::Value::String(String::from("server-owned-next-text"));
    Mock::given(method("GET"))
        .and(path("/api/account/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

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
}

#[tokio::test]
async fn accepts_all_deployed_usage_states_and_action_pairs()
-> Result<(), Box<dyn std::error::Error>> {
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
        let server = MockServer::start().await;
        let mut body = usage_response();
        body["state"] = serde_json::json!(state);
        body["warning"] = serde_json::json!(warning);
        body["blocked"] = serde_json::json!(blocked);
        body["limit"] = serde_json::json!(limit);
        body["next_action"]["code"] = serde_json::json!(code);
        body["next_action"]["target"] = serde_json::json!(target);
        body["next_action"]["state"] = serde_json::json!(state);
        body["next_action"]["limit"] = serde_json::json!(limit);
        Mock::given(method("GET"))
            .and(path("/api/account/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let command = parse_command(["logbrew", "usage", "--json"])?;
        execute_command(&command, &environment(&server), &mut Vec::new()).await?;
    }
    Ok(())
}

#[tokio::test]
async fn malformed_or_mismatched_usage_success_fails_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let mut cases = Vec::new();

    let mut extra = usage_response();
    extra["private_customer"] = serde_json::Value::String(String::from("hidden-value"));
    cases.push(extra);

    let mut bad_percent = usage_response();
    bad_percent["percent_used"] = serde_json::json!(100.1);
    cases.push(bad_percent);

    let mut mismatched_state = usage_response();
    mismatched_state["next_action"]["state"] = serde_json::json!("blocked");
    cases.push(mismatched_state);

    let mut mismatched_limit = usage_response();
    mismatched_limit["next_action"]["limit"] = serde_json::json!("bytes");
    cases.push(mismatched_limit);

    let mut mismatched_reset = usage_response();
    mismatched_reset["next_action"]["reset_at"] = serde_json::json!("2026-09-01T00:00:00Z");
    cases.push(mismatched_reset);

    let mut invalid_action = usage_response();
    invalid_action["next_action"]["target"] = serde_json::json!("pricing");
    cases.push(invalid_action);

    let mut unknown_action = usage_response();
    unknown_action["next_action"]["code"] = serde_json::json!("private_action");
    cases.push(unknown_action);

    let mut invalid_uuid = usage_response();
    invalid_uuid["by_project"][0]["project_id"] = serde_json::json!("private-customer-id");
    cases.push(invalid_uuid);

    let mut invalid_time = usage_response();
    invalid_time["period_start"] = serde_json::json!("not-a-time");
    cases.push(invalid_time);

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
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/account/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "usage", "--json"])?;
        let error = execute_command(&command, &environment(&server), &mut Vec::new())
            .await
            .expect_err("invalid success fails closed");
        assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;
    }
    Ok(())
}

#[tokio::test]
async fn nonfinite_usage_numbers_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let raw = usage_response()
        .to_string()
        .replace("\"warning_threshold\":80.0", "\"warning_threshold\":1e400");
    Mock::given(method("GET"))
        .and(path("/api/account/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(raw, "application/json"))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = execute_command(&command, &environment(&server), &mut Vec::new())
        .await
        .expect_err("nonfinite number fails closed");
    assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;
    Ok(())
}

#[tokio::test]
async fn typed_usage_errors_are_fixed_and_never_reflect_backend_text()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            401,
            "unauthorized",
            "unauthorized",
            "sign_in",
            "auth",
            "run logbrew login",
        ),
        (
            405,
            "method_not_allowed",
            "method_not_allowed",
            "use_supported_method",
            "api_method",
            "retry logbrew usage with the supported GET request",
        ),
        (
            500,
            "internal_error",
            "server_error",
            "retry",
            "request",
            "retry logbrew usage later",
        ),
    ];

    for (status, response_code, expected_code, action, target, expected_next) in cases {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "error": "private-host-secret-body",
            "code": response_code,
            "next": "send token to private path",
            "next_action": {"code": action, "target": target}
        });
        Mock::given(method("GET"))
            .and(path("/api/account/usage"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "usage", "--json"])?;
        let error = execute_command(&command, &environment(&server), &mut Vec::new())
            .await
            .expect_err("typed error returned");
        assert_safe_usage_error(&error, expected_code, expected_next)?;
    }
    Ok(())
}

#[tokio::test]
async fn malformed_typed_errors_and_missing_auth_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/account/usage"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "private-host-secret-body",
            "code": "unauthorized",
            "next": "send token",
            "next_action": {"code": "retry", "target": "request"}
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = execute_command(&command, &environment(&server), &mut Vec::new())
        .await
        .expect_err("mismatched 401 fails closed");
    assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;

    let malformed_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/account/usage"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
            "error": "private-host-secret-body",
            "code": "undeployed_error",
            "next": "send token",
            "next_action": {"code": "sign_in", "target": "auth"}
        })))
        .mount(&malformed_server)
        .await;
    let error = execute_command(&command, &environment(&malformed_server), &mut Vec::new())
        .await
        .expect_err("mismatched 500 fails closed");
    assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;

    let no_auth_server = MockServer::start().await;
    let no_auth = CliEnvironment {
        base_url: no_auth_server.uri(),
        token: None,
        home: None,
        cwd: None,
    };
    let error = execute_command(&command, &no_auth, &mut Vec::new())
        .await
        .expect_err("missing auth fails locally");
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    assert!(text.contains("not_logged_in"));
    assert!(text.contains("run logbrew login"));
    assert!(
        no_auth_server
            .received_requests()
            .await
            .expect("request recording")
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn oversized_usage_response_fails_before_body_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let oversized = format!("{{\"private_secret\":\"{}\"}}", "x".repeat(300_000));
    Mock::given(method("GET"))
        .and(path("/api/account/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(oversized, "application/json"))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = execute_command(&command, &environment(&server), &mut Vec::new())
        .await
        .expect_err("oversized response fails closed");

    assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;
    Ok(())
}

#[tokio::test]
async fn duplicate_usage_keys_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let valid = usage_response().to_string();
    let duplicate_success = format!(
        "{{\"period_start\":\"private-host-secret-body\",{}",
        valid.strip_prefix('{').expect("usage object")
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
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/account/usage"))
            .respond_with(ResponseTemplate::new(status).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "usage", "--json"])?;
        let error = execute_command(&command, &environment(&server), &mut Vec::new())
            .await
            .expect_err("duplicate keys fail closed");
        assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;
    }
    Ok(())
}

#[tokio::test]
async fn usage_does_not_follow_redirects() -> Result<(), Box<dyn std::error::Error>> {
    let redirect_target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/capture"))
        .respond_with(ResponseTemplate::new(200).set_body_json(usage_response()))
        .mount(&redirect_target)
        .await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/account/usage"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/capture", redirect_target.uri())),
        )
        .mount(&server)
        .await;

    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = execute_command(&command, &environment(&server), &mut Vec::new())
        .await
        .expect_err("redirect is rejected");
    assert_safe_usage_error(&error, "usage response is invalid", "retry logbrew usage")?;
    assert!(
        redirect_target
            .received_requests()
            .await
            .expect("request recording")
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn usage_rejects_non_root_api_origins_before_io() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/private-prefix/api/account/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(usage_response()))
        .mount(&server)
        .await;
    let environment = CliEnvironment {
        base_url: format!("{}/private-prefix", server.uri()),
        token: Some(String::from("account-token")),
        home: None,
        cwd: None,
    };

    let command = parse_command(["logbrew", "usage", "--json"])?;
    let error = execute_command(&command, &environment, &mut Vec::new())
        .await
        .expect_err("path-bearing origin fails closed");
    assert_safe_usage_error(
        &error,
        "usage request could not be completed",
        "check network connectivity",
    )?;
    assert!(
        server
            .received_requests()
            .await
            .expect("request recording")
            .is_empty()
    );
    Ok(())
}

fn environment(server: &MockServer) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some(String::from("account-token")),
        home: None,
        cwd: None,
    }
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
) -> Result<(), Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    write_runtime_error(error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    assert!(text.contains(expected), "unexpected error: {text}");
    assert!(text.contains(expected_next), "unexpected recovery: {text}");
    assert!(!text.contains("private-host-secret-body"));
    assert!(!text.contains("private_secret"));
    assert!(!text.contains("send token"));
    assert!(!text.contains("account-token"));
    Ok(())
}
