//! Shared cursor collection command, response, and recovery contracts.

use super::{assert_cursor_flag_errors, assert_cursor_help, authenticated_env, run_command};
use crate::matchers::{header, query_param};
use crate::{Mock, MockServer, ResponseTemplate, execute_command};
use logbrew_cli::{parse_command, write_cli_error, write_runtime_error};
use serde_json::{Map, Value, json};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const CURSOR_ID: &str = "9b2b4b3a-bd4e-4f85-a0f6-48118f037c17";
const TRACE_ID: &str = "0123456789abcdef0123456789abcdef";
const SPAN_ID: &str = "0123456789abcdef";
const CURSOR_TIME: &str = "2026-07-13T08:00:00.123456Z";
const CURSOR_RECOVERY: &str = "send pagination=cursor alone for the first page, then send \
    cursor_time and cursor_id together from next_cursor";

#[derive(Clone, Copy)]
struct CursorSpec {
    resource: &'static str,
    title: &'static str,
    history: &'static str,
    path: &'static str,
    wrapper: &'static str,
    cursor_key: &'static str,
    cursor_id: &'static str,
    error_code: &'static str,
    help_id: &'static str,
    expected_initial: &'static str,
    expected_continuation: &'static str,
    expected_line: &'static str,
}

const SPECS: [CursorSpec; 5] = [
    CursorSpec {
        resource: "logs",
        title: "Logs",
        history: "log",
        path: "/api/logs",
        wrapper: "logs",
        cursor_key: "id",
        cursor_id: CURSOR_ID,
        error_code: "invalid_log_cursor",
        help_id: "--cursor-id <uuid>",
        expected_initial: "/api/logs?service_name=checkout-api&severity=error&search=checkout%20failed&since=24h&trace_id=trace_123&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&limit=2",
        expected_continuation: "/api/logs?service_name=checkout-api&severity=error&search=checkout%20failed&since=24h&trace_id=trace_123&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&cursor_time=2026-07-13T08%3A00%3A00.123456Z&cursor_id=9b2b4b3a-bd4e-4f85-a0f6-48118f037c17&limit=2",
        expected_line: "error checkout failed service=checkout-api trace=trace_123 [checkout@1.2.3 / production]",
    },
    CursorSpec {
        resource: "issues",
        title: "Issues",
        history: "issue",
        path: "/api/telemetry/issues",
        wrapper: "issues",
        cursor_key: "id",
        cursor_id: CURSOR_ID,
        error_code: "invalid_issue_cursor",
        help_id: "--cursor-id <uuid>",
        expected_initial: "/api/telemetry/issues?service_name=checkout-api&since=24h&status=unresolved&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&limit=2",
        expected_continuation: "/api/telemetry/issues?service_name=checkout-api&since=24h&status=unresolved&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&cursor_time=2026-07-13T08%3A00%3A00.123456Z&cursor_id=9b2b4b3a-bd4e-4f85-a0f6-48118f037c17&limit=2",
        expected_line: "9b2b4b3a-bd4e-4f85-a0f6-48118f037c17 unresolved error PaymentError occurrences=2 last_seen=2026-07-13T08:00:00.123456Z service=checkout-api trace=trace_123 [checkout@1.2.3 / production]",
    },
    CursorSpec {
        resource: "actions",
        title: "Actions",
        history: "action",
        path: "/api/telemetry/actions",
        wrapper: "actions",
        cursor_key: "id",
        cursor_id: CURSOR_ID,
        error_code: "invalid_action_cursor",
        help_id: "--cursor-id <uuid>",
        expected_initial: "/api/telemetry/actions?service_name=checkout-api&name=checkout_failed&since=24h&distinct_id=user_123&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&limit=2",
        expected_continuation: "/api/telemetry/actions?service_name=checkout-api&name=checkout_failed&since=24h&distinct_id=user_123&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&cursor_time=2026-07-13T08%3A00%3A00.123456Z&cursor_id=9b2b4b3a-bd4e-4f85-a0f6-48118f037c17&limit=2",
        expected_line: "checkout_failed error service=checkout-api user=user_123 trace=trace_123 [checkout@1.2.3 / production]",
    },
    CursorSpec {
        resource: "metrics",
        title: "Metrics",
        history: "metric",
        path: "/api/telemetry/metrics",
        wrapper: "metrics",
        cursor_key: "id",
        cursor_id: CURSOR_ID,
        error_code: "invalid_metric_cursor",
        help_id: "--cursor-id <uuid>",
        expected_initial: "/api/telemetry/metrics?name=http.server.duration&service_name=checkout-api&since=24h&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&limit=2",
        expected_continuation: "/api/telemetry/metrics?name=http.server.duration&service_name=checkout-api&since=24h&project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&pagination=cursor&cursor_time=2026-07-13T08%3A00%3A00.123456Z&cursor_id=9b2b4b3a-bd4e-4f85-a0f6-48118f037c17&limit=2",
        expected_line: "http.server.duration histogram value=12.5 unit=ms temporality=delta service=checkout-api trace=0123456789abcdef0123456789abcdef span=0123456789abcdef occurred=2026-07-13T08:00:00.123456Z sdk=@logbrew/node@0.1.8 [checkout@1.2.3 / production]",
    },
    CursorSpec {
        resource: "traces",
        title: "Traces",
        history: "trace",
        path: "/api/telemetry/traces",
        wrapper: "traces",
        cursor_key: "trace_id",
        cursor_id: TRACE_ID,
        error_code: "invalid_trace_cursor",
        help_id: "--cursor-id <trace_id>",
        expected_initial: "/api/telemetry/traces?project_id=123e4567-e89b-12d3-a456-426614174000&service_name=checkout-api&release=checkout%401.2.3&environment=production&status=error&since=24h&min_duration_ms=10&pagination=cursor&limit=2",
        expected_continuation: "/api/telemetry/traces?project_id=123e4567-e89b-12d3-a456-426614174000&service_name=checkout-api&release=checkout%401.2.3&environment=production&status=error&since=24h&min_duration_ms=10&pagination=cursor&cursor_time=2026-07-13T08%3A00%3A00.123456Z&cursor_trace_id=0123456789abcdef0123456789abcdef&limit=2",
        expected_line: "0123456789abcdef0123456789abcdef error GET /checkout service=checkout-api operation=http.server spans=3 errors=1 services=1 duration=45ms started=2026-07-13T08:00:00.123456Z",
    },
];

