//! Versioned issue, log, action, trace, release, and metric explanation contracts.

use logbrew_cli::{CliEnvironment, execute_command, parse_command, write_runtime_error};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ISSUE_ID: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const LOG_ID: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const SPAN_ID: &str = "00f067aa0ba902b7";

#[tokio::test]
async fn human_issue_explanation_surfaces_fix_context_timeline_and_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/telemetry/issues/{ISSUE_ID}/investigation"
        )))
        .and(query_param("response_version", "2"))
        .and(query_param("selection", "recommended"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue_response()))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "explain", "issue", ISSUE_ID])?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    for expected in [
        "Issue cccccccc-cccc-4ccc-8ccc-cccccccccccc unresolved severity=error",
        "Occurrence selection: requested=recommended reason=context_rich_recent_occurrence \
         selected=dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        "Exception: PaymentError mechanism=unhandled handled=false",
        "Frame: module=checkout function=charge file=payment.rs line=42 column=7 in_app=true",
        "Breadcrumb: at=2026-08-03T11:04:55Z category=ui.click",
        "Runtime: service=checkout-api@1.2.3 runtime=rust@1.88",
        "Captured correlation: trace=4bf92f3577b34da6a3ce929d0e0e4736",
        "Tag: plan=pro",
        "Cause assessment: status=evidence_only",
        "Fix area: status=observed_application_frame provenance=backend_observed",
        "Impact: occurrences=3",
        "Known affected users: not captured in retained issue context.",
        "User-impact coverage: retained=3 indexed=0 identified=0 anonymous=0 missing=0 \
         privacy_filtered=0 historical_unindexed=3 index=0.00%",
        "User-impact limitations: historical_occurrences_unindexed",
        "Related logs: status=available count=1",
        "Evidence: status=partial",
        "Next 1: code=inspect_code_location target=source_code reason=likely_fix_location_available",
    ] {
        assert!(text.contains(expected), "missing issue detail: {expected}");
    }
    Ok(())
}

