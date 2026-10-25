//! CLI API response rendering tests.

use crate::matchers::{body_json, header, query_param};
use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command};
use logbrew_cli::{CliEnvironment, RuntimeError, parse_command, write_runtime_error};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const USER_ID: &str = "323e4567-e89b-12d3-a456-426614174009";

async_test!(authenticated_read_logs_rejects_multiple_json_documents -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/api/logs")
        .and(query_param("project_id", PROJECT_ID))
        .and(query_param("search", "startup failure"))
        .and(query_param("since", "1h"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "{\"project_id\":\"hostile-project\"}\n[]",
            "application/json",
        ))
        .mount(&server);
    let command = parse_command([
        "logbrew",
        "read",
        "logs",
        "--project",
        PROJECT_ID,
        "--search",
        "startup failure",
        "--since",
        "1h",
        "--json",
    ])?;
    let env = super::authenticated_env(&server, "test-token", Some("authenticated-read-test"));
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .err()
        .ok_or("multiple JSON documents fail closed")?;
    assert!(output.is_empty() && matches!(error, RuntimeError::Unavailable { .. }));
    Ok(())
});

async_test!(human_logs_render_populated_and_empty_shapes -> TestResult, {
    let row = |severity| {
        serde_json::json!({
            "level": severity, "severity": severity, "message": "checkout failed",
            "release": "checkout@1.2.3", "environment": "production", "trace_id": "trace_123"
        })
    };
    for (body, release, expected, home) in [
        (
            serde_json::json!({"logs": [row("warning")]}),
            "checkout@1.2.3",
            "Logs (1)\n- warning checkout failed trace=trace_123 [checkout@1.2.3 / production]\n",
            "human-read-logs",
        ),
        (
            serde_json::json!([row("critical")]),
            "checkout@1.2.3",
            "Logs (1)\n- critical checkout failed trace=trace_123 [checkout@1.2.3 / production]\n",
            "human-read-logs-array",
        ),
        (
            serde_json::json!({"logs": []}),
            "empty@0",
            "Logs (0)\nNo logs found.\nNext: widen filters or check --release/--environment.\n",
            "human-empty-logs",
        ),
        (
            serde_json::json!([]),
            "empty@0",
            "Logs (0)\nNo logs found.\nNext: widen filters or check --release/--environment.\n",
            "human-empty-logs-array",
        ),
    ] {
        let server = MockServer::start().await?;
        mount_authenticated_json(
            &server,
            "GET",
            "/api/logs",
            [("release", release), ("environment", "production")],
            &body,
        );
        let text = successful_human_output(
            &server,
            [
                "logbrew",
                "logs",
                "--release",
                release,
                "--env",
                "production",
            ],
            home,
        )
        .await?;
        assert_eq!(text, expected, "case {home}");
    }
    Ok(())
});

async_test!(human_read_actions_and_issues_summarize_pivot_context -> TestResult, {
    struct ContextCase {
        route: &'static str,
        query: &'static [(&'static str, &'static str)],
        body: serde_json::Value,
        args: &'static [&'static str],
        home: &'static str,
        expected: &'static str,
    }

    let cases = [
        ContextCase {
            route: "/api/telemetry/actions",
            query: &[
                ("release", "checkout@1.2.3"),
                ("environment", "production"),
                ("name", "checkout_failed"),
            ],
            body: serde_json::json!([
                {
                    "name": "checkout_failed",
                    "severity": "warning",
                    "distinct_id": "user_123",
                    "trace_id": "trace_123",
                    "release": "checkout@1.2.3",
                    "environment": "production"
                }
            ]),
            args: &[
                "logbrew",
                "actions",
                "--release",
                "checkout@1.2.3",
                "--environment",
                "production",
                "--name",
                "checkout_failed",
            ],
            home: "human-read-actions-context",
            expected: "Actions (1)\n- checkout_failed warning user=user_123 trace=trace_123 \
                           [checkout@1.2.3 / production]\n",
        },
        ContextCase {
            route: "/api/telemetry/issues",
            query: &[
                ("release", "checkout@1.2.3"),
                ("environment", "production"),
                ("status", "unresolved"),
            ],
            body: serde_json::json!([
                {
                    "id": "issue_123",
                    "status": "unresolved",
                    "severity": "error",
                    "title": "PaymentError",
                    "occurrence_count": 2,
                    "trace_id": "trace_123",
                    "release": "checkout@1.2.3",
                    "environment": "production"
                }
            ]),
            args: &[
                "logbrew",
                "issues",
                "--release",
                "checkout@1.2.3",
                "--environment",
                "production",
                "--status",
                "unresolved",
            ],
            home: "human-read-issues-context",
            expected: "Issues (1)\n- issue_123 unresolved error PaymentError occurrences=2 \
                           trace=trace_123 [checkout@1.2.3 / production]\n",
        },
    ];

    for case in cases {
        let server = MockServer::start().await?;
        mount_authenticated_json(
            &server,
            "GET",
            case.route,
            case.query.iter().copied(),
            &case.body,
        );
        let text = successful_human_output(&server, case.args.iter().copied(), case.home)
            .await?;

        assert_eq!(text, case.expected);
    }
    Ok(())
});