#[test]
fn cursor_pages_preserve_exact_filters_and_backend_cursor_keys() {
    for spec in SPECS {
        let initial = parse_command(command_args(spec, false)).expect("first page parses");
        assert_eq!(initial.http_path().as_deref(), Some(spec.expected_initial));
        let continuation = parse_command(command_args(spec, true)).expect("continuation parses");
        assert_eq!(
            continuation.http_path().as_deref(),
            Some(spec.expected_continuation)
        );
    }
}

#[test]
fn cursor_flags_and_help_are_resource_specific() {
    for spec in SPECS {
        assert_cursor_flag_errors(spec.resource, CURSOR_TIME, spec.cursor_id, spec.error_code);
        assert_cursor_help(spec.resource, spec.help_id);
    }
    let error = parse_command(["logbrew", "releases", "--pagination", "cursor", "--json"])
        .expect_err("releases do not invent cursor support");
    let mut output = Vec::new();
    write_cli_error(&error, true, &mut output).expect("error writes");
    let body: Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(body["error"], "unsupported_flag");
    assert_eq!(body["next"], "run logbrew read releases --help");
}

async_test!(json_preserves_legacy_arrays_and_cursor_envelopes -> Result<(), Box<dyn std::error::Error>>, {
    for spec in SPECS {
        let legacy_server = MockServer::start().await;
        let cursor_server = MockServer::start().await;
        let row = row_value(spec);
        let envelope = page_value(spec, row.clone(), Some(cursor_value(spec)));
        Mock::auth("GET", spec.path, "test-token")
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([row.clone()])))
            .mount(&legacy_server);
        Mock::route("GET", spec.path)
            .and(query_param("pagination", "cursor"))
            .and(query_param("limit", "1"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(envelope.clone()))
            .mount(&cursor_server);

        let legacy = run_command(
            &legacy_server,
            ["logbrew", spec.resource, "--json"],
            format!("{}-legacy-json", spec.resource).as_str(),
        )
        .await?;
        assert_eq!(serde_json::from_str::<Value>(&legacy)?, json!([row]));
        let cursor = run_command(
            &cursor_server,
            [
                "logbrew",
                spec.resource,
                "--pagination",
                "cursor",
                "--limit",
                "1",
                "--json",
            ],
            format!("{}-cursor-json", spec.resource).as_str(),
        )
        .await?;
        assert_eq!(serde_json::from_str::<Value>(&cursor)?, envelope);
    }
    Ok(())
});

