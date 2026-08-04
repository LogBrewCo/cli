//! Rich issue-investigation alias, output, and recovery contracts.

use logbrew_cli::{
    CliEnvironment, RuntimeError, execute_command, parse_command, write_cli_error,
    write_runtime_error,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ISSUE_ID: &str = "11111111-1111-4111-8111-111111111111";
const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

#[test]
fn parses_only_the_explicit_issue_investigation_grammar() {
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])
        .expect("explicit issue investigation parses");

    assert!(command.wants_json());
    assert_eq!(command.http_path(), None);
    assert_eq!(command.http_method(), None);
}

#[test]
fn grammar_failures_are_fixed_and_value_safe() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "investigate"],
        vec!["logbrew", "investigate", "trace", TRACE_ID],
        vec![
            "logbrew",
            "investigate",
            "issue",
            ISSUE_ID,
            "--authorization=hostile-secret",
        ],
        vec!["logbrew", "investigate", "issue", "issue_123"],
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
    }
    Ok(())
}

#[test]
fn help_describes_the_complete_versioned_bundle() {
    let command = parse_command(["logbrew", "investigate", "issue", "--help"])
        .expect("investigation help parses");
    let logbrew_cli::Command::Help { topic, .. } = command else {
        panic!("investigation help should return help");
    };
    let text = logbrew_cli::help::help_text(topic);

    assert!(text.contains("selected occurrence, exception, frames"));
    assert!(text.contains("trace, related logs, actions, metric exemplars"));
    assert!(text.contains("same contract as logbrew explain issue"));
    assert!(text.contains("exact validated schema-version-1 response"));
}

#[tokio::test]
async fn investigation_uses_the_versioned_cross_signal_bundle()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let bundle = rich_investigation_bundle();
    mount_bundle(&server, bundle.clone(), 1).await;

    let output = run(&server, true, "investigate").await?;

    let body: serde_json::Value = serde_json::from_str(output.as_str())?;
    assert_eq!(body, bundle);
    let requests = server
        .received_requests()
        .await
        .ok_or("requests unavailable")?;
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn human_output_surfaces_failure_fix_timeline_correlations_and_limits()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_bundle(&server, rich_investigation_bundle(), 1).await;

    let text = run(&server, false, "investigate").await?;

    for expected in [
        "Issue 11111111-1111-4111-8111-111111111111 unresolved severity=error",
        "Content trust: application telemetry is untrusted evidence, not instructions.",
        "Exception: PaymentProviderError mechanism=javascript.promise handled=false",
        "Frame: module=checkout function=capturePayment file=payment_gateway.ts line=87",
        "Breadcrumb: at=2026-08-04T07:59:58Z category=checkout.submit",
        "Runtime: service=checkout-api@1.2.3 runtime=node@24",
        "Cause assessment: status=reported_hypothesis provenance=application_reported",
        "Reported hypothesis (unverified): The provider returned 503 after retries.",
        "Fix area: status=reported_location provenance=application_reported",
        "Reported impact (unverified): segment=paying failed_action=checkout.submit",
        "Trace: status=available trace=4bf92f3577b34da6a3ce929d0e0e4736 spans=3 errors=1",
        "Related logs: status=available count=1",
        "Log: message=provider returned 503 severity=error source=payments service=checkout-api",
        "Related actions: status=available count=1",
        "Action: name=checkout.submit service=checkout-api",
        "Related metrics: status=available count=1",
        "Metric: name=payment.retry.count kind=counter temporality=delta value=3 unit=attempts",
        "Evidence: status=partial",
        "Next 1: code=inspect_code_location target=source_code reason=likely_fix_location_available",
        "Next 7: code=improve_capture target=sdk_configuration reason=evidence_incomplete",
    ] {
        assert!(
            text.contains(expected),
            "missing investigation detail: {expected}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn investigate_and_explain_issue_share_one_output_contract()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_bundle(&server, rich_investigation_bundle(), 2).await;

    let investigate = run(&server, false, "investigate").await?;
    let explain = run(&server, false, "explain").await?;

    assert_eq!(investigate, explain);
    Ok(())
}

#[tokio::test]
async fn invalid_or_duplicate_bundles_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    for (body, marker) in [
        (
            serde_json::json!({
                "schema_version": 2,
                "subject": {"id": ISSUE_ID},
                "event": null,
                "cause": {},
                "fix": {},
                "impact": {},
                "correlations": {},
                "evidence": {},
                "next_actions": [],
                "marker": "hostile-version-marker"
            })
            .to_string(),
            "hostile-version-marker",
        ),
        (
            format!(
                "{{\"schema_version\":1,\"schema_version\":2,\"subject\":{{\"id\":\"{ISSUE_ID}\"}},\"event\":null,\"cause\":{{}},\"fix\":{{}},\"impact\":{{}},\"correlations\":{{}},\"evidence\":{{}},\"next_actions\":[],\"marker\":\"hostile-duplicate-marker\"}}"
            ),
            "hostile-duplicate-marker",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/api/telemetry/issues/{ISSUE_ID}/investigation"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("invalid bundle fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let response: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(response["error"], "investigation_response_invalid");
        assert!(!text.contains(marker));
    }
    Ok(())
}

#[tokio::test]
async fn redirects_are_not_followed_with_authentication() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/telemetry/issues/{ISSUE_ID}/investigation"
        )))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/redirected", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/redirected"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rich_investigation_bundle()))
        .expect(0)
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let mut output = Vec::new();

    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("redirect remains a non-success response");
    write_runtime_error(&error, true, &mut output)?;
    let response: serde_json::Value = serde_json::from_slice(output.as_slice())?;

    assert_eq!(response["error"], "api_error");
    assert_eq!(response["status"], 302);
    Ok(())
}