#[tokio::test]
async fn human_release_explanation_connects_health_sdk_and_every_signal()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/releases/investigation"))
        .and(query_param("project_id", PROJECT_ID))
        .and(query_param("release", "checkout@1.2.3"))
        .and(query_param("environment", "production"))
        .and(query_param("service_name", "checkout-api"))
        .and(query_param("response_version", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(release_response()))
        .mount(&server)
        .await;
    let command = release_command(false)?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    for expected in [
        "Release checkout@1.2.3 status=failures_observed causality=evidence_only",
        "Signals: issues=1 logs=1 spans=2 actions=4 metrics=3",
        "Trace health: status=available traces=1 error_traces=1 error_rate_bps=10000",
        "SDK: name=logbrew-rust version=1.2.3 stream=issues items=1",
        "Release issues: status=available count=1",
        "High-severity logs: status=available count=1",
        "Log: message=processor rejected charge level=error",
        "Action cardinality: unique_counts_approximate=true method=approximate_uniq_combined64",
        "Action: name=checkout.submit events=4 known_users=~2 anonymous_subjects=~1 sessions=~3",
        "Action subject coverage: index_version=1 typed_user_events=2 anonymous_events=1 \
         legacy_unknown_events=0 missing_events=1 historical_unindexed_events=0",
        "Metric: name=checkout.duration kind=histogram temporality=delta latest=240 min=120 max=300 average=220 events=3",
        "Timeline item: at=2026-08-03T11:05:00Z kind=issue summary=Payment failed",
        "Comparison: status=unavailable reason=deployment_boundary_not_captured",
        "Next 1: code=inspect_release_issue target=issue_investigation",
    ] {
        assert!(
            text.contains(expected),
            "missing release detail: {expected}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn built_binary_release_preserves_validated_version_2_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = release_response();
    Mock::given(method("GET"))
        .and(path("/api/telemetry/releases/investigation"))
        .and(query_param("response_version", "2"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let base_url = server.uri();
    let process = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_logbrew"))
            .env_clear()
            .env("HOME", std::env::temp_dir())
            .env("LOGBREW_API_URL", base_url)
            .env("LOGBREW_TOKEN", "test-token")
            .args([
                "explain",
                "release",
                "checkout@1.2.3",
                "--project",
                PROJECT_ID,
                "--environment",
                "production",
                "--service",
                "checkout-api",
                "--json",
            ])
            .output()
    })
    .await??;

    assert!(
        process.status.success(),
        "built binary failed: {}",
        String::from_utf8_lossy(process.stderr.as_slice())
    );
    assert!(process.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(process.stdout.as_slice())?;
    assert_eq!(actual, response);
    Ok(())
}

#[tokio::test]
async fn release_explanation_rejects_contradictory_or_unversioned_subject_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut contradictory_partition = release_response();
    contradictory_partition["signals"]["actions"]["items"][0]["subject_coverage"]["missing_subject_events"] =
        serde_json::json!(2);
    let mut impossible_cardinality = release_response();
    impossible_cardinality["signals"]["actions"]["items"][0]["identified_user_count"] =
        serde_json::json!(3);
    let mut unlabelled_estimate = release_response();
    unlabelled_estimate["signals"]["actions"]["estimation"]["unique_counts_are_approximate"] =
        serde_json::json!(false);
    let mut legacy_schema = release_response();
    legacy_schema["schema_version"] = serde_json::json!(1);
    let mut unsafe_count = release_response();
    unsafe_count["signals"]["actions"]["items"][0]["event_count"] =
        serde_json::json!(9_007_199_254_740_992_u64);
    let mut missing_evidence_receipt = release_response();
    missing_evidence_receipt["evidence"]["captured_fields"] =
        serde_json::json!(["release.issues", "release.traces"]);
    let mut legacy_priority = release_response();
    legacy_priority["next_actions"][0]["priority"] = serde_json::json!(1);
    let mut mismatched_release_action = release_response();
    mismatched_release_action["next_actions"][0]["issue_id"] =
        serde_json::json!("dddddddd-dddd-4ddd-8ddd-dddddddddddd");
    let mut missing_boundary_action = release_response();
    missing_boundary_action["next_actions"]
        .as_array_mut()
        .ok_or("next-actions fixture")?
        .retain(|action| action["code"] != "capture_deployment_boundary");

    for response in [
        contradictory_partition,
        impossible_cardinality,
        unlabelled_estimate,
        legacy_schema,
        unsafe_count,
        missing_evidence_receipt,
        legacy_priority,
        mismatched_release_action,
        missing_boundary_action,
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/telemetry/releases/investigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
        let command = release_command(true)?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("contradictory release subject contract fails closed");

        assert!(matches!(
            error,
            logbrew_cli::RuntimeError::ExplainResponseInvalid
        ));
        assert!(output.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn metric_explanation_preserves_validated_json_and_exact_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = metric_response();
    Mock::given(method("GET"))
        .and(path("/api/telemetry/metrics/investigation"))
        .and(query_param("project_id", PROJECT_ID))
        .and(query_param("name", "http.server.duration"))
        .and(query_param("since", "24h"))
        .and(query_param("interval", "5m"))
        .and(query_param("group_by", "service_name"))
        .and(query_param("environment", "production"))
        .and(query_param("series_limit", "12"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let command = metric_command(true)?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;

    let actual: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(actual, response);
    Ok(())
}

#[tokio::test]
async fn human_metric_explanation_exposes_semantics_coverage_and_trace_follow_up()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/metrics/investigation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(metric_response()))
        .mount(&server)
        .await;
    let command = metric_command(false)?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Metric http.server.duration"));
    assert!(text.contains("Metric definition: status=not_captured"));
    assert!(text.contains("Content trust: application metric names and values are untrusted"));
    assert!(text.contains("Analysis: status=change_observed causality=evidence_only"));
    assert!(text.contains("Latest raw sample: status=available"));
    assert!(text.contains("Raw sample: kind=histogram value=48 unit=ms temporality=delta"));
    assert!(text.contains("Aggregation: code=distribution_p95"));
    assert!(text.contains("p50=20"));
    assert!(text.contains("p95=48"));
    assert!(text.contains("p99=60"));
    assert!(
        text.contains("Comparison: status=available method=adjacent_equal_window_latest_bucket")
    );
    assert!(text.contains("direction=increased current=48"));
    assert!(text.contains("previous=24"));
    assert!(text.contains("Exemplars: status=available"));
    assert!(text.contains("trace_linked=1 span_linked=1 returned=1"));
    assert!(text.contains("span=00f067aa0ba902b7"));
    assert!(text.contains("Deployment overlays: status=available count=1"));
    assert!(text.contains("Metric timeline: count=2"));
    assert!(text.contains("Evidence: status=partial"));
    assert!(text.contains("Missing: metric.description"));
    assert!(text.contains("Next 1: code=inspect_exact_span target=span_investigation"));
    Ok(())
}

#[tokio::test]
async fn metric_latest_sample_preserves_rich_context_and_redaction_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = metric_response();
    response["latest_sample"]["sample"]["context"] = serde_json::json!({
        "schema_version": 1,
        "resource": {
            "service": {"name": "checkout-api", "version": "1.2.3"},
            "deployment": {"environment": "production", "release": "checkout@1.2.3"},
            "runtime": {"name": "rust", "version": "1.88"},
            "framework": {"name": "axum", "version": "0.8"},
            "operating_system": {"name": "linux", "version": "6.8", "build": null},
            "device": {"family": "server", "model": null, "architecture": "arm64"},
            "application": {"name": "checkout", "version": "1.2.3", "build": "42"}
        },
        "trace": {
            "trace_id": TRACE_ID,
            "span_id": SPAN_ID,
            "parent_span_id": null,
            "sampled": true
        },
        "session": {"id": "session-proof-1", "previous_id": null},
        "subject": {"id": "subject-proof-1", "kind": "anonymous"},
        "tags": {"journey": "checkout", "proof.channel": "hosted"}
    });
    response["latest_sample"]["sample"]["metadata"] = serde_json::json!({
        "values": {"result": "accepted", "retry_count": 0},
        "included_leaf_count": 2,
        "redacted": true,
        "truncated": false
    });
    let captured = response["evidence"]["captured_fields"]
        .as_array_mut()
        .ok_or("captured fixture")?;
    captured.extend(
        [
            "metric.latest_sample.context",
            "metric.latest_sample.context.resource",
            "metric.latest_sample.context.session",
            "metric.latest_sample.context.subject",
            "metric.latest_sample.context.tags",
            "metric.latest_sample.context.trace",
            "metric.latest_sample.metadata",
            "metric.latest_sample.metadata.result",
            "metric.latest_sample.metadata.retry_count",
        ]
        .into_iter()
        .map(|value| serde_json::json!(value)),
    );
    captured.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    response["evidence"]["missing_fields"]
        .as_array_mut()
        .ok_or("missing fixture")?
        .retain(|field| {
            !matches!(
                field.as_str(),
                Some("metric.latest_sample.context" | "metric.latest_sample.metadata")
            )
        });
    response["evidence"]["redacted_fields"] =
        serde_json::json!(["metric.latest_sample.metadata.authorization"]);
    Mock::given(method("GET"))
        .and(path("/api/telemetry/metrics/investigation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(&server)
        .await;
    let command = metric_command(false)?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Runtime: service=checkout-api@1.2.3 runtime=rust@1.88"));
    assert!(text.contains("Captured correlation: trace=4bf92f3577b34da6a3ce929d0e0e4736"));
    assert!(text.contains("Tag: proof.channel=hosted"));
    assert!(text.contains("Raw sample metadata: fields=2 redacted=true truncated=false"));
    assert!(text.contains("Raw sample field: result=accepted"));
    assert!(!text.contains("authorization="));
    Ok(())
}

#[tokio::test]
async fn empty_metric_investigation_stays_truthful_and_actionable()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/metrics/investigation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_metric_response()))
        .mount(&server)
        .await;
    let command = metric_command(false)?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Analysis: status=no_samples causality=evidence_only"));
    assert!(text.contains("No metric series matched this exact bounded query."));
    assert!(text.contains("Comparison: status=no_current_samples"));
    assert!(text.contains("Exemplars: status=not_found"));
    assert!(text.contains("Deployment overlays: status=not_found count=0"));
    assert!(text.contains("Next 1: code=verify_metric_capture target=metric_capture"));
    Ok(())
}

#[tokio::test]
async fn unknown_or_duplicate_versions_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    for (body, marker) in [
        (
            serde_json::json!({
                "schema_version": 2,
                "query": {},
                "purpose": "hostile-version-marker",
                "coverage": {},
                "series": [],
                "next_action": {}
            })
            .to_string(),
            "hostile-version-marker",
        ),
        (
            String::from(
                "{\"schema_version\":1,\"schema_version\":2,\"query\":{},\"purpose\":\"hostile-duplicate-marker\",\"coverage\":{},\"series\":[],\"next_action\":{}}",
            ),
            "hostile-duplicate-marker",
        ),
        (
            String::from(
                "{\"schema_version\":1,\"query\":{\"project_id\":\"hostile-nested-marker\",\"project_id\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"},\"purpose\":\"nested duplicate\",\"coverage\":{},\"series\":[],\"next_action\":{}}",
            ),
            "hostile-nested-marker",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/telemetry/metrics/investigation"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let command = metric_command(true)?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("unknown contract fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let response: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(response["error"], "explain_response_invalid");
        assert!(!text.contains(marker));
    }
    Ok(())
}

#[tokio::test]
async fn explanation_rejects_identity_mismatch_and_hostile_api_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let mismatch = MockServer::start().await;
    let mut response = metric_response();
    response["query"]["project_id"] = serde_json::json!("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    Mock::given(method("GET"))
        .and(path("/api/telemetry/metrics/investigation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(&mismatch)
        .await;
    let command = metric_command(true)?;
    let mut output = Vec::new();
    let error = execute_command(&command, &authenticated_env(&mismatch), &mut output)
        .await
        .expect_err("mismatched identity fails closed");
    assert!(matches!(
        error,
        logbrew_cli::RuntimeError::ExplainResponseInvalid
    ));

    let rejected = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/metrics/investigation"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_string("hostile-api-marker test-token private upstream detail"),
        )
        .mount(&rejected)
        .await;
    let mut output = Vec::new();
    let error = execute_command(&command, &authenticated_env(&rejected), &mut output)
        .await
        .expect_err("rejected request is fixed locally");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("validation_failed"));
    assert!(!text.contains("hostile-api-marker"));
    assert!(!text.contains("test-token"));
    assert!(!text.contains("private upstream detail"));
    Ok(())
}

#[tokio::test]
async fn metric_explanation_rejects_contradictory_semantics_and_group_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut contradictory_semantics = metric_response();
    contradictory_semantics["series"][0]["identity"]["kind"] = serde_json::json!("counter");
    let mut contradictory_group = metric_response();
    contradictory_group["series"][0]["identity"]["group_by"] = serde_json::json!("environment");
    let mut contradictory_change = metric_response();
    contradictory_change["comparison"]["items"][0]["absolute_change"] = serde_json::json!(12.0);
    let mut contradictory_linkage = metric_response();
    contradictory_linkage["exemplars"]["coverage"]["span_linked_samples"] = serde_json::json!(0);
    let mut mismatched_span_action = metric_response();
    mismatched_span_action["next_actions"][0]["context"]["span_id"] =
        serde_json::json!("1111111111111111");
    let mut nondeterministic_focus = metric_response();
    nondeterministic_focus["analysis"]["focus"]["selection"] =
        serde_json::json!("largest_absolute_change");
    let mut omitted_timeline_evidence = metric_response();
    omitted_timeline_evidence["timeline"]["items"] = serde_json::json!([]);
    let mut invented_evidence_receipt = metric_response();
    invented_evidence_receipt["evidence"]["captured_fields"] = serde_json::json!([
        "metric.current_window_coverage",
        "metric.deployment_overlays",
        "metric.identity",
        "metric.prior_window_comparison",
        "metric.query_scope",
        "metric.series_semantics",
        "metric.span_exemplars",
        "metric.trace_exemplars",
        "metric.user_identity"
    ]);
    let mut missing_latest_sample = metric_response();
    missing_latest_sample["latest_sample"]["sample"] = serde_json::Value::Null;
    let mut mismatched_latest_trace = metric_response();
    mismatched_latest_trace["latest_sample"]["sample"]["context"] = serde_json::json!({
        "schema_version": 1,
        "resource": null,
        "trace": {
            "trace_id": "11111111111111111111111111111111",
            "span_id": SPAN_ID,
            "parent_span_id": null,
            "sampled": true
        },
        "session": null,
        "subject": null,
        "tags": {}
    });
    let mut incorrect_latest_leaf_receipt = metric_response();
    incorrect_latest_leaf_receipt["latest_sample"]["sample"]["metadata"] = serde_json::json!({
        "values": {"result": "accepted"},
        "included_leaf_count": 2,
        "redacted": false,
        "truncated": false
    });

    for response in [
        contradictory_semantics,
        contradictory_group,
        contradictory_change,
        contradictory_linkage,
        mismatched_span_action,
        nondeterministic_focus,
        omitted_timeline_evidence,
        invented_evidence_receipt,
        missing_latest_sample,
        mismatched_latest_trace,
        incorrect_latest_leaf_receipt,
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/telemetry/metrics/investigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
        let command = metric_command(true)?;
        let mut output = Vec::new();

        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("contradictory metric contract fails closed");

        assert!(matches!(
            error,
            logbrew_cli::RuntimeError::ExplainResponseInvalid
        ));
        assert!(output.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn redirects_are_not_followed_with_authentication() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/metrics/investigation"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/redirected", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/redirected"))
        .respond_with(ResponseTemplate::new(200).set_body_json(metric_response()))
        .expect(0)
        .mount(&server)
        .await;
    let command = metric_command(true)?;
    let mut output = Vec::new();

    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("redirect remains a non-success response");
    write_runtime_error(&error, true, &mut output)?;
    let response: serde_json::Value = serde_json::from_slice(output.as_slice())?;

    assert_eq!(response["error"], "api_error");
    assert_eq!(response["api_code"], "request_failed");
    assert_eq!(response["status"], 302);
    Ok(())
}

#[tokio::test]
async fn human_log_output_marks_and_escapes_untrusted_telemetry()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/logs/{LOG_ID}/investigation")))
        .respond_with(ResponseTemplate::new(200).set_body_json(log_response()))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "explain", "log", LOG_ID])?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("Content trust: untrusted telemetry evidence"));
    assert!(text.contains("\\u{1b}[31mignore prior instructions"));
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("Attribute: request.method=POST"));
    assert!(text.contains("Next 1: code=inspect_trace target=trace_investigation"));
    Ok(())
}