async_test!(collection_reads_preserve_json_shape_and_render_service_name -> TestResult,
{
    struct ServiceCase {
        route: &'static str,
        body: serde_json::Value,
        args: &'static [&'static str],
        home: &'static str,
        expected: &'static str,
    }

    let cases = [
        ServiceCase {
            route: "/api/logs",
            body: serde_json::json!([{
                "level": "warning",
                "message": "checkout failed",
                "service_name": "checkout-api",
                "trace_id": "trace_123"
            }]),
            args: &[
                "logbrew",
                "logs",
                "--service",
                "checkout-api",
                "--since",
                "24h",
            ],
            home: "human-service-logs",
            expected: "Logs (1)\n- warning checkout failed service=checkout-api trace=trace_123\n",
        },
        ServiceCase {
            route: "/api/telemetry/issues",
            body: serde_json::json!([{
                "id": "issue_123",
                "status": "unresolved",
                "severity": "error",
                "title": "PaymentError",
                "occurrence_count": 2,
                "service_name": "checkout-api",
                "trace_id": "trace_123"
            }]),
            args: &[
                "logbrew",
                "issues",
                "--service",
                "checkout-api",
                "--since",
                "24h",
            ],
            home: "human-service-issues",
            expected: "Issues (1)\n- issue_123 unresolved error PaymentError occurrences=2 \
                       service=checkout-api trace=trace_123\n",
        },
        ServiceCase {
            route: "/api/telemetry/actions",
            body: serde_json::json!([{
                "name": "checkout_failed",
                "severity": "warning",
                "service_name": "checkout-api",
                "distinct_id": "user_123"
            }]),
            args: &[
                "logbrew",
                "actions",
                "--service",
                "checkout-api",
                "--since",
                "24h",
            ],
            home: "human-service-actions",
            expected: "Actions (1)\n- checkout_failed warning service=checkout-api user=user_123\n",
        },
        ServiceCase {
            route: "/api/telemetry/releases",
            body: serde_json::json!([{
                "release": "checkout@1.2.3",
                "environment": "production",
                "service_name": "checkout-api",
                "log_count": 1,
                "issue_count": 1,
                "trace_span_count": 1,
                "action_count": 1
            }]),
            args: &[
                "logbrew",
                "releases",
                "--service",
                "checkout-api",
                "--since",
                "24h",
            ],
            home: "human-service-releases",
            expected: "Releases (1)\n- checkout@1.2.3 production service=checkout-api logs=1 issues=1 \
                       spans=1 actions=1\n",
        },
    ];

    for case in cases {
        let server = MockServer::start().await?;
        mount_authenticated_json(
            &server,
            "GET",
            case.route,
            [("service_name", "checkout-api"), ("since", "24h")],
            &case.body.clone(),
        );

        let text = successful_human_output(&server, case.args.iter().copied(), case.home)
            .await?;
        assert_eq!(text, case.expected);

        let mut json_args = case.args.to_vec();
        json_args.push("--json");
        let command = parse_command(json_args.iter().copied())?;
        let env = authenticated_env(&server, case.home);
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output)
            .await?;

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice())?;
        assert_eq!(body, case.body, "JSON evidence changed for {}", case.home);
        let rows = body.as_array().ok_or("backend list shape stays bare")?;
        assert_eq!(rows[0]["service_name"], "checkout-api");
    }
    Ok(())
});

async_test!(issue_list_json_omits_a_null_raw_stack_and_keeps_frames -> TestResult, {
    let frames = serde_json::json!({"exceptionChain": {"entries": [{"stackFrames": [
        {"filename": "src/checkout.ts", "function": "pay", "line": 12}
    ]}]}});
    let issue = |id: &str, stack: serde_json::Value| {
        serde_json::json!({"id": id, "status": "unresolved", "stack_trace": stack, "attributes": frames})
    };
    for (args, body) in [
        (
            &["logbrew", "issues", "--json"][..],
            serde_json::json!([issue("a", serde_json::Value::Null), issue("b", "Error: raw".into())]),
        ),
        (
            &["logbrew", "issues", "--pagination", "cursor", "--json"][..],
            serde_json::json!({
                "issues": [issue("a", serde_json::Value::Null), issue("b", "Error: raw".into())],
                "next_cursor": null
            }),
        ),
    ] {
        let server = MockServer::start().await?;
        mount_authenticated_json(&server, "GET", "/api/telemetry/issues", [], &body);
        let env = authenticated_env(&server, "issue-list-null-stack");
        let mut output = Vec::new();

        execute_command(&parse_command(args.iter().copied())?, &env, &mut output).await?;

        let printed: serde_json::Value = serde_json::from_slice(&output)?;
        let issues = printed.get("issues").unwrap_or(&printed);
        assert!(issues[0].get("stack_trace").is_none(), "{printed}");
        assert_eq!(issues[0]["attributes"], frames);
        assert_eq!(issues[1]["stack_trace"], "Error: raw");
        assert_eq!(printed.get("next_cursor"), body.get("next_cursor"));
    }
    Ok(())
});

async_test!(invalid_since_preserves_backend_validation_recovery_for_agents -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/api/telemetry/issues")
        .and(query_param("service_name", "checkout-api"))
        .and(query_param("since", "0h"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(422).set_body_json(&serde_json::json!({
            "code": "validation_failed",
            "error": "invalid since value",
            "next": "use since=24h, since=7d, or an RFC3339 timestamp"
        })))
        .mount(&server);
    let command = parse_command([
        "logbrew",
        "issues",
        "--service",
        "checkout-api",
        "--since",
        "0h",
        "--json",
    ])?;
    let env = authenticated_env(&server, "invalid-since-recovery");
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .err()
        .ok_or("invalid since fails")?;
    write_runtime_error(&error, command.wants_json(), &mut output)?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "api_error");
    assert_eq!(body["status"], 422);
    assert_eq!(body["api_code"], "validation_failed");
    assert_eq!(body["api_error"], "invalid since value");
    assert_eq!(
        body["next"],
        "use since=24h, since=7d, or an RFC3339 timestamp"
    );
    Ok(())
});

async_test!(human_read_releases_prints_all_telemetry_counts -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/api/telemetry/releases")
        .and(query_param("environment", "production"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "releases": [
                {
                    "release": "checkout@1.2.3",
                    "environment": "production",
                    "log_count": 1,
                    "issue_count": 1,
                    "trace_span_count": 1,
                    "action_count": 1
                }
            ]
        })))
        .mount(&server);
    let text = successful_human_output(
        &server,
        ["logbrew", "releases", "--environment", "production"],
        "human-read-releases",
    )
    .await?;
    assert_eq!(
        text,
        "Releases (1)\n- checkout@1.2.3 production logs=1 issues=1 spans=1 actions=1\n"
    );
    Ok(())
});