async_test!(human_pages_keep_rows_receipts_and_retryable_continuations -> Result<(), Box<dyn std::error::Error>>, {
    for spec in SPECS {
        let server = MockServer::start().await;
        Mock::route("GET", spec.path)
            .and(query_param("pagination", "cursor"))
            .and(query_param("limit", "1"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_value(
                spec,
                row_value(spec),
                Some(cursor_value(spec)),
            )))
            .mount(&server);
        let human = run_command(
            &server,
            [
                "logbrew",
                spec.resource,
                "--pagination",
                "cursor",
                "--limit",
                "1",
            ],
            format!("{}-cursor-human", spec.resource).as_str(),
        )
        .await?;

        assert!(
            human.starts_with(format!("{} (1)\n- {}\n", spec.title, spec.expected_line).as_str())
        );
        assert!(
            human.contains(
                format!(
                    "Next page: set --cursor-time {CURSOR_TIME} --cursor-id {} on the same command",
                    spec.cursor_id
                )
                .as_str()
            )
        );
        assert!(
            human.ends_with("Retry: rerun that same command; the rows above remain visible.\n")
        );
        if spec.resource == "traces" {
            assert!(human.contains("Naming quality: evaluated=1 meaningful=1 generic=0 unmatched=0 generic_operations=0 truncated=true\n"));
        }
        for private in ["raw issue message", "stack sentinel", "attribute sentinel"] {
            assert!(!human.contains(private));
        }
    }
    Ok(())
});

async_test!(terminal_pages_and_replaced_continuations_are_explicit -> Result<(), Box<dyn std::error::Error>>, {
    for spec in SPECS {
        let terminal = MockServer::start().await;
        Mock::route("GET", spec.path)
            .and(query_param("pagination", "cursor"))
            .and(query_param("cursor_time", CURSOR_TIME))
            .and(query_param(
                if spec.resource == "traces" {
                    "cursor_trace_id"
                } else {
                    "cursor_id"
                },
                spec.cursor_id,
            ))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_value(
                spec,
                row_value(spec),
                None,
            )))
            .mount(&terminal);
        let human = run_command(
            &terminal,
            [
                "logbrew",
                spec.resource,
                "--pagination",
                "cursor",
                "--cursor-time",
                CURSOR_TIME,
                "--cursor-id",
                spec.cursor_id,
            ],
            format!("{}-cursor-terminal", spec.resource).as_str(),
        )
        .await?;
        assert!(human.ends_with(format!("End of {} history.\n", spec.history).as_str()));
        assert!(!human.contains("Next page:"));
    }

    let spec = SPECS[2];
    let server = MockServer::start().await;
    let next_time = "2026-07-12T07:00:00.250+00:00";
    let next_id = "b7d388c7-c486-420b-970f-0126b7e649cb";
    Mock::route("GET", spec.path)
        .and(query_param("pagination", "cursor"))
        .and(query_param("cursor_time", CURSOR_TIME))
        .and(query_param("cursor_id", CURSOR_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_value(
            spec,
            row_value(spec),
            Some(json!({"time": next_time, "id": next_id})),
        )))
        .mount(&server);
    let human = run_command(
        &server,
        [
            "logbrew",
            spec.resource,
            "--pagination",
            "cursor",
            "--cursor-time",
            CURSOR_TIME,
            "--cursor-id",
            spec.cursor_id,
        ],
        "cursor-replacement",
    )
    .await?;
    assert!(human.contains(
        format!("Next page: set --cursor-time {next_time} --cursor-id {next_id}").as_str()
    ));
    assert!(!human.contains("Next page: add"));
    Ok(())
});