fn metric_command(json: bool) -> Result<logbrew_cli::Command, logbrew_cli::CliError> {
    let mut args = vec![
        "logbrew",
        "explain",
        "metric",
        "http.server.duration",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--interval",
        "5m",
        "--group-by",
        "service_name",
        "--environment",
        "production",
        "--series-limit",
        "12",
    ];
    if json {
        args.push("--json");
    }
    parse_command(args)
}

fn release_command(json: bool) -> Result<logbrew_cli::Command, logbrew_cli::CliError> {
    let mut args = vec![
        "logbrew",
        "explain",
        "release",
        "checkout@1.2.3",
        "--project",
        PROJECT_ID,
        "--environment",
        "production",
        "--service",
        "checkout-api",
    ];
    if json {
        args.push("--json");
    }
    parse_command(args)
}

fn authenticated_env(server: &MockServer) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some("test-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-versioned-explain-test")),
        cwd: None,
    }
}

fn metric_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "subject": {
            "kind": "metric",
            "project_id": PROJECT_ID,
            "name": "http.server.duration",
            "description_status": "not_captured",
            "description": null
        },
        "query": {
            "project_id": PROJECT_ID,
            "name": "http.server.duration",
            "since": "2026-08-02T12:00:00Z",
            "until": "2026-08-03T12:00:00Z",
            "interval": "5m",
            "interval_seconds": 300,
            "group_by": "service_name",
            "environment": "production",
            "series_limit": 12
        },
        "purpose": "Explains metric semantics, adjacent-window change, exact trace/span exemplars, and nearby deployments without claiming anomaly or cause.",
        "content_trust": "untrusted_telemetry",
        "analysis": {
            "status": "change_observed",
            "causality": "evidence_only",
            "focus": {
                "comparison_index": 0,
                "selection": "largest_absolute_relative_change",
                "direction": "increased",
                "absolute_change": 24.0,
                "relative_change_percent": 100.0
            }
        },
        "coverage": {
            "samples": 5,
            "series": 1,
            "returned_series": 1,
            "points": 2,
            "expected_buckets_per_series": 288,
            "first_seen_at": "2026-08-03T11:00:10Z",
            "last_seen_at": "2026-08-03T11:09:50Z",
            "truncated": false
        },
        "series": [{
            "identity": {
                "kind": "histogram",
                "unit": "ms",
                "temporality": "delta",
                "group_by": "service_name",
                "group_value": "checkout-api"
            },
            "status": "ready",
            "aggregation": {
                "code": "distribution_p95",
                "description": "Primary value is the approximate p95 in each bucket."
            },
            "sample_count": 5,
            "points": [{
                "bucket_start": "2026-08-03T11:00:00Z",
                "bucket_end": "2026-08-03T11:05:00Z",
                "sample_count": 2,
                "value": 42.0,
                "min": 10.0,
                "max": 50.0,
                "average": 28.0,
                "sum": 56.0,
                "p50": 20.0,
                "p95": 42.0,
                "p99": 50.0,
                "trace_exemplars": []
            }, {
                "bucket_start": "2026-08-03T11:05:00Z",
                "bucket_end": "2026-08-03T11:10:00Z",
                "sample_count": 3,
                "value": 48.0,
                "min": 12.0,
                "max": 60.0,
                "average": 30.0,
                "sum": 90.0,
                "p50": 20.0,
                "p95": 48.0,
                "p99": 60.0,
                "trace_exemplars": [TRACE_ID]
            }]
        }],
        "comparison": {
            "status": "available",
            "method": "adjacent_equal_window_latest_bucket",
            "previous_since": "2026-08-01T12:00:00Z",
            "previous_until": "2026-08-02T12:00:00Z",
            "items": [{
                "identity": {
                    "kind": "histogram",
                    "unit": "ms",
                    "temporality": "delta",
                    "group_by": "service_name",
                    "group_value": "checkout-api"
                },
                "aggregation": "distribution_p95",
                "current": {
                    "bucket_start": "2026-08-03T11:05:00Z",
                    "bucket_end": "2026-08-03T11:10:00Z",
                    "sample_count": 3,
                    "value": 48.0
                },
                "previous": {
                    "bucket_start": "2026-08-02T11:55:00Z",
                    "bucket_end": "2026-08-02T12:00:00Z",
                    "sample_count": 2,
                    "value": 24.0
                },
                "direction": "increased",
                "absolute_change": 24.0,
                "relative_change_percent": 100.0
            }],
            "truncated": false,
            "limitation": "Observed adjacent-window latest-bucket change is not seasonality-aware anomaly detection or proof that a deployment caused it."
        },
        "latest_sample": {
            "status": "available",
            "sample": {
                "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
                "kind": "histogram",
                "value": 48.0,
                "unit": "ms",
                "temporality": "delta",
                "occurred_at": "2026-08-03T11:09:50Z",
                "service_name": "checkout-api",
                "environment": "production",
                "release": "checkout@1.2.3",
                "trace_id": TRACE_ID,
                "span_id": SPAN_ID,
                "sdk": {"name": "logbrew-rust", "version": "1.2.3"},
                "context": null,
                "metadata": {
                    "values": {},
                    "included_leaf_count": 0,
                    "redacted": false,
                    "truncated": false
                }
            }
        },
        "exemplars": {
            "status": "available",
            "coverage": {
                "matching_samples": 5,
                "trace_linked_samples": 1,
                "span_linked_samples": 1,
                "returned_exemplars": 1
            },
            "items": [{
                "id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                "value": 48.0,
                "occurred_at": "2026-08-03T11:09:50Z",
                "trace_id": TRACE_ID,
                "span_id": SPAN_ID,
                "service_name": "checkout-api",
                "environment": "production",
                "release": "checkout@1.2.3",
                "sdk": {"name": "logbrew-rust", "version": "1.2.3"}
            }],
            "truncated": false
        },
        "deployments": {
            "status": "available",
            "items": [{
                "deployment_id": "deploy-123",
                "release": "checkout@1.2.3",
                "environment": "production",
                "service_name": "checkout-api",
                "status": "succeeded",
                "started_at": "2026-08-03T11:02:00Z",
                "finished_at": "2026-08-03T11:03:00Z",
                "commit_sha": "0123456789abcdef"
            }],
            "truncated": false
        },
        "timeline": {
            "items": [{
                "id": "deploy-123",
                "kind": "deployment_finished",
                "occurred_at": "2026-08-03T11:03:00Z",
                "value": null,
                "trace_id": null,
                "span_id": null,
                "release": "checkout@1.2.3",
                "service_name": "checkout-api"
            }, {
                "id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                "kind": "metric_exemplar",
                "occurred_at": "2026-08-03T11:09:50Z",
                "value": 48.0,
                "trace_id": TRACE_ID,
                "span_id": SPAN_ID,
                "release": "checkout@1.2.3",
                "service_name": "checkout-api"
            }],
            "truncated": false
        },
        "evidence": {
            "status": "partial",
            "captured_fields": [
                "metric.current_window_coverage",
                "metric.deployment_overlays",
                "metric.identity",
                "metric.latest_sample",
                "metric.latest_sample.kind",
                "metric.latest_sample.occurred_at",
                "metric.latest_sample.scope",
                "metric.latest_sample.sdk.name",
                "metric.latest_sample.sdk.version",
                "metric.latest_sample.span_id",
                "metric.latest_sample.trace_id",
                "metric.latest_sample.value",
                "metric.prior_window_comparison",
                "metric.query_scope",
                "metric.series_semantics",
                "metric.span_exemplars",
                "metric.trace_exemplars"
            ],
            "missing_fields": [
                "metric.description",
                "metric.latest_sample.context",
                "metric.latest_sample.metadata"
            ],
            "redacted_fields": [],
            "truncated_fields": []
        },
        "next_actions": [{
            "priority": 1,
            "code": "inspect_exact_span",
            "target": "span_investigation",
            "reason": "Inspect the exact retained span evidence.",
            "context": {
                "project_id": PROJECT_ID,
                "trace_id": TRACE_ID,
                "span_id": SPAN_ID,
                "environment": "production",
                "release": "checkout@1.2.3",
                "service_name": "checkout-api"
            }
        }, {
            "priority": 2,
            "code": "review_deployment",
            "target": "release_investigation",
            "reason": "Review nearby deployment evidence without assuming causality.",
            "context": {
                "project_id": PROJECT_ID,
                "trace_id": null,
                "span_id": null,
                "environment": "production",
                "release": "checkout@1.2.3",
                "service_name": "checkout-api"
            }
        }]
    })
}