async_test!(human_explain_trace_prints_scan_friendly_summary -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth(
            "GET",
            "/api/telemetry/traces/trace_123/investigation",
            "test-token",
        )
        .and(query_param("response_version", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "schema_version": 3,
            "subject": {
                "kind": "trace",
                "trace_id": "trace_123",
                "analyzed_span_count": 2,
                "error_span_count": 1,
                "service_count": 1,
                "project_count": 1,
                "started_at": "2026-06-02T20:00:00Z",
                "duration_ms": 845,
                "releases": ["checkout@1.2.3"],
                "environments": ["production"]
            },
            "analysis": {
                "status": "errors_observed",
                "causality": "evidence_only",
                "root_span": null,
                "first_error_span": {
                    "name": "charge card",
                    "service_name": "checkout-api",
                    "operation": "payment.charge",
                    "status": "error",
                    "duration_ms": 420,
                    "span_id": "0123456789abcdef",
                    "parent_span_id": null
                },
                "first_error_path": [{"name": "charge card"}],
                "bottleneck_span": null,
                "bottleneck_path": []
            },
            "spans": {"items": [], "truncated": false},
            "correlations": {
                "window": {
                    "since": "2026-06-02T20:00:00Z",
                    "until": "2026-06-02T20:00:01Z",
                    "scopes": [{
                        "project_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                        "environment": "production",
                        "release": "checkout@1.2.3"
                    }],
                    "truncated": false
                },
                "issues": {"status": "available", "items": [{
                    "id": "11111111-1111-4111-8111-111111111111",
                    "issue_id": "22222222-2222-4222-8222-222222222222",
                    "project_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                    "severity": "error",
                    "title": "PaymentError",
                    "message": "provider response could not be decoded",
                    "occurred_at": "2026-06-02T20:00:00.250Z",
                    "service_name": "checkout-api",
                    "span_id": "0123456789abcdef",
                    "environment": "production",
                    "release": "checkout@1.2.3",
                    "cause": {"status": "reported_hypothesis", "summary": "The provider returned a malformed response.",
                        "provenance": "application_reported", "signals": ["reported_root_cause"]},
                    "fix": {
                        "status": "reported_location",
                        "location": {"component": "checkout-api", "module": null,
                            "function": "decodeProviderResponse", "file": "src/provider.rs", "line": 87,
                            "column": null, "in_app": true},
                        "provenance": "application_reported"
                    }
                }], "truncated": false},
                "logs": {"status": "not_found", "items": [], "truncated": false},
                "actions": {"status": "not_found", "items": [], "truncated": false},
                "metrics": {"status": "not_found", "items": [], "truncated": false}
            },
            "timeline": {"items": [
                {"kind": "span", "occurred_at": "2026-06-02T20:00:00Z"},
                {"kind": "span", "occurred_at": "2026-06-02T20:00:00.500Z"}
            ], "truncated": false},
            "evidence": {
                "status": "partial",
                "captured_fields": ["trace.spans"],
                "missing_fields": ["span.attributes"],
                "redacted_fields": [],
                "truncated_fields": []
            },
            "next_actions": [{
                "priority": 1,
                "code": "inspect_error_span",
                "target": "trace_span",
                "reason": "inspect the first retained error span"
            }]
        })))
        .mount(&server);
    let text = successful_human_output(
        &server,
        ["logbrew", "explain", "trace", "trace_123"],
        "human-explain-trace",
    )
    .await?;
    assert_eq!(
        text,
        "Trace trace_123 status=errors_observed causality=evidence_only spans=2 errors=1 services=1 \
             projects=1 duration_ms=845\nStarted: 2026-06-02T20:00:00Z\nReleases: \
             checkout@1.2.3\nEnvironments: production\nFirst error: charge card \
             service=checkout-api operation=payment.charge status=error duration_ms=420 \
             span=0123456789abcdef\nFirst error path: charge card\nRelated issues: \
             status=available count=1 truncated=false\nIssue: title=PaymentError severity=error \
             issue=22222222-2222-4222-8222-222222222222 span=0123456789abcdef at=2026-06-02T20:00:00.250Z\nCause \
             assessment: status=reported_hypothesis provenance=application_reported\nReported hypothesis \
             (unverified): The provider returned a malformed response.\nCause signals: reported_root_cause\nFix area: \
             status=reported_location provenance=application_reported component=checkout-api \
             function=decodeProviderResponse file=src/provider.rs line=87 in_app=true\nRelated logs: status=not_found \
             count=0 truncated=false\nRelated actions: status=not_found count=0 \
             truncated=false\nRelated metrics: status=not_found count=0 truncated=false\nTimeline: \
             count=2 truncated=false\nTimeline item: at=2026-06-02T20:00:00Z kind=span\nTimeline item: \
             at=2026-06-02T20:00:00.500Z kind=span\nEvidence: status=partial captured=1 missing=1 redacted=0 \
             truncated=0\nMissing: span.attributes\nNext 1: code=inspect_error_span target=trace_span \
             reason=inspect the first retained error span\n"
    );
    Ok(())
});

async_test!(human_read_trace_summarizes_real_api_array_shape -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/telemetry/traces/trace_123", "test-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([
            {
                "trace_id": "trace_123",
                "name": "checkout",
                "release": "checkout@1.2.3",
                "environment": "production"
            }
        ])))
        .mount(&server);
    let text = successful_human_output(
        &server,
        ["logbrew", "trace", "trace_123"],
        "human-trace-array",
    )
    .await?;
    assert_eq!(
        text,
        "Trace trace_123 spans=1 [checkout@1.2.3 / production]\n- checkout\n"
    );
    Ok(())
});

async_test!(human_read_issue_summarizes_real_api_object_shape_with_next_action -> TestResult,
{
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/telemetry/issues/issue_123", "test-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": "issue_123",
            "status": "unresolved",
            "severity": "error",
            "title": "PaymentError",
            "message": "card declined",
            "occurrence_count": 2,
            "first_seen_at": "2026-06-02T19:00:00Z",
            "last_seen_at": "2026-06-02T20:00:00Z",
            "trace_id": "trace_123",
            "release": "checkout@1.2.3",
            "environment": "production"
        })))
        .mount(&server);
    let text = successful_human_output(
        &server,
        ["logbrew", "issue", "issue_123"],
        "human-issue-object",
    )
    .await?;
    assert_eq!(
        text,
        "Issue issue_123 unresolved error trace=trace_123 [checkout@1.2.3 / production]\nTitle: \
         PaymentError\nMessage: card declined\nOccurrences: 2\nFirst seen: \
         2026-06-02T19:00:00Z\nLast seen: 2026-06-02T20:00:00Z\nNext: logbrew resolve issue_123 \
         or logbrew ignore issue_123\n"
    );
    Ok(())
});