async_test!(every_cursor_resource_hides_malformed_or_non_json_successes -> Result<(), Box<dyn std::error::Error>>, {
    for spec in SPECS {
        for (suffix, body) in [("missing", Some(wrapper_only(spec))), ("text", None)] {
            let server = MockServer::start().await;
            let response = body.map_or_else(
                || {
                    ResponseTemplate::new(200)
                        .set_body_raw("not-json\nNext page: unsafe response", "text/plain")
                },
                |body| ResponseTemplate::new(200).set_body_json(body),
            );
            Mock::route("GET", spec.path)
                .and(query_param("pagination", "cursor"))
                .respond_with(response)
                .mount(&server);
            let human = run_command(
                &server,
                ["logbrew", spec.resource, "--pagination", "cursor"],
                format!("{}-{suffix}", spec.resource).as_str(),
            )
            .await?;
            assert_eq!(human, invalid_response(spec.title));
            assert!(!human.contains("unsafe response"));
        }
    }
    Ok(())
});

async_test!(malformed_cursor_values_and_issue_rows_fail_closed -> Result<(), Box<dyn std::error::Error>>, {
    let spec = SPECS[0];
    for cursor in [
        json!({"time": 123, "id": CURSOR_ID}),
        json!({"time": "not-rfc3339", "id": CURSOR_ID}),
        json!({"time": CURSOR_TIME, "id": "invalid-id\nunsafe cursor"}),
    ] {
        assert_invalid_page(spec, page_value(spec, row_value(spec), Some(cursor))).await?;
    }

    let issue = SPECS[1];
    for key in ["title", "occurrence_count", "last_seen_at"] {
        let mut row = row_value(issue);
        match key {
            "title" => {
                drop(row.as_object_mut().expect("row object").remove(key));
            }
            "occurrence_count" => row[key] = json!("two"),
            _ => row[key] = json!("not-rfc3339"),
        }
        assert_invalid_page(issue, page_value(issue, row, None)).await?;
    }
    Ok(())
});

async_test!(metric_human_output_never_falls_back_to_attributes -> Result<(), Box<dyn std::error::Error>>, {
    let spec = SPECS[3];
    let mut malformed = row_value(spec);
    malformed["occurred_at"] = json!("not-a-time");
    for (cursor, body) in [
        (false, json!([malformed.clone()])),
        (true, page_value(spec, malformed, None)),
    ] {
        let server = MockServer::start().await;
        Mock::route("GET", spec.path)
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server);
        let human = if cursor {
            run_command(
                &server,
                ["logbrew", "metrics", "--pagination", "cursor"],
                "metric-malformed-cursor",
            )
            .await?
        } else {
            run_command(&server, ["logbrew", "metrics"], "metric-malformed-list").await?
        };
        assert_eq!(
            human,
            if cursor {
                invalid_response("Metrics")
            } else {
                String::from(
                    "Metrics response could not be rendered safely.\nNext: retry the same command with --json and inspect the public response shape.\n",
                )
            }
        );
        assert!(!human.contains("attribute sentinel"));
    }
    Ok(())
});

