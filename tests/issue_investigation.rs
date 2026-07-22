//! Server-directed, read-only issue investigation contracts.

use logbrew_cli::{
    CliEnvironment, execute_command, parse_command, write_cli_error, write_runtime_error,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ISSUE_ID: &str = "11111111-1111-4111-8111-111111111111";
const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn parses_only_the_explicit_issue_investigation_grammar() {
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])
        .expect("explicit issue investigation parses");

    assert!(command.wants_json());
    assert_eq!(command.http_path(), None);
    assert_eq!(command.http_method(), None);
}

#[test]
fn investigation_grammar_failures_are_fixed_and_value_safe()
-> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "investigate"],
        vec!["logbrew", "investigate", "trace", "trace_123"],
        vec![
            "logbrew",
            "investigate",
            "issue",
            ISSUE_ID,
            "--authorization=hostile-secret",
        ],
        vec![
            "logbrew",
            "investigate",
            "issue",
            ISSUE_ID,
            "hostile-secret\ncontrol",
        ],
        vec![
            "logbrew",
            "investigate",
            "issue",
            "11111111-1111-4111-8111-11111111111\n",
        ],
        vec!["logbrew", "investigate", "issue", "issue_123"],
        vec![
            "logbrew",
            "investigate",
            "issue",
            "11111111-1111-4111-8111-11111111111A",
        ],
        vec![
            "logbrew",
            "investigate",
            "issue",
            "11111111-1111-4111-8111-111111111111/path",
        ],
    ] {
        let error = parse_command(args).expect_err("closed investigation grammar rejects input");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["error"], "invalid_investigation_command");
        assert_eq!(
            body["next"],
            "use logbrew investigate issue <issue_id> with optional --json"
        );
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("authorization"));
        assert!(!text.contains("control"));
    }
    Ok(())
}

#[test]
fn investigation_help_describes_the_read_only_server_directed_flow() {
    let command = parse_command(["logbrew", "investigate", "issue", "--help"])
        .expect("investigation help parses");
    let logbrew_cli::Command::Help { topic, .. } = command else {
        panic!("investigation help should return help");
    };
    let text = logbrew_cli::help::help_text(topic);

    assert!(text.contains("logbrew investigate issue <issue_id>"));
    assert!(text.contains("read-only"));
    assert!(text.contains("JSON preserves the issue and exactly one directed result"));
}

#[tokio::test]
async fn investigation_fails_closed_on_an_unknown_server_action_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut issue = issue_detail("inspect_related_logs", "telemetry_logs", None);
    issue["title"] = serde_json::json!("hostile-secret");
    issue["next_action"] = serde_json::json!({
        "code": "open_private_dashboard",
        "target": "internal_url"
    });
    Mock::given(method("GET"))
        .and(path(format!("/api/telemetry/issues/{ISSUE_ID}")))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue))
        .expect(1)
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let env = authenticated_env(&server);
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("unknown server action fails closed");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["error"], "investigation_response_invalid");
    assert_eq!(
        body["next"],
        "retry the issue investigation; if it repeats, report the public response contract"
    );
    assert!(!text.contains("hostile-secret"));
    assert!(!text.contains("open_private_dashboard"));
    assert!(!text.contains("internal_url"));
    Ok(())
}