async_test!(human_set_issue_status_renders_wrapped_and_object_shapes -> TestResult, {
    let issue = serde_json::json!({
        "id": "issue_123", "status": "resolved", "trace_id": "trace_123",
        "release": "checkout@1.2.3", "environment": "production"
    });
    for (body, args, home) in [
        (
            serde_json::json!({"issue": issue}),
            &["logbrew", "set", "issue", "issue_123", "resolved"][..],
            "human-set-issue",
        ),
        (
            issue,
            &["logbrew", "resolve", "issue_123"][..],
            "human-set-issue-object",
        ),
    ] {
        let server = MockServer::start().await?;
        Mock::auth("PATCH", "/api/telemetry/issues/issue_123", "test-token")
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server);
        let text = successful_human_output(&server, args.iter().copied(), home)
            .await?;
        assert_eq!(
            text,
            "Issue issue_123 marked resolved trace=trace_123 [checkout@1.2.3 / production].\n"
        );
    }
    Ok(())
});

async_test!(project_setup_seen_posts_backend_owned_setup_state -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("POST", "/api/projects/proj_123/setup/seen", "test-token")
        .and(body_json(serde_json::json!({
            "runtime": "node",
            "source": "cli",
            "environment": "production"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "status": "sdk_seen",
            "runtime": "node",
            "source": "cli",
            "environment": "production",
            "last_seen_at": "2026-06-15T20:00:00Z",
            "next": "send telemetry for this project"
        })))
        .mount(&server);
    let command = parse_command([
        "logbrew",
        "projects",
        "setup",
        "proj_123",
        "--runtime",
        "node",
        "--source",
        "cli",
        "--environment",
        "production",
        "--json",
    ])?;
    let env = authenticated_env(&server, "project-setup-seen-json");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["status"], "sdk_seen");
    assert_eq!(body["runtime"], "node");
    assert_eq!(body["source"], "cli");
    assert_eq!(body["environment"], "production");
    assert_eq!(body["next"], "send telemetry for this project");
    Ok(())
});

async_test!(project_setup_seen_omits_source_for_ingest_key_auth -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth(
        "POST",
        "/api/projects/proj_123/setup/seen",
        "lbw_ingest_test",
    )
    .and(body_json(serde_json::json!({ "runtime": "node" })))
    .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
        "status": "sdk_seen",
        "runtime": "node",
        "source": "sdk",
        "next": "send telemetry for this project"
    })))
    .mount(&server);
    let command = parse_command([
        "logbrew",
        "projects",
        "setup",
        "proj_123",
        "--runtime",
        "node",
        "--json",
    ])?;
    let env =
        super::authenticated_env(&server, "lbw_ingest_test", Some("project-setup-ingest-key"));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["source"], "sdk");
    Ok(())
});

async_test!(human_project_setup_seen_prints_status_and_next -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("POST", "/api/projects/proj_123/setup/seen", "test-token")
        .and(body_json(serde_json::json!({ "source": "cli" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "status": "sdk_seen",
            "source": "cli",
            "last_seen_at": "2026-06-15T20:00:00Z",
            "next": "send telemetry for this project"
        })))
        .mount(&server);
    let text = successful_human_output(
        &server,
        ["logbrew", "projects", "setup", "proj_123"],
        "human-project-setup-seen",
    )
    .await?;

    assert_eq!(
        text,
        "Project setup seen: sdk_seen\nLast seen: 2026-06-15T20:00:00Z\nNext: send telemetry for this project\n"
    );
    Ok(())
});

async_test!(local_auth_refreshes_once_and_persists_replacement_credentials -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/logs", "expired-access")
        .respond_with(ResponseTemplate::new(401).set_body_json(&serde_json::json!({
            "error": "not_logged_in"
        })))
        .expect(1)
        .mount(&server);
    Mock::route("POST", "/api/auth/refresh")
        .and(body_json(serde_json::json!({
            "refresh_token": "old-refresh"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "access_token": "fresh-access",
            "refresh_token": "fresh-refresh"
        })))
        .expect(1)
        .mount(&server);
    Mock::auth("GET", "/api/logs", "fresh-access")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "logs": []
        })))
        .expect(1)
        .mount(&server);

    let command = parse_command(["logbrew", "logs", "--json"])?;
    let home = super::isolated_home("logbrew-cli", "refresh-local-auth")?;
    let session = super::write_test_session(
        &home,
        server.uri().as_str(),
        "expired-access",
        "old-refresh",
    )?;
    let env = super::test_env(&server, None, Some(home));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["logs"], serde_json::json!([]));
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(session)?)?;
    assert_eq!(saved["access_token"], "fresh-access");
    assert_eq!(saved["refresh_token"], "fresh-refresh");
    for secret in [
        "expired-access",
        "old-refresh",
        "fresh-access",
        "fresh-refresh",
    ] {
        assert!(!text.contains(secret));
    }
    Ok(())
});

async_test!(concurrent_local_401s_share_one_refresh_rotation -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/logs", "shared-expired")
        .respond_with(ResponseTemplate::new(401))
        .expect(2)
        .mount(&server);
    Mock::route("POST", "/api/auth/refresh")
        .and(body_json(serde_json::json!({
            "refresh_token": "shared-refresh"
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(50))
                .set_body_json(&serde_json::json!({
                    "access_token": "shared-fresh",
                    "refresh_token": "shared-next-refresh"
                })),
        )
        .expect(1)
        .mount(&server);
    Mock::auth("GET", "/api/logs", "shared-fresh")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "logs": []
        })))
        .expect(2)
        .mount(&server);
    let home = super::isolated_home("logbrew-cli", "refresh-concurrent")?;
    let session = super::write_test_session(
        &home,
        server.uri().as_str(),
        "shared-expired",
        "shared-refresh",
    )?;
    let env = super::test_env(&server, None, Some(home));
    let command = parse_command(["logbrew", "logs", "--json"])?;
    let mut first_output = Vec::new();
    let mut second_output = Vec::new();

    let (first, second) = futures_util::future::join(
        execute_command(&command, &env, &mut first_output),
        execute_command(&command, &env, &mut second_output),
    )
    .await;
    first?;
    second?;
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(session)?)?;
    assert_eq!(saved["access_token"], "shared-fresh");
    assert_eq!(saved["refresh_token"], "shared-next-refresh");
    assert_eq!(saved["origin"], server.uri());

    for output in [first_output, second_output] {
        let text = String::from_utf8(output)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text)?["logs"],
            serde_json::json!([])
        );
        for secret in [
            "shared-expired",
            "shared-refresh",
            "shared-fresh",
            "shared-next-refresh",
        ] {
            assert!(!text.contains(secret));
        }
    }
    Ok(())
});