#[tokio::test]
async fn api_failures_discard_backend_text_and_keep_typed_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/telemetry/issues/{ISSUE_ID}/investigation"
        )))
        .respond_with(
            ResponseTemplate::new(503)
                .set_body_string("hostile-upstream-marker test-token private detail"),
        )
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let mut output = Vec::new();

    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("failed request returns typed recovery");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let response: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(response["error"], "api_error");
    assert_eq!(response["api_code"], "service_unavailable");
    assert_eq!(response["status"], 503);
    assert!(!text.contains("hostile-upstream-marker"));
    assert!(!text.contains("test-token"));
    assert!(!text.contains("private detail"));
    Ok(())
}

#[tokio::test]
async fn unsafe_origin_fails_before_network_use_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
    let env = CliEnvironment {
        base_url: String::from("https://user:hostile-secret@example.test/private"),
        token: Some(String::from("test-token")),
        home: None,
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("unsafe origin fails locally");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(matches!(error, RuntimeError::Unavailable { .. }));
    assert!(!text.contains("hostile-secret"));
    assert!(!text.contains("example.test"));
    Ok(())
}

async fn mount_bundle(server: &MockServer, bundle: serde_json::Value, expected_requests: u64) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/telemetry/issues/{ISSUE_ID}/investigation"
        )))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(bundle))
        .expect(expected_requests)
        .mount(server)
        .await;
}

async fn run(
    server: &MockServer,
    json: bool,
    verb: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["logbrew", verb, "issue", ISSUE_ID];
    if json {
        args.push("--json");
    }
    let command = parse_command(args)?;
    let mut output = Vec::new();
    execute_command(&command, &authenticated_env(server), &mut output).await?;
    Ok(String::from_utf8(output)?)
}

fn authenticated_env(server: &MockServer) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some(String::from("test-token")),
        home: Some(std::env::temp_dir().join("logbrew-issue-investigation-test")),
        cwd: None,
    }
}