#[tokio::test]
async fn trace_investigation_preserves_exact_json_and_canonical_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let issue = issue_detail(
        "inspect_trace",
        "trace_summary",
        Some("trace/checkout value"),
    );
    let summary = trace_summary("trace/checkout value");
    mount_issue(&server, issue.clone()).await;
    Mock::given(method("GET"))
        .and(path(
            "/api/telemetry/traces/trace%2Fcheckout%20value/summary",
        ))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(summary.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let output = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(output.as_str())?;

    assert_eq!(
        body,
        serde_json::json!({
            "issue": issue,
            "next_action": {"code": "inspect_trace", "target": "trace_summary"},
            "trace_summary": summary,
            "related_logs": null
        })
    );
    assert_follow_request(
        &server,
        "/api/telemetry/traces/trace%2Fcheckout%20value/summary",
        "project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn trace_id_dot_segments_fail_closed_before_following()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_issue(
        &server,
        issue_detail("inspect_trace", "trace_summary", Some("..")),
    )
    .await;
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let mut output = Vec::new();
    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("URL dot segment fails closed");
    write_runtime_error(&error, true, &mut output)?;
    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;

    assert_eq!(body["error"], "investigation_response_invalid");
    let requests = server
        .received_requests()
        .await
        .ok_or("wiremock request recording is enabled")?;
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn trace_investigation_omits_blank_optional_release_and_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut issue = issue_detail("inspect_trace", "trace_summary", Some("trace_123"));
    issue["release"] = serde_json::json!("");
    issue["environment"] = serde_json::json!("");
    mount_issue(&server, issue).await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/traces/trace_123/summary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(trace_summary("trace_123")))
        .expect(1)
        .mount(&server)
        .await;

    drop(run(&server, true).await?);
    assert_follow_request(
        &server,
        "/api/telemetry/traces/trace_123/summary",
        "project_id=123e4567-e89b-12d3-a456-426614174000",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn related_log_investigation_preserves_scope_and_bare_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let issue = issue_detail("inspect_related_logs", "telemetry_logs", None);
    let logs = serde_json::json!([{
        "id": "log_123",
        "severity": "error",
        "message": "hostile-secret",
        "service_name": "checkout-api",
        "release": "checkout@1.2.3",
        "environment": "production",
        "timestamp": "2026-07-15T09:45:00Z"
    }]);
    mount_issue(&server, issue.clone()).await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(logs.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let output = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(output.as_str())?;

    assert_eq!(
        body,
        serde_json::json!({
            "issue": issue,
            "next_action": {"code": "inspect_related_logs", "target": "telemetry_logs"},
            "trace_summary": null,
            "related_logs": logs
        })
    );
    assert_follow_request(
        &server,
        "/api/logs",
        "project_id=123e4567-e89b-12d3-a456-426614174000&service_name=checkout-api&release=checkout%401.2.3&environment=production&since=2026-07-15T09%3A00%3A00Z&limit=25",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn human_investigation_is_bounded_and_hides_raw_context()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_issue(
        &server,
        issue_detail("inspect_related_logs", "telemetry_logs", None),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "severity": "error",
                "message": "hostile-secret",
                "attributes": {"cookie": "hostile-cookie"},
                "service_name": "checkout-api",
                "release": "checkout@1.2.3",
                "environment": "production",
                "timestamp": "2026-07-15T09:45:00Z"
            }])),
        )
        .mount(&server)
        .await;

    let output = run(&server, false).await?;

    assert_eq!(
        output,
        "Issue investigation\nRoute: inspect_related_logs -> telemetry_logs\nRelated logs: 1 (first page)\nNext: rerun this command with --json to inspect full public log fields.\n"
    );
    assert!(!output.contains(ISSUE_ID));
    assert!(!output.contains(PROJECT_ID));
    assert!(!output.contains("checkout-api"));
    assert!(!output.contains("hostile-secret"));
    assert!(!output.contains("hostile-cookie"));
    Ok(())
}

#[tokio::test]
async fn human_trace_investigation_hides_names_and_raw_context()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_issue(
        &server,
        issue_detail("inspect_trace", "trace_summary", Some("trace_123")),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/traces/trace_123/summary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(trace_summary("trace_123")))
        .mount(&server)
        .await;

    let output = run(&server, false).await?;

    assert_eq!(
        output,
        "Issue investigation\nRoute: inspect_trace -> trace_summary\nTrace summary: spans=12 errors=2 services=3 duration=845ms\nNext: rerun this command with --json to inspect full public trace fields.\n"
    );
    assert!(!output.contains(ISSUE_ID));
    assert!(!output.contains(PROJECT_ID));
    assert!(!output.contains("checkout-api"));
    assert!(!output.contains("hostile-secret"));
    assert!(!output.contains("hostile-bearer"));
    Ok(())
}