fn empty_metric_response() -> serde_json::Value {
    let mut response = metric_response();
    response["analysis"] = serde_json::json!({
        "status": "no_samples",
        "causality": "evidence_only",
        "focus": null
    });
    response["coverage"] = serde_json::json!({
        "samples": 0,
        "series": 0,
        "returned_series": 0,
        "points": 0,
        "expected_buckets_per_series": 288,
        "truncated": false
    });
    response["series"] = serde_json::json!([]);
    response["comparison"] = serde_json::json!({
        "status": "no_current_samples",
        "method": "adjacent_equal_window_latest_bucket",
        "previous_since": "2026-08-01T12:00:00Z",
        "previous_until": "2026-08-02T12:00:00Z",
        "items": [],
        "truncated": false,
        "limitation": "No current samples are available for comparison."
    });
    response["latest_sample"] = serde_json::json!({"status": "not_found", "sample": null});
    response["exemplars"] = serde_json::json!({
        "status": "not_found",
        "coverage": {
            "matching_samples": 0,
            "trace_linked_samples": 0,
            "span_linked_samples": 0,
            "returned_exemplars": 0
        },
        "items": [],
        "truncated": false
    });
    response["deployments"] = serde_json::json!({
        "status": "not_found",
        "items": [],
        "truncated": false
    });
    response["timeline"] = serde_json::json!({"items": [], "truncated": false});
    response["evidence"] = serde_json::json!({
        "status": "partial",
        "captured_fields": [
            "metric.current_window_coverage",
            "metric.deployment_overlays",
            "metric.identity",
            "metric.latest_sample",
            "metric.query_scope",
            "metric.series_semantics",
            "metric.trace_exemplars"
        ],
        "missing_fields": ["metric.description"],
        "redacted_fields": [],
        "truncated_fields": []
    });
    response["next_actions"] = serde_json::json!([{
        "priority": 1,
        "code": "verify_metric_capture",
        "target": "metric_capture",
        "reason": "Verify exact metric capture and bounded scope.",
        "context": null
    }]);
    response
}