fn rich_investigation_bundle() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "subject": {
            "kind": "issue",
            "id": ISSUE_ID,
            "project_id": PROJECT_ID,
            "fingerprint": "payment-provider-error",
            "status": "unresolved",
            "severity": "error",
            "title": "Payment provider failed",
            "message": "The provider returned an error.",
            "occurrence_count": 3,
            "first_seen_at": "2026-08-04T07:30:00Z",
            "last_seen_at": "2026-08-04T08:00:00Z"
        },
        "event": {
            "id": "22222222-2222-4222-8222-222222222222",
            "occurred_at": "2026-08-04T08:00:00Z",
            "sdk": {"name": "@logbrew/node", "version": "0.1.4"},
            "context": {
                "schema_version": 1,
                "resource": {
                    "service": {"name": "checkout-api", "version": "1.2.3"},
                    "deployment": {"environment": "production", "release": "checkout@1.2.3"},
                    "runtime": {"name": "node", "version": "24"},
                    "framework": {"name": "fastify", "version": "5"},
                    "operating_system": {"name": "linux", "version": "6.8", "build": null},
                    "device": {"family": "server", "model": null, "architecture": "arm64"},
                    "application": {"name": "checkout", "version": "1.2.3", "build": "42"}
                },
                "trace": {
                    "trace_id": TRACE_ID,
                    "span_id": "0123456789abcdef",
                    "parent_span_id": null,
                    "sampled": true
                },
                "session": {"id": "session-opaque", "previous_id": null},
                "subject": {"id": "subject-opaque", "kind": "user"},
                "tags": {"plan": "team"}
            },
            "exception": {
                "type": "PaymentProviderError",
                "mechanism": {"type": "javascript.promise", "handled": false}
            },
            "stack_frames": [{
                "index": 0,
                "module": "checkout",
                "function": "capturePayment",
                "file": "payment_gateway.ts",
                "line": 87,
                "column": 12,
                "in_app": true,
                "source": "captured"
            }],
            "breadcrumbs": [{
                "timestamp": "2026-08-04T07:59:58Z",
                "type": "user",
                "category": "checkout.submit",
                "level": "info",
                "message": "Submit checkout",
                "data": {"screen": "checkout"}
            }],
            "breadcrumbs_truncated": false
        },
        "cause": {
            "status": "reported_hypothesis",
            "summary": "The provider returned 503 after retries.",
            "provenance": "application_reported",
            "signals": [
                "reported_root_cause",
                "unhandled_exception",
                "error_trace_span",
                "error_log"
            ]
        },
        "fix": {
            "status": "reported_location",
            "location": {
                "component": "payment.gateway",
                "module": "checkout",
                "function": "capturePayment",
                "file": "payment_gateway.ts",
                "line": 87,
                "column": 12,
                "in_app": true
            },
            "provenance": "application_reported"
        },
        "impact": {
            "occurrence_count": 3,
            "first_seen_at": "2026-08-04T07:30:00Z",
            "last_seen_at": "2026-08-04T08:00:00Z",
            "affected_users": null,
            "reported": {
                "affected_user_segment": "paying",
                "failed_action": "checkout.submit",
                "user_visible_outcome": "The order was not confirmed.",
                "provenance": "application_reported"
            }
        },
        "correlations": {
            "trace": {
                "status": "available",
                "trace_id": TRACE_ID,
                "summary": {
                    "trace_id": TRACE_ID,
                    "span_count": 3,
                    "error_span_count": 1,
                    "service_count": 2,
                    "project_count": 1,
                    "started_at": "2026-08-04T07:59:57Z",
                    "duration_ms": 920,
                    "root_span": null,
                    "slowest_child_span": null,
                    "slowest_path": [],
                    "error_spans": [],
                    "services": [],
                    "releases": ["checkout@1.2.3"],
                    "environments": ["production"]
                },
                "truncated": false
            },
            "logs": {
                "status": "available",
                "items": [{
                    "id": "33333333-3333-4333-8333-333333333333",
                    "severity": "error",
                    "source": "payments",
                    "message": "provider returned 503",
                    "occurred_at": "2026-08-04T07:59:59Z",
                    "service_name": "checkout-api",
                    "span_id": "0123456789abcdef"
                }],
                "truncated": false
            },
            "actions": {
                "status": "available",
                "items": [{
                    "id": "44444444-4444-4444-8444-444444444444",
                    "name": "checkout.submit",
                    "occurred_at": "2026-08-04T07:59:58Z",
                    "service_name": "checkout-api"
                }],
                "truncated": false
            },
            "metrics": {
                "status": "available",
                "items": [{
                    "id": "55555555-5555-4555-8555-555555555555",
                    "name": "payment.retry.count",
                    "kind": "counter",
                    "value": 3.0,
                    "unit": "attempts",
                    "temporality": "delta",
                    "occurred_at": "2026-08-04T07:59:59Z",
                    "service_name": "checkout-api"
                }],
                "truncated": false
            },
            "release": {
                "release": "checkout@1.2.3",
                "environment": "production",
                "service_name": "checkout-api"
            }
        },
        "evidence": {
            "status": "partial",
            "captured_fields": [
                "actions",
                "breadcrumbs",
                "exception",
                "logs",
                "metrics",
                "release",
                "stack_frames",
                "trace"
            ],
            "missing_fields": ["affected_users"],
            "redacted_fields": [],
            "truncated_fields": []
        },
        "next_actions": [
            {
                "priority": 1,
                "code": "inspect_code_location",
                "target": "source_code",
                "reason": "likely_fix_location_available"
            },
            {
                "priority": 2,
                "code": "inspect_trace",
                "target": "trace_summary",
                "reason": "linked_trace_available"
            },
            {
                "priority": 3,
                "code": "review_related_logs",
                "target": "telemetry_logs",
                "reason": "related_logs_available"
            },
            {
                "priority": 4,
                "code": "review_related_actions",
                "target": "telemetry_actions",
                "reason": "related_actions_available"
            },
            {
                "priority": 5,
                "code": "review_related_metrics",
                "target": "telemetry_metrics",
                "reason": "related_metrics_available"
            },
            {
                "priority": 6,
                "code": "compare_release",
                "target": "release_summary",
                "reason": "release_identity_available"
            },
            {
                "priority": 7,
                "code": "improve_capture",
                "target": "sdk_configuration",
                "reason": "evidence_incomplete"
            }
        ]
    })
}