#[tokio::test]
async fn related_logs_require_complete_incident_scope_before_following()
-> Result<(), Box<dyn std::error::Error>> {
    for key in ["service_name", "release", "environment"] {
        let server = MockServer::start().await;
        let mut issue = issue_detail("inspect_related_logs", "telemetry_logs", None);
        issue[key] = serde_json::json!("");
        mount_issue(&server, issue).await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();
        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("incomplete log scope fails closed");
        write_runtime_error(&error, true, &mut output)?;
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output)?["error"],
            "investigation_response_invalid"
        );
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_missing_required_scope_before_following()
-> Result<(), Box<dyn std::error::Error>> {
    for (mut issue, missing) in [
        (
            issue_detail("inspect_trace", "trace_summary", Some("")),
            "trace_id",
        ),
        (
            issue_detail("inspect_related_logs", "telemetry_logs", None),
            "project_id",
        ),
        (
            issue_detail("inspect_related_logs", "telemetry_logs", None),
            "first_seen_at",
        ),
    ] {
        if missing != "trace_id" {
            drop(
                issue
                    .as_object_mut()
                    .expect("issue fixture is an object")
                    .remove(missing),
            );
        }
        let server = MockServer::start().await;
        mount_issue(&server, issue).await;

        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();
        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("missing scope fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["error"], "investigation_response_invalid");
        assert!(!text.contains(missing));
    }
    Ok(())
}

#[tokio::test]
async fn investigation_binds_issue_and_trace_response_identity()
-> Result<(), Box<dyn std::error::Error>> {
    for mismatch in ["issue", "trace"] {
        let server = MockServer::start().await;
        let mut issue = issue_detail("inspect_trace", "trace_summary", Some("trace_123"));
        if mismatch == "issue" {
            issue["id"] = serde_json::json!("22222222-2222-4222-8222-222222222222");
        }
        mount_issue(&server, issue).await;
        if mismatch == "trace" {
            Mock::given(method("GET"))
                .and(path("/api/telemetry/traces/trace_123/summary"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(trace_summary("trace_other")),
                )
                .mount(&server)
                .await;
        }
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("response identity mismatch fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;

        assert_eq!(body["error"], "investigation_response_invalid");
    }
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_cross_scope_or_malformed_follow_up_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            issue_detail("inspect_trace", "trace_summary", Some("trace_123")),
            "/api/telemetry/traces/trace_123/summary",
            {
                let mut summary = trace_summary("trace_123");
                summary["unexpected"] = serde_json::json!(true);
                summary
            },
        ),
        (
            issue_detail("inspect_related_logs", "telemetry_logs", None),
            "/api/logs",
            serde_json::json!(["malformed log row"]),
        ),
        (
            issue_detail("inspect_related_logs", "telemetry_logs", None),
            "/api/logs",
            serde_json::json!([{
                "project_id": "99999999-9999-4999-8999-999999999999",
                "service_name": "other-service",
                "release": "checkout@1.2.3",
                "environment": "production",
                "timestamp": "2026-07-15T09:45:00Z"
            }]),
        ),
        (
            issue_detail("inspect_related_logs", "telemetry_logs", None),
            "/api/logs",
            serde_json::json!({
                "logs": [],
                "next_cursor": {"time": "not-rfc3339", "id": "not-a-uuid"}
            }),
        ),
        (
            issue_detail("inspect_related_logs", "telemetry_logs", None),
            "/api/logs",
            serde_json::json!({"logs": [], "unexpected": true}),
        ),
    ];
    for (issue, follow_path, response) in cases {
        let server = MockServer::start().await;
        mount_issue(&server, issue).await;
        Mock::given(method("GET"))
            .and(path(follow_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("cross-scope or malformed follow-up fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;

        assert_eq!(body["error"], "investigation_response_invalid");
    }
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_directional_scope_controls_before_following()
-> Result<(), Box<dyn std::error::Error>> {
    for unsafe_scope in [
        "checkout\u{202e}ipa",
        "checkout\u{2028}api",
        "checkout\u{2066}api",
    ] {
        let server = MockServer::start().await;
        let mut issue = issue_detail("inspect_related_logs", "telemetry_logs", None);
        issue["service_name"] = serde_json::json!(unsafe_scope);
        mount_issue(&server, issue).await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("directional scope control fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
        let text = String::from_utf8(output)?;

        assert_eq!(body["error"], "investigation_response_invalid");
        assert!(!text.contains("checkout"));
        assert!(!text.contains(unsafe_scope));
        let requests = server
            .received_requests()
            .await
            .ok_or("wiremock request recording is enabled")?;
        assert_eq!(requests.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_a_contradictory_related_log_route_before_following()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_issue(
        &server,
        issue_detail("inspect_related_logs", "telemetry_logs", Some("trace_123")),
    )
    .await;
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let mut output = Vec::new();

    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("contradictory route fails closed");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("investigation_response_invalid"));
    assert!(!text.contains("trace_123"));
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_an_unsafe_api_origin_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let env = CliEnvironment {
        base_url: "https://user:hostile-secret@example.invalid/private-path?token=value".to_owned(),
        token: Some("test-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-investigation-origin-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("unsafe origin fails closed");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("unavailable"));
    assert!(!text.contains("hostile-secret"));
    assert!(!text.contains("example.invalid"));
    assert!(!text.contains("private-path"));
    assert!(!text.contains("token=value"));
    Ok(())
}

#[tokio::test]
async fn follow_up_failures_discard_backend_text_and_malformed_success()
-> Result<(), Box<dyn std::error::Error>> {
    for response in [
        ResponseTemplate::new(500).set_body_string(
            "hostile-secret https://private.example/path Authorization: bearer-value",
        ),
        ResponseTemplate::new(200).set_body_string("hostile-secret not-json"),
    ] {
        let server = MockServer::start().await;
        mount_issue(
            &server,
            issue_detail("inspect_related_logs", "telemetry_logs", None),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/api/logs"))
            .respond_with(response)
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("unsafe follow-up response fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;

        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("private.example"));
        assert!(!text.contains("bearer-value"));
        assert!(!text.contains(server.uri().as_str()));
    }
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_extra_or_duplicate_issue_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let serialized = serde_json::to_string(&issue_detail(
        "inspect_related_logs",
        "telemetry_logs",
        None,
    ))?;
    let duplicate = format!(
        "{{\"id\":\"{ISSUE_ID}\",{}",
        serialized.strip_prefix('{').expect("fixture is an object")
    );
    let mut extra = issue_detail("inspect_related_logs", "telemetry_logs", None);
    extra["private_url"] = serde_json::json!("https://hostile.example/private");

    for body in [
        ResponseTemplate::new(200).set_body_string(duplicate),
        ResponseTemplate::new(200).set_body_json(extra),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/api/telemetry/issues/{ISSUE_ID}")))
            .respond_with(body)
            .expect(1)
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();
        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("non-exact issue response fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;

        assert!(text.contains("investigation_response_invalid"));
        assert!(!text.contains("hostile.example"));
    }
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_redirects_without_forwarding_auth()
-> Result<(), Box<dyn std::error::Error>> {
    let destination = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_detail(
            "inspect_related_logs",
            "telemetry_logs",
            None,
        )))
        .expect(0)
        .mount(&destination)
        .await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/telemetry/issues/{ISSUE_ID}")))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/redirected", destination.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;

    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let mut output = Vec::new();
    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("redirect fails closed");
    write_runtime_error(&error, true, &mut output)?;

    assert!(String::from_utf8(output)?.contains("request_failed"));
    destination.verify().await;
    Ok(())
}

#[tokio::test]
async fn investigation_rejects_oversized_success_without_rendering_it()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_issue(
        &server,
        issue_detail("inspect_related_logs", "telemetry_logs", None),
    )
    .await;
    let oversized = serde_json::Value::Array(
        (0..20_000)
            .map(|_| serde_json::json!({"message": "hostile-secret-padding"}))
            .collect(),
    );
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(oversized))
        .expect(1)
        .mount(&server)
        .await;

    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let mut output = Vec::new();
    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("oversized success fails closed");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("investigation_response_invalid"));
    assert!(!text.contains("hostile-secret-padding"));
    Ok(())
}

#[tokio::test]
async fn investigation_preserves_typed_status_for_an_oversized_error_body()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/telemetry/issues/{ISSUE_ID}")))
        .respond_with(ResponseTemplate::new(500).set_body_string("hostile-secret".repeat(30_000)))
        .expect(1)
        .mount(&server)
        .await;

    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let mut output = Vec::new();
    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("oversized server error remains typed");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["api_code"], "service_unavailable");
    assert!(!text.contains("hostile-secret"));
    Ok(())
}

#[tokio::test]
async fn investigation_status_recovery_is_fixed_and_value_safe()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, expected_code, action_code, action_target) in [
        (400, "validation_failed", "fix_request", "request"),
        (401, "unauthorized", "sign_in", "auth"),
        (404, "not_found", "check_resource", "resource"),
        (
            405,
            "method_not_allowed",
            "use_supported_method",
            "api_method",
        ),
        (422, "validation_failed", "fix_request", "request"),
        (429, "rate_limited", "retry_later", "issue"),
        (500, "service_unavailable", "retry_later", "issue"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/api/telemetry/issues/{ISSUE_ID}")))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_string("hostile-secret https://private.example/path bearer-value"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();
        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("typed status fails safely");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;
        let safe_api_body: serde_json::Value = serde_json::from_str(
            body["body"]
                .as_str()
                .ok_or("safe API body is a JSON string")?,
        )?;

        assert_eq!(body["api_code"], expected_code);
        assert_eq!(safe_api_body["next_action"]["code"], action_code);
        assert_eq!(safe_api_body["next_action"]["target"], action_target);
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("private.example"));
        assert!(!text.contains("bearer-value"));
    }
    Ok(())
}

fn issue_detail(code: &str, target: &str, trace_id: Option<&str>) -> serde_json::Value {
    let mut issue = serde_json::json!({
        "id": ISSUE_ID,
        "project_id": PROJECT_ID,
        "fingerprint": "payment-error",
        "severity": "error",
        "status": "unresolved",
        "service_name": "checkout-api",
        "release": "checkout@1.2.3",
        "environment": "production",
        "first_seen_at": "2026-07-15T09:00:00Z",
        "last_seen_at": "2026-07-15T10:00:00Z",
        "title": "PaymentError",
        "message": "hostile-secret",
        "stack_trace": "private stack",
        "attributes": {"authorization": "private bearer"},
        "next_action": {"code": code, "target": target},
        "occurrence_count": 3
    });
    if let Some(trace_id) = trace_id {
        issue["trace_id"] = serde_json::Value::String(trace_id.to_owned());
    }
    issue
}

fn trace_summary(trace_id: &str) -> serde_json::Value {
    serde_json::json!({
        "trace_id": trace_id,
        "span_count": 12,
        "error_span_count": 2,
        "service_count": 3,
        "project_count": 1,
        "started_at": "2026-07-15T09:30:00Z",
        "duration_ms": 845,
        "root_span": {"name": "hostile-secret", "attributes": {"authorization": "hostile-bearer"}},
        "slowest_child_span": null,
        "slowest_path": [],
        "error_spans": [],
        "services": ["checkout-api", "payments-api"],
        "releases": ["checkout@1.2.3"],
        "environments": ["production"]
    })
}

async fn mount_issue(server: &MockServer, issue: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/telemetry/issues/{ISSUE_ID}")))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue))
        .expect(1)
        .mount(server)
        .await;
}

async fn run(server: &MockServer, json: bool) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["logbrew", "investigate", "issue", ISSUE_ID];
    if json {
        args.push("--json");
    }
    let command = parse_command(args)?;
    let mut output = Vec::new();
    execute_command(&command, &authenticated_env(server), &mut output).await?;
    Ok(String::from_utf8(output)?)
}

async fn assert_follow_request(
    server: &MockServer,
    expected_path: &str,
    expected_query: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let requests = server
        .received_requests()
        .await
        .ok_or("wiremock request recording is enabled")?;
    let follow = requests
        .iter()
        .find(|request| request.url.path() == expected_path)
        .expect("follow-up request is present");

    assert_eq!(follow.method.as_str(), "GET");
    assert_eq!(follow.url.query(), Some(expected_query));
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    Ok(())
}

fn authenticated_env(server: &MockServer) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some("test-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-issue-investigation-test")),
        cwd: None,
    }
}