async_test!(trace_naming_receipt_is_exact_and_actionable -> Result<(), Box<dyn std::error::Error>>, {
    let spec = SPECS[4];
    let server = MockServer::start().await;
    let mut weak_trace = row_value(spec);
    weak_trace["root_span_name"] = json!("GET unmatched");
    weak_trace["root_operation"] = json!("sdk.span");
    let mut page = page_value(spec, weak_trace, None);
    page["naming_quality"] = trace_quality(1, 0, 0, 1, 1, false, true);
    Mock::route("GET", spec.path)
        .respond_with(ResponseTemplate::new(200).set_body_json(page))
        .mount(&server);
    let human = run_command(
        &server,
        ["logbrew", "traces", "--pagination", "cursor"],
        "trace-quality-guidance",
    )
    .await?;
    assert!(human.contains("Naming quality: evaluated=1 meaningful=0 generic=0 unmatched=1 generic_operations=1 truncated=false\n"));
    assert!(human.contains("Naming action: improve_trace_naming target=sdk_configuration\n"));

    for quality in [
        trace_quality(2, 1, 0, 0, 0, false, false),
        trace_quality(1, 1, 1, 0, 0, false, true),
        trace_quality(1, 0, 1, 0, 0, false, true),
        trace_quality(1, 1, 0, 0, 2, false, true),
        trace_quality(1, 1, 0, 0, 0, true, false),
        trace_quality(1, 1, 0, 0, 0, false, true),
    ] {
        let mut invalid = page_value(spec, row_value(spec), None);
        invalid["naming_quality"] = quality;
        assert_invalid_page(spec, invalid).await?;
    }
    Ok(())
});

async_test!(backend_cursor_validation_is_preserved_without_echoing_values -> Result<(), Box<dyn std::error::Error>>, {
    let spec = SPECS[3];
    let server = MockServer::start().await;
    Mock::route("GET", spec.path)
        .and(query_param("pagination", "cursor"))
        .and(query_param("cursor_time", "not-a-time"))
        .and(query_param("cursor_id", CURSOR_ID))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "error": "invalid cursor pagination",
            "code": "validation_failed",
            "next": CURSOR_RECOVERY,
            "next_action": {"code": "fix_request", "target": "request"}
        })))
        .mount(&server);
    let command = parse_command([
        "logbrew",
        "metrics",
        "--pagination",
        "cursor",
        "--cursor-time",
        "not-a-time",
        "--cursor-id",
        CURSOR_ID,
        "--json",
    ])?;
    let env = authenticated_env(&server, "test-token", Some("metric-cursor-invalid"));
    let mut output = Vec::new();
    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("invalid backend cursor fails");
    write_runtime_error(&error, true, &mut output)?;
    let body: Value = serde_json::from_slice(&output)?;
    assert_eq!(body["status"], 422);
    assert_eq!(body["api_code"], "validation_failed");
    assert_eq!(body["next"], CURSOR_RECOVERY);
    assert!(!String::from_utf8_lossy(&output).contains("not-a-time"));
    Ok(())
});

fn command_args(spec: CursorSpec, continuation: bool) -> Vec<&'static str> {
    let mut args = vec!["logbrew", spec.resource];
    match spec.resource {
        "logs" => args.extend([
            "--level",
            "error",
            "--search",
            "checkout failed",
            "--trace-id",
            "trace_123",
        ]),
        "issues" => args.extend(["--status", "open"]),
        "actions" => args.extend(["--name", "checkout_failed", "--distinct-id", "user_123"]),
        "metrics" => args.extend(["--name", "http.server.duration"]),
        "traces" => args.extend(["--status", "error", "--min-duration-ms", "10"]),
        _ => unreachable!(),
    }
    args.extend([
        if continuation {
            "--project"
        } else {
            "--project-id"
        },
        PROJECT_ID,
        if continuation {
            "--service"
        } else {
            "--service-name"
        },
        "checkout-api",
        "--release",
        "checkout@1.2.3",
        if continuation {
            "--environment"
        } else {
            "--env"
        },
        "production",
        "--since",
        "24h",
        "--pagination",
        "cursor",
    ]);
    if continuation {
        args.extend(["--cursor-time", CURSOR_TIME, "--cursor-id", spec.cursor_id]);
    }
    args.extend(["--limit", "2", "--json"]);
    args
}