async_test!(env_auth_401_never_reads_or_rotates_local_refresh_credentials -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/logs", "env-expired")
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server);
    Mock::route("POST", "/api/auth/refresh")
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server);
    let home = super::isolated_home("logbrew-cli", "refresh-env-isolated")?;
    let auth_dir = home.join(".logbrew");
    std::fs::create_dir_all(auth_dir.as_path())?;
    std::fs::write(auth_dir.join("token"), "local-access\n")?;
    std::fs::write(auth_dir.join("refresh-token"), "local-refresh\n")?;
    let command = parse_command(["logbrew", "logs", "--json"])?;
    let env = super::test_env(&server, Some("env-expired"), Some(home));
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .err()
        .ok_or("env 401 remains an auth error")?;

    assert!(matches!(error, RuntimeError::Api { status: 401, .. }));
    assert!(output.is_empty(), "environment auth failure must not emit data");
    assert_eq!(
        std::fs::read_to_string(auth_dir.join("token"))?.trim(),
        "local-access"
    );
    assert_eq!(
        std::fs::read_to_string(auth_dir.join("refresh-token"))?.trim(),
        "local-refresh"
    );
    Ok(())
});

async_test!(persisted_refresh_session_never_authenticates_to_a_different_api_origin -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/api/logs")
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server);
    Mock::route("POST", "/api/auth/refresh")
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server);
    let home = super::isolated_home("logbrew-cli", "refresh-origin-mismatch")?;
    let _session = super::write_test_session(
        &home,
        "https://trusted.example",
        "origin-access",
        "origin-refresh",
    )?;
    let command = parse_command(["logbrew", "logs", "--json"])?;
    let env = super::test_env(&server, None, Some(home));
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .err()
        .ok_or("origin mismatch fails before network access")?;

    assert!(matches!(error, RuntimeError::Unavailable { .. }));
    assert_eq!(output, b"");
    Ok(())
});

async_test!(api_auth_errors_report_source_without_leaking_tokens -> TestResult, {
    for (status, code, api_token, local_token, json) in [
        (401, "not_logged_in", "expired", "expired", true),
        (403, "forbidden", "env-token", "file-token", false),
    ] {
        let server = MockServer::start().await?;
        Mock::auth("GET", "/api/logs", api_token)
            .respond_with(
                ResponseTemplate::new(status).set_body_json(&serde_json::json!({
                    "ok": false, "error": code, "detail": api_token
                })),
            )
            .mount(&server);
        let args = ["logbrew", "logs", "--json"];
        let command = parse_command(if json { &args[..] } else { &args[..2] })?;
        let home = super::isolated_home("logbrew-cli", if json {
            "api-auth-token-file"
        } else {
            "api-auth-env"
        })?;
        let token_path = home.join(".logbrew").join("token");
        std::fs::create_dir_all(token_path.parent().ok_or("token path has parent")?)?;
        std::fs::write(token_path, format!("{local_token}\n"))?;
        let env = super::test_env(&server, (status == 403).then_some(api_token), Some(home));
        let mut output = Vec::new();
        let error = execute_command(&command, &env, &mut output)
            .await
            .err()
            .ok_or("auth fails")?;
        write_runtime_error(&error, command.wants_json(), &mut output)?;
        let text = String::from_utf8(output)?;
        assert!(!text.contains(api_token) && !text.contains(local_token));
        if json {
            let body: serde_json::Value = serde_json::from_str(text.as_str())?;
            assert_eq!(body["ok"], false);
            assert_eq!(body["error"], "api_error");
            assert_eq!(body["status"], 401);
            assert_eq!(body["auth_source"], "token_file");
            assert_eq!(body["next"], "run logbrew login");
        } else {
            assert!(
                text.contains("api returned status 403")
                    && text.contains("Auth: logged in (env token)")
                    && text.contains("Next: run logbrew login")
            );
        }
    }
    Ok(())
});

async_test!(human_archive_renders_when_the_issue_reopens -> TestResult, {
    let issue = "223e4567-e89b-12d3-a456-426614174001";
    for (flags, archive, label) in [
        (
            "--events 100",
            serde_json::json!({"until": "events", "count": 100}),
            serde_json::json!({"until": "events", "reopen_at": null, "count": 100}),
        ),
        (
            "--for 7d",
            serde_json::json!({"until": "time", "duration": "7d"}),
            serde_json::json!({"until": "time", "reopen_at": "2026-10-30T12:00:00.000Z", "count": null}),
        ),
        (
            "--until escalating",
            serde_json::json!({"until": "escalating"}),
            serde_json::json!({"until": "escalating", "reopen_at": null, "count": 60}),
        ),
    ] {
        let server = MockServer::start().await?;
        Mock::auth("PATCH", format!("/api/telemetry/issues/{issue}"), "test-token")
            .and(query_param("response_version", "5"))
            .and(body_json(serde_json::json!({"status": "archived", "archive": archive})))
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
                "id": issue, "status": "archived", "priority": "medium",
                "priority_source": "severity", "assignee": null, "archive": label
            })))
            .mount(&server);
        let args = ["logbrew", "issues", "archive", "223e4567-e89b-12d3-a456-426614174001"]
            .into_iter()
            .chain(flags.split_whitespace());
        let text = successful_human_output(&server, args, "human-archive").await?;
        let reopens = match flags {
            "--events 100" => "reopens after 100 more events",
            "--for 7d" => "reopens at 2026-10-30T12:00:00.000Z",
            _ => "reopens when it escalates (60 events in 1 hour)",
        };
        assert_eq!(
            text,
            format!("Issue {issue} marked archived, {reopens}, priority medium, assignee unassigned.\n"),
            "{flags}"
        );
    }
    Ok(())
});

async fn successful_human_output<I>(
    server: &MockServer,
    args: I,
    home_name: &str,
) -> TestResult<String>
where
    I: IntoIterator<Item = &'static str>,
{
    let command = parse_command(args)?;
    let env = authenticated_env(server, home_name);
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    Ok(String::from_utf8(output)?)
}