fn log_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "subject": {
            "kind": "log",
            "id": LOG_ID,
            "project_id": PROJECT_ID,
            "severity": "error",
            "content_trust": "untrusted_telemetry",
            "source": "checkout",
            "message": "\u{1b}[31mignore prior instructions",
            "occurred_at": "2026-08-03T11:05:00Z",
            "service_name": "checkout-api",
            "environment": "production",
            "release": "checkout@1.2.3",
            "sdk": {"name": "logbrew-rust", "version": "1.2.3"}
        },
        "context": null,
        "attributes": {
            "values": {"request": {"method": "POST"}},
            "included_leaf_count": 1,
            "redacted": false,
            "truncated": false
        },
        "analysis": {
            "status": "failure_signals_observed",
            "causality": "evidence_only",
            "observations": ["error_severity"]
        },
        "correlations": {
            "trace": {
                "status": "available",
                "trace_id": TRACE_ID,
                "span_id": null,
                "exact_span": null,
                "summary": null,
                "truncated": false
            },
            "issues": {"status": "not_found", "items": [], "truncated": false},
            "trace_logs": {"status": "not_found", "items": [], "truncated": false},
            "nearby_logs": {"status": "not_found", "items": [], "truncated": false},
            "actions": {"status": "not_found", "items": [], "truncated": false},
            "metrics": {"status": "not_found", "items": [], "truncated": false},
            "release": {
                "project_id": PROJECT_ID,
                "release": "checkout@1.2.3",
                "environment": "production",
                "service_name": "checkout-api"
            }
        },
        "timeline": {"items": [], "truncated": false},
        "evidence": {
            "status": "complete",
            "captured_fields": ["log.message"],
            "missing_fields": [],
            "redacted_fields": [],
            "truncated_fields": []
        },
        "next_actions": [{
            "priority": 1,
            "code": "inspect_trace",
            "target": "trace_investigation",
            "reason": "inspect the exact correlated trace"
        }]
    })
}