fn row_value(spec: CursorSpec) -> Value {
    match spec.resource {
        "logs" => json!({
            "message": "checkout failed", "severity": "error", "service_name": "checkout-api",
            "trace_id": "trace_123", "release": "checkout@1.2.3", "environment": "production"
        }),
        "issues" => json!({
            "id": CURSOR_ID, "project_id": PROJECT_ID, "fingerprint": "payment-error",
            "status": "unresolved", "severity": "error", "title": "PaymentError",
            "message": "raw issue message", "stack_trace": "stack sentinel",
            "attributes": {"debug": "attribute sentinel"}, "occurrence_count": 2,
            "service_name": "checkout-api", "trace_id": "trace_123",
            "release": "checkout@1.2.3", "environment": "production",
            "first_seen_at": "2026-07-13T07:00:00Z", "last_seen_at": CURSOR_TIME,
            "next_action": {"code": "inspect_trace", "target": "trace_summary"}
        }),
        "actions" => json!({
            "name": "checkout_failed", "severity": "error", "service_name": "checkout-api",
            "distinct_id": "user_123", "trace_id": "trace_123",
            "release": "checkout@1.2.3", "environment": "production"
        }),
        "metrics" => json!({
            "id": CURSOR_ID, "event_id": "metric-1", "project_id": PROJECT_ID,
            "name": "http.server.duration", "kind": "histogram", "value": 12.5,
            "unit": "ms", "temporality": "delta", "attributes": {"private": "attribute sentinel"},
            "occurred_at": CURSOR_TIME, "environment": "production", "release": "checkout@1.2.3",
            "trace_id": TRACE_ID, "span_id": SPAN_ID, "service_name": "checkout-api",
            "sdk_name": "@logbrew/node", "sdk_version": "0.1.8"
        }),
        "traces" => json!({
            "trace_id": TRACE_ID, "project_ids": [PROJECT_ID], "root_span_name": "GET /checkout",
            "root_service_name": "checkout-api", "root_operation": "http.server",
            "span_count": 3, "error_span_count": 1, "service_count": 1,
            "started_at": CURSOR_TIME, "duration_ms": 45, "services": ["checkout-api"],
            "releases": ["checkout@1.2.3"], "environments": ["production"],
            "next_action": {"code": "inspect_trace", "target": "trace_summary"}
        }),
        _ => unreachable!(),
    }
}

fn cursor_value(spec: CursorSpec) -> Value {
    let mut cursor = Map::new();
    drop(cursor.insert("time".into(), json!(CURSOR_TIME)));
    drop(cursor.insert(spec.cursor_key.into(), json!(spec.cursor_id)));
    Value::Object(cursor)
}

fn page_value(spec: CursorSpec, row: Value, next_cursor: Option<Value>) -> Value {
    let mut page = Map::new();
    drop(page.insert(spec.wrapper.into(), json!([row])));
    drop(page.insert("next_cursor".into(), next_cursor.unwrap_or(Value::Null)));
    if spec.resource == "traces" {
        drop(page.insert(
            "naming_quality".into(),
            trace_quality(1, 1, 0, 0, 0, page["next_cursor"] != Value::Null, false),
        ));
    }
    Value::Object(page)
}

fn wrapper_only(spec: CursorSpec) -> Value {
    let mut page = Map::new();
    drop(page.insert(spec.wrapper.into(), json!([row_value(spec)])));
    Value::Object(page)
}

fn trace_quality(
    evaluated: u64,
    meaningful: u64,
    generic: u64,
    unmatched: u64,
    generic_operations: u64,
    truncated: bool,
    action: bool,
) -> Value {
    json!({
        "evaluated_traces": evaluated,
        "meaningful_name_traces": meaningful,
        "generic_name_traces": generic,
        "unmatched_route_traces": unmatched,
        "generic_operation_traces": generic_operations,
        "truncated": truncated,
        "next_action": action.then(|| json!({
            "code": "improve_trace_naming", "target": "sdk_configuration"
        }))
    })
}

async fn assert_invalid_page(
    spec: CursorSpec,
    body: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::route("GET", spec.path)
        .and(query_param("pagination", "cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server);
    let human = run_command(
        &server,
        ["logbrew", spec.resource, "--pagination", "cursor"],
        format!("{}-invalid", spec.resource).as_str(),
    )
    .await?;
    assert_eq!(human, invalid_response(spec.title));
    assert!(!human.contains("unsafe cursor"));
    Ok(())
}

fn invalid_response(title: &str) -> String {
    format!(
        "{title} response could not be rendered safely.\nNext: retry the same command with --json and inspect next_cursor.\n"
    )
}