fn mount_authenticated_json<I>(
    server: &MockServer,
    http_method: &str,
    route: &str,
    query: I,
    body: &serde_json::Value,
) where
    I: IntoIterator<Item = (&'static str, &'static str)>,
{
    let mut builder =
        Mock::route(http_method, route).and(header("authorization", "Bearer test-token"));
    for (name, value) in query {
        builder = builder.and(query_param(name, value));
    }
    builder
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server);
}

fn authenticated_env(server: &MockServer, home_name: &str) -> CliEnvironment {
    super::test_env(
        server,
        Some("test-token"),
        Some(std::env::temp_dir().join(format!("logbrew-{home_name}-{}", std::process::id()))),
    )
}

async_test!(human_issue_changes_render_priority_and_bulk_outcomes -> TestResult, {
    let issue = "223e4567-e89b-12d3-a456-426614174001";
    let other = "223e4567-e89b-12d3-a456-426614174002";
    let third = "223e4567-e89b-12d3-a456-426614174003";
    let server = MockServer::start().await?;
    Mock::auth("PATCH", format!("/api/telemetry/issues/{issue}"), "test-token")
        .and(query_param("response_version", "4"))
        .and(body_json(serde_json::json!({"priority": "low"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": issue, "status": "archived", "priority": "low", "priority_source": "manual",
            "assignee": null, "release": "checkout@1.2.3", "environment": "production"
        })))
        .mount(&server);
    let line = concat!("logbrew issues priority low ", "223e4567-e89b-12d3-a456-426614174001");
    let text = successful_human_output(&server, line.split_whitespace(), "human-issue-priority").await?;
    assert_eq!(
        text,
        format!(
            "Issue {issue} marked archived, priority low, assignee unassigned \
             [checkout@1.2.3 / production].\n"
        )
    );

    let server = MockServer::start().await?;
    Mock::auth("POST", "/api/telemetry/issues/bulk", "test-token")
        .and(body_json(serde_json::json!({
            "project_id": PROJECT_ID, "issue_ids": [issue, other, third], "assignee": USER_ID,
            "response_version": 2
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "project_id": PROJECT_ID,
            "results": [
                {"issue_id": issue, "outcome": "updated",
                 "issue": {"id": issue, "status": "archived", "priority": "high",
                           "assignee": {"user_id": USER_ID, "name": "Ada", "email": "ada@example.com"}}},
                {"issue_id": other, "outcome": "unchanged",
                 "issue": {"id": other, "status": "unresolved", "priority": "low",
                           "assignee": null}},
                {"issue_id": third, "outcome": "not_found", "issue": null}
            ]
        })))
        .mount(&server);
    let line = concat!(
        "logbrew issues assign 323e4567-e89b-12d3-a456-426614174009 ",
        "223e4567-e89b-12d3-a456-426614174001 223e4567-e89b-12d3-a456-426614174002 ",
        "223e4567-e89b-12d3-a456-426614174003 --project 123e4567-e89b-12d3-a456-426614174000"
    );
    let text = successful_human_output(&server, line.split_whitespace(), "human-issue-bulk").await?;
    assert_eq!(
        text,
        format!(
            "Project {PROJECT_ID}: 1 updated, 1 unchanged, 1 not found.\n  {issue} updated: archived, \
             priority high, assignee Ada\n  {other} unchanged: unresolved, priority low, assignee \
             unassigned\n  {third} not found\n"
        )
    );
    Ok(())
});

async_test!(human_issue_reads_and_changes_show_the_assignee -> TestResult, {
    let issue = "223e4567-e89b-12d3-a456-426614174001";
    let no_name = serde_json::json!({"user_id": USER_ID, "name": null, "email": "dev@example.com"});
    let server = MockServer::start().await?;
    Mock::auth("PATCH", format!("/api/telemetry/issues/{issue}"), "test-token")
        .and(query_param("response_version", "4"))
        .and(body_json(serde_json::json!({"assignee": "me"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": issue, "status": "unresolved", "priority": "medium", "assignee": no_name
        })))
        .mount(&server);
    let line = concat!("logbrew issues assign me ", "223e4567-e89b-12d3-a456-426614174001");
    let text = successful_human_output(&server, line.split_whitespace(), "human-issue-assign").await?;
    assert_eq!(
        text,
        format!("Issue {issue} marked unresolved, priority medium, assignee dev@example.com.\n")
    );

    let server = MockServer::start().await?;
    mount_authenticated_json(
        &server,
        "GET",
        "/api/telemetry/issues/issue_123",
        [("response_version", "4")],
        &serde_json::json!({
            "id": "issue_123", "status": "unresolved", "priority": "high",
            "priority_source": "derived",
            "assignee": {"user_id": USER_ID, "name": " ", "email": ""}
        }),
    );
    let text = successful_human_output(&server, ["logbrew", "issue", "issue_123"], "human-issue-assignee").await?;
    assert_eq!(
        text,
        format!(
            "Issue issue_123 unresolved\nPriority: high (derived)\nAssignee: {USER_ID}\nNext: \
             logbrew resolve issue_123 or logbrew ignore issue_123\n"
        )
    );

    let server = MockServer::start().await?;
    mount_authenticated_json(
        &server,
        "GET",
        "/api/telemetry/issues",
        [("assignee", "none"), ("response_version", "4")],
        &serde_json::json!([
            {"id": "issue_1", "status": "unresolved", "title": "PaymentError", "assignee": null},
            {"id": "issue_2", "status": "resolved", "title": "Timeout",
             "assignee": {"user_id": USER_ID, "name": "Ada\u{1b}[2J", "email": "ada@example.com"}}
        ]),
    );
    let text = successful_human_output(
        &server,
        ["logbrew", "issues", "--assignee", "none"],
        "human-issue-list-assignee",
    )
    .await?;
    assert_eq!(
        text,
        "Issues (2)\n- issue_1 unresolved PaymentError assignee=unassigned\n- issue_2 resolved \
         Timeout assignee=Ada\\u{1b}[2J\n"
    );
    Ok(())
});