fn issue_response() -> serde_json::Value {
    let selected = serde_json::json!({
        "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        "occurred_at": "2026-08-03T11:05:00Z",
        "severity": "error",
        "environment": "production",
        "release": "checkout@1.2.3",
        "service_name": "checkout-api",
        "sdk": {"name": "logbrew-rust", "version": "1.2.3"},
        "exception_type": "PaymentError",
        "trace_linked": true,
        "stack": {"frame_count": 1, "truncated": false},
        "breadcrumbs": {"count": 1, "truncated": false},
        "context_captured": true
    });
    serde_json::json!({
        "schema_version": 2,
        "subject": {
            "kind": "issue",
            "id": ISSUE_ID,
            "project_id": PROJECT_ID,
            "fingerprint": "payment-charge",
            "status": "unresolved",
            "severity": "error",
            "title": "Payment failed",
            "message": "processor rejected the charge",
            "occurrence_count": 3,
            "first_seen_at": "2026-08-03T10:00:00Z",
            "last_seen_at": "2026-08-03T11:05:00Z"
        },
        "event": {
            "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
            "occurred_at": "2026-08-03T11:05:00Z",
            "sdk": {"name": "logbrew-rust", "version": "1.2.3"},
            "context": {
                "schema_version": 1,
                "resource": {
                    "service": {"name": "checkout-api", "version": "1.2.3"},
                    "deployment": {"environment": "production", "release": "checkout@1.2.3"},
                    "runtime": {"name": "rust", "version": "1.88"},
                    "framework": {"name": "axum", "version": "0.8"},
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
                "session": {"id": "session-42", "previous_id": null},
                "subject": {"id": "user-opaque", "kind": "user"},
                "tags": {"plan": "pro"}
            },
            "exception": {
                "type": "PaymentError",
                "mechanism": {"type": "unhandled", "handled": false}
            },
            "stack_frames": [{
                "index": 0,
                "module": "checkout",
                "function": "charge",
                "file": "payment.rs",
                "line": 42,
                "column": 7,
                "in_app": true,
                "source": "captured"
            }],
            "breadcrumbs": [{
                "timestamp": "2026-08-03T11:04:55Z",
                "type": "user",
                "category": "ui.click",
                "level": "info",
                "message": "submit checkout",
                "data": {"screen": "checkout"}
            }],
            "breadcrumbs_truncated": false
        },
        "occurrence_selection": {
            "requested": "recommended",
            "reason": "context_rich_recent_occurrence",
            "selected": selected.clone(),
            "first": {
                "id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                "occurred_at": "2026-08-03T10:00:00Z",
                "severity": "error",
                "environment": "production",
                "release": "checkout@1.2.1",
                "service_name": "checkout-api",
                "sdk": {"name": "logbrew-rust", "version": "1.2.1"},
                "exception_type": null,
                "trace_linked": false,
                "stack": {"frame_count": 0, "truncated": false},
                "breadcrumbs": {"count": 0, "truncated": false},
                "context_captured": false
            },
            "latest": selected.clone(),
            "recommended": selected,
            "recommendation": {
                "algorithm_version": 1,
                "candidate_count": 3,
                "candidate_limit": 50,
                "candidate_window_truncated": false
            }
        },
        "cause": {
            "status": "evidence_only",
            "summary": null,
            "provenance": null,
            "signals": ["unhandled_exception", "error_trace_span"]
        },
        "fix": {
            "status": "observed_application_frame",
            "location": {
                "component": "payments",
                "module": "checkout",
                "function": "charge",
                "file": "payment.rs",
                "line": 42,
                "column": 7,
                "in_app": true
            },
            "provenance": "backend_observed"
        },
        "impact": {
            "occurrence_count": 3,
            "first_seen_at": "2026-08-03T10:00:00Z",
            "last_seen_at": "2026-08-03T11:05:00Z",
            "affected_users": null,
            "user_impact": {
                "status": "not_captured",
                "known_affected_users": null,
                "count_method": "unavailable",
                "coverage": {
                    "retained_occurrences": 3,
                    "indexed_occurrences": 0,
                    "historical_unindexed_occurrences": 3,
                    "identified_user_occurrences": 0,
                    "anonymous_subject_occurrences": 0,
                    "missing_subject_occurrences": 0,
                    "privacy_filtered_subject_occurrences": 0,
                    "index_coverage_basis_points": 0,
                    "identified_user_coverage_basis_points": null
                },
                "limitations": ["historical_occurrences_unindexed"]
            },
            "reported": null
        },
        "correlations": {
            "trace": {
                "status": "available",
                "trace_id": TRACE_ID,
                "summary": null,
                "truncated": false
            },
            "logs": {
                "status": "available",
                "items": [{
                    "id": LOG_ID,
                    "severity": "error",
                    "source": "payments",
                    "message": "processor rejected charge",
                    "occurred_at": "2026-08-03T11:04:59Z",
                    "service_name": "checkout-api",
                    "span_id": "0123456789abcdef"
                }],
                "truncated": false
            },
            "actions": {"status": "not_found", "items": [], "truncated": false},
            "metrics": {"status": "not_found", "items": [], "truncated": false},
            "release": {
                "release": "checkout@1.2.3",
                "environment": "production",
                "service_name": "checkout-api"
            }
        },
        "evidence": {
            "status": "partial",
            "captured_fields": [
                "issue.exception",
                "issue.stack_frames",
                "occurrence.boundaries",
                "occurrence.recommendation",
                "occurrence.selection"
            ],
            "missing_fields": ["issue.attachment"],
            "redacted_fields": [],
            "truncated_fields": []
        },
        "next_actions": [{
            "priority": 1,
            "code": "inspect_code_location",
            "target": "source_code",
            "reason": "likely_fix_location_available"
        }]
    })
}

fn release_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 2,
        "subject": {
            "kind": "release",
            "project_id": PROJECT_ID,
            "release": "checkout@1.2.3",
            "environment": "production",
            "service_name": "checkout-api",
            "issue_count": 1,
            "log_count": 1,
            "trace_span_count": 2,
            "action_count": 4,
            "metric_count": 3,
            "first_seen_at": "2026-08-03T10:00:00Z",
            "last_seen_at": "2026-08-03T11:10:00Z",
            "trace_health_status": "available",
            "trace_health": {
                "status": "failures_observed",
                "trace_count": 1,
                "error_trace_count": 1,
                "error_rate_basis_points": 10000
            }
        },
        "analysis": {"status": "failures_observed", "causality": "evidence_only"},
        "sdk_coverage": {
            "status": "available",
            "items": [{
                "name": "logbrew-rust",
                "version": "1.2.3",
                "stream": "issues",
                "item_count": 1,
                "first_seen_at": "2026-08-03T11:05:00Z",
                "last_seen_at": "2026-08-03T11:05:00Z"
            }],
            "truncated": false
        },
        "signals": {
            "issues": {
                "status": "available",
                "items": [{
                    "issue_id": ISSUE_ID,
                    "severity": "error",
                    "title": "Payment failed",
                    "message": "processor rejected the charge",
                    "occurrence_count": 1,
                    "first_seen_at": "2026-08-03T11:05:00Z",
                    "last_seen_at": "2026-08-03T11:05:00Z",
                    "trace_id": TRACE_ID
                }],
                "truncated": false
            },
            "traces": {
                "status": "available",
                "items": [{
                    "trace_id": TRACE_ID,
                    "root_span_name": "POST /checkout",
                    "span_count": 2,
                    "error_span_count": 1,
                    "started_at": "2026-08-03T11:04:58Z",
                    "duration_ms": 900
                }],
                "truncated": false
            },
            "logs": {
                "status": "available",
                "selection": "warning_error_critical",
                "items": [{
                    "id": LOG_ID,
                    "level": "error",
                    "source": "payments",
                    "message": "processor rejected charge",
                    "occurred_at": "2026-08-03T11:04:59Z",
                    "trace_id": TRACE_ID,
                    "span_id": "0123456789abcdef"
                }],
                "truncated": false
            },
            "actions": {
                "status": "available",
                "estimation": {
                    "unique_counts_are_approximate": true,
                    "method": "approximate_uniq_combined64"
                },
                "items": [{
                    "name": "checkout.submit",
                    "event_count": 4,
                    "identified_user_count": 2,
                    "anonymous_subject_count": 1,
                    "subject_coverage": {
                        "index_version": 1,
                        "identified_user_events": 2,
                        "anonymous_subject_events": 1,
                        "legacy_unknown_kind_events": 0,
                        "missing_subject_events": 1,
                        "historical_unindexed_events": 0
                    },
                    "session_count": 3,
                    "first_seen_at": "2026-08-03T10:30:00Z",
                    "last_seen_at": "2026-08-03T11:04:57Z",
                    "trace_id": TRACE_ID
                }],
                "truncated": false
            },
            "metrics": {
                "status": "available",
                "items": [{
                    "name": "checkout.duration",
                    "kind": "histogram",
                    "unit": "ms",
                    "temporality": "delta",
                    "event_count": 3,
                    "minimum_value": 120.0,
                    "maximum_value": 300.0,
                    "average_value": 220.0,
                    "latest_value": 240.0,
                    "latest_at": "2026-08-03T11:05:01Z",
                    "trace_id": TRACE_ID
                }],
                "truncated": false
            }
        },
        "timeline": {
            "items": [{
                "kind": "issue",
                "occurred_at": "2026-08-03T11:05:00Z",
                "summary": "Payment failed",
                "issue_id": ISSUE_ID,
                "trace_id": TRACE_ID
            }],
            "truncated": false
        },
        "comparison": {
            "status": "unavailable",
            "reason": "deployment_boundary_not_captured"
        },
        "evidence": {
            "status": "partial",
            "captured_fields": [
                "release.actions.subject_coverage",
                "release.issues",
                "release.traces"
            ],
            "missing_fields": ["deployment.boundary"],
            "redacted_fields": [],
            "truncated_fields": []
        },
        "next_actions": [{
            "code": "inspect_release_issue",
            "target": "issue_investigation",
            "reason": "issue_observed",
            "issue_id": ISSUE_ID,
            "trace_id": TRACE_ID
        }, {
            "code": "inspect_release_trace",
            "target": "trace_investigation",
            "reason": "trace_observed",
            "issue_id": null,
            "trace_id": TRACE_ID
        }, {
            "code": "review_release_logs",
            "target": "telemetry_logs",
            "reason": "high_severity_logs_observed",
            "issue_id": null,
            "trace_id": null
        }, {
            "code": "review_release_actions",
            "target": "telemetry_actions",
            "reason": "product_usage_observed",
            "issue_id": null,
            "trace_id": null
        }, {
            "code": "review_release_metrics",
            "target": "telemetry_metrics",
            "reason": "metric_evidence_observed",
            "issue_id": null,
            "trace_id": null
        }, {
            "code": "capture_deployment_boundary",
            "target": "release_instrumentation",
            "reason": "comparison_unavailable",
            "issue_id": null,
            "trace_id": null
        }]
    })
}