async_test!(issue_assignees_render_one_line_per_account_and_keep_json -> TestResult, {
    let body = serde_json::json!({
        "project_id": PROJECT_ID,
        "assignees": [
            {"user_id": USER_ID, "name": "Ada Lovelace", "email": "ada@example.com"},
            {"user_id": "323e4567-e89b-12d3-a456-42661417400a", "name": null, "email": "dev@example.com"}
        ],
        "teams": [
            {"team_id": "523e4567-e89b-12d3-a456-426614174005", "slug": "payments", "name": "Payments"}
        ]
    });
    let server = MockServer::start().await?;
    mount_authenticated_json(
        &server,
        "GET",
        "/api/telemetry/issues/assignees",
        [("project_id", PROJECT_ID), ("response_version", "2")],
        &body,
    );
    let line = "logbrew issues assignees --project 123e4567-e89b-12d3-a456-426614174000";
    let text = successful_human_output(&server, line.split_whitespace(), "human-assignees").await?;
    assert_eq!(
        text,
        format!(
            "Assignees for project {PROJECT_ID} (2)\n- {USER_ID} Ada Lovelace <ada@example.com>\n- \
             323e4567-e89b-12d3-a456-42661417400a <dev@example.com>\nTeams (1)\n- \
             team:523e4567-e89b-12d3-a456-426614174005 #payments Payments\n"
        )
    );
    let json = successful_human_output(
        &server,
        "logbrew issues assignees --project 123e4567-e89b-12d3-a456-426614174000 --json"
        .split_whitespace(),
        "json-assignees",
    )
    .await?;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, body);

    let server = MockServer::start().await?;
    let malformed = serde_json::json!({
        "project_id": PROJECT_ID, "assignees": [{"user_id": "not-a-user", "name": null, "email": "x"}]
    });
    mount_authenticated_json(
        &server,
        "GET",
        "/api/telemetry/issues/assignees",
        [("project_id", PROJECT_ID), ("response_version", "2")],
        &malformed,
    );
    let text = successful_human_output(&server, line.split_whitespace(), "human-assignees-bad").await?;
    assert!(!text.starts_with("Assignees"), "malformed accounts are not summarized: {text}");
    Ok(())
});

async_test!(issue_owners_render_reasons_inputs_and_the_next_assignment -> TestResult, {
    let issue = "223e4567-e89b-12d3-a456-426614174001";
    let other = "323e4567-e89b-12d3-a456-42661417400a";
    let ada = serde_json::json!({"user_id": USER_ID, "name": "Ada Lovelace", "email": "ada@example.com"});
    let body = serde_json::json!({
        "issue_id": issue, "project_id": PROJECT_ID, "suggested_assignee": ada,
        "owners": [
            {"user_id": USER_ID, "name": "Ada Lovelace", "email": "ada@example.com",
             "source": "ownership_rule", "rule": "path:src/payments/** ada@example.com",
             "line": 2, "matched": "src/payments/charge.ts"},
            {"user_id": other, "name": "Dev\u{1b}[2J", "email": "dev@example.com",
             "source": "recent_activity", "rule": null, "line": null, "matched": null}
        ],
        "inputs": {"frame_paths": 1, "frame_modules": 0, "route_template": true,
                   "context_tags": 2, "ownership_rules": 3, "codeowners_rules": 0}
    });
    let server = MockServer::start().await?;
    let route = "/api/telemetry/issues/223e4567-e89b-12d3-a456-426614174001/owners";
    mount_authenticated_json(&server, "GET", route, [], &body);
    let line = "logbrew issues owners 223e4567-e89b-12d3-a456-426614174001";
    let text = successful_human_output(&server, line.split_whitespace(), "human-owners").await?;
    assert_eq!(
        text,
        format!(
            "Owners of issue {issue} (2)\nSuggested assignee: Ada Lovelace <ada@example.com>\n- Ada \
             Lovelace <ada@example.com>: ownership rule line 2 `path:src/payments/** \
             ada@example.com`, matched src/payments/charge.ts\n- Dev\\u{{1b}}[2J \
             <dev@example.com>: recent activity\nChecked: frame paths 1, modules 0, route template yes, tags 2, ownership \
             rules 3, CODEOWNERS lines 0.\nNext: run logbrew issues assign {USER_ID} {issue}\n"
        )
    );
    let json = successful_human_output(
        &server,
        "logbrew issues owners 223e4567-e89b-12d3-a456-426614174001 --json".split_whitespace(),
        "json-owners",
    )
    .await?;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json)?, body);

    let server = MockServer::start().await?;
    let none = serde_json::json!({
        "issue_id": issue, "project_id": PROJECT_ID, "suggested_assignee": null, "owners": [],
        "inputs": {"frame_paths": 0, "frame_modules": 0, "route_template": false,
                   "context_tags": 0, "ownership_rules": 0, "codeowners_rules": 0}
    });
    mount_authenticated_json(&server, "GET", route, [], &none);
    let text = successful_human_output(&server, line.split_whitespace(), "human-no-owners").await?;
    assert!(
        text.ends_with(&format!(
            "Next: add rules with logbrew issues ownership set --project {PROJECT_ID} --rules \
             <file>\n"
        )),
        "{text}"
    );
    Ok(())
});

async_test!(issue_activity_renders_actors_access_loss_and_the_next_page -> TestResult, {
    let issue = "223e4567-e89b-12d3-a456-426614174001";
    let change = "523e4567-e89b-12d3-a456-426614174005";
    let former = "323e4567-e89b-12d3-a456-42661417400b";
    let entry = |id: &str, kind: &str, value: serde_json::Value, actor: serde_json::Value| {
        serde_json::json!({
            "id": id, "type": kind, "value": value, "actor_id": actor["user_id"].clone(),
            "actor": actor, "assignee_id": null, "assignee": null,
            "at": "2026-10-19T02:00:00.000Z"
        })
    };
    let ada = serde_json::json!({"user_id": USER_ID, "name": "Ada Lovelace", "email": "ada@example.com"});
    let mut assigned = entry(change, "assigned", serde_json::Value::Null, ada);
    assigned["assignee_id"] = serde_json::json!(former);
    let mut status = entry(change, "status", serde_json::json!("archived"), serde_json::Value::Null);
    status["actor_id"] = serde_json::json!(former);
    let body = serde_json::json!({
        "issue_id": issue, "project_id": PROJECT_ID,
        "activity": [assigned, status,
                     entry(issue, "first_seen", serde_json::Value::Null, serde_json::Value::Null)],
        "next_cursor": {"time": "2026-10-19T01:00:00.000Z", "id": change}
    });
    let server = MockServer::start().await?;
    Mock::route(
        "GET",
        "/api/telemetry/issues/223e4567-e89b-12d3-a456-426614174001/activity",
    )
    .and(header("authorization", "Bearer test-token"))
    .and(query_param("limit", "3"))
    .respond_with(ResponseTemplate::new(200).set_body_json(&body))
    .mount(&server);
    let line = "logbrew issues activity 223e4567-e89b-12d3-a456-426614174001 --limit 3";
    let text = successful_human_output(&server, line.split_whitespace(), "human-activity").await?;
    assert_eq!(
        text,
        format!(
            "Activity of issue {issue} (3)\n- 2026-10-19T02:00:00.000Z assigned to {former} (no \
             access) by Ada Lovelace <ada@example.com>\n- 2026-10-19T02:00:00.000Z status set to \
             archived by {former} (no access)\n- 2026-10-19T02:00:00.000Z first seen\nMore: run \
             logbrew issues activity {issue} --cursor-time 2026-10-19T01:00:00.000Z --cursor-id \
             {change}\n"
        )
    );
    Ok(())
});

async_test!(ownership_and_severity_rules_render_and_send_their_bodies -> TestResult, {
    let issue = "223e4567-e89b-12d3-a456-426614174001";
    let log = "423e4567-e89b-12d3-a456-426614174004";
    let ownership = serde_json::json!({
        "project_id": PROJECT_ID, "rules": "# payments\npath:src/** ada@example.com\n",
        "codeowners": "", "updated_at": "2026-10-19T02:00:00.000Z",
        "unresolved_owners": ["@org/web"]
    });
    let server = MockServer::start().await?;
    mount_authenticated_json(
        &server,
        "GET",
        "/api/telemetry/issues/ownership",
        [("project_id", PROJECT_ID)],
        &ownership,
    );
    let line = "logbrew issues ownership --project 123e4567-e89b-12d3-a456-426614174000";
    let text = successful_human_output(&server, line.split_whitespace(), "human-ownership").await?;
    assert_eq!(
        text,
        format!(
            "Ownership for project {PROJECT_ID}\nUpdated: 2026-10-19T02:00:00.000Z\nOwnership rules \
             (2 lines)\n  # payments\n  path:src/** ada@example.com\nCODEOWNERS (0 lines)\nOwners \
             without project access: @org/web\n"
        )
    );

    let triage = |severity: &str, ignored: bool| {
        serde_json::json!({"effective_severity": severity, "ignored": ignored, "rule_applied": true})
    };
    let server = MockServer::start().await?;
    Mock::auth("PUT", format!("/api/telemetry/issues/{issue}/triage-rule"), "test-token")
        .and(body_json(serde_json::json!({"severity": "warning"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": issue, "severity": "error", "triage": triage("warning", false)
        })))
        .mount(&server);
    Mock::auth("PUT", format!("/api/logs/{log}/triage-rule"), "test-token")
        .and(body_json(serde_json::json!({"severity": "source", "ignored": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": log, "severity": "info", "triage": triage("info", true)
        })))
        .mount(&server);
    let text = successful_human_output(
        &server,
        "logbrew issues rule 223e4567-e89b-12d3-a456-426614174001 warning".split_whitespace(),
        "human-issue-rule",
    )
    .await?;
    assert_eq!(text, format!("Issue {issue} severity warning (captured error).\n"));
    let text = successful_human_output(
        &server,
        "logbrew logs rule 423e4567-e89b-12d3-a456-426614174004 source --ignore".split_whitespace(),
        "human-log-rule",
    )
    .await?;
    assert_eq!(text, format!("Log {log} severity info (captured info), ignored.\n"));
    Ok(())
});

async_test!(teams_render_in_owners_activity_issues_and_the_inbox -> TestResult, {
    let issue = "223e4567-e89b-12d3-a456-426614174001";
    let team_id = "523e4567-e89b-12d3-a456-426614174005";
    let team = serde_json::json!({"type": "team", "team_id": team_id, "slug": "payments", "name": "Payments"});
    let server = MockServer::start().await?;
    let mut owner = team.clone();
    owner["source"] = serde_json::json!("codeowners");
    owner["rule"] = serde_json::json!("src/ @acme/payments");
    owner["line"] = serde_json::json!(1);
    owner["matched"] = serde_json::json!("src/a.ts");
    mount_authenticated_json(
        &server,
        "GET",
        "/api/telemetry/issues/223e4567-e89b-12d3-a456-426614174001/owners",
        [("response_version", "2")],
        &serde_json::json!({
            "issue_id": issue, "project_id": PROJECT_ID, "suggested_assignee": team,
            "owners": [owner],
            "inputs": {"frame_paths": 1, "frame_modules": 0, "route_template": false,
                       "context_tags": 0, "ownership_rules": 0, "codeowners_rules": 1}
        }),
    );
    mount_authenticated_json(
        &server,
        "GET",
        "/api/telemetry/issues/notifications",
        [],
        &serde_json::json!({
            "notifications": [{
                "id": "623e4567-e89b-12d3-a456-426614174006", "issue_id": issue,
                "project_id": PROJECT_ID, "title": "Charge failed", "assignee": team,
                "actor_id": USER_ID, "actor": {"user_id": USER_ID, "name": "Ada", "email": "ada@example.com"},
                "at": "2026-10-19T02:00:00.000Z", "read": false
            }],
            "unread": 1, "read_at": null, "next_cursor": null
        }),
    );
    let text = successful_human_output(
        &server,
        "logbrew issues owners 223e4567-e89b-12d3-a456-426614174001".split_whitespace(),
        "human-team-owners",
    )
    .await?;
    assert!(text.contains("Suggested assignee: team #payments\n"), "{text}");
    assert!(text.contains(&format!("Next: run logbrew issues assign team:{team_id} {issue}\n")), "{text}");
    let text = successful_human_output(&server, "logbrew inbox".split_whitespace(), "human-inbox").await?;
    assert_eq!(
        text,
        format!(
            "Inbox (1, 1 unread)\n- 2026-10-19T02:00:00.000Z [new] Charge failed ({issue}) \
             assigned to team #payments by Ada <ada@example.com>\nNext: run logbrew inbox read\n"
        )
    );
    Ok(())
});
