//! Rich issue-investigation alias, output, and recovery contracts.

use crate::matchers::{header, query_param};
use crate::{Mock, MockServer, ResponseTemplate};
use logbrew_cli::{
    CliEnvironment, CliError, Command, ExplainTarget, IssueCorrectionTarget,
    IssueOccurrenceSelection, RuntimeError, execute_command, parse_command, write_cli_error,
    write_runtime_error,
};

const ISSUE_ID: &str = "11111111-1111-4111-8111-111111111111";
const OCCURRENCE_ID: &str = "22222222-2222-4222-8222-222222222222";
const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const HOSTILE_MARKER: &str = "hostile-response-marker";
const DEPLOYMENT_ID: &str = "candidate-deploy-42";

#[test]
fn parses_only_the_explicit_issue_investigation_grammar() {
    let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])
        .expect("explicit issue investigation parses");

    assert!(command.wants_json());
    assert_eq!(command.http_path(), None);
    assert_eq!(command.http_method(), None);
    let first = parse_command([
        "logbrew",
        "investigate",
        "issue",
        ISSUE_ID,
        "--occurrence=first",
    ])
    .expect("named occurrence parses");
    assert_eq!(
        first,
        Command::InvestigateIssue {
            issue_id: ISSUE_ID.to_owned(),
            occurrence: IssueOccurrenceSelection::First,
            json: false,
        }
    );

    let exact = parse_command([
        "logbrew",
        "explain",
        "issue",
        ISSUE_ID,
        "--occurrence",
        OCCURRENCE_ID,
        "--json",
    ])
    .expect("exact occurrence parses");
    assert_eq!(
        exact.http_path().as_deref(),
        Some(
            "/api/telemetry/issues/11111111-1111-4111-8111-111111111111/investigation?\
             response_version=11&occurrence_id=22222222-2222-4222-8222-222222222222"
        )
    );
    assert!(exact.wants_json());

    let latest = parse_command([
        "logbrew",
        "explain",
        "issue",
        ISSUE_ID,
        "--occurrence=latest",
    ])
    .expect("latest occurrence parses");
    assert_eq!(
        latest.http_path().as_deref(),
        Some(
            "/api/telemetry/issues/11111111-1111-4111-8111-111111111111/investigation?\
             response_version=11&selection=latest"
        )
    );
}

#[test]
fn parses_exact_correction_verification_without_a_parallel_command_stack() {
    let command = parse_command([
        "logbrew",
        "investigate",
        "issue",
        ISSUE_ID,
        "verify",
        "--baseline-occurrence",
        OCCURRENCE_ID,
        "--candidate-deployment=candidate-deploy-42",
        "--json",
    ])
    .expect("correction verification parses");
    assert_eq!(
        command,
        Command::Explain {
            target: ExplainTarget::IssueCorrection(IssueCorrectionTarget {
                issue_id: ISSUE_ID.to_owned(),
                baseline_occurrence_id: OCCURRENCE_ID.to_owned(),
                candidate_deployment_id: DEPLOYMENT_ID.to_owned(),
            }),
            json: true,
        }
    );
    assert_eq!(
        command.http_path().as_deref(),
        Some(
            "/api/telemetry/issues/11111111-1111-4111-8111-111111111111/correction-verification?\
             baseline_occurrence_id=22222222-2222-4222-8222-222222222222&\
             candidate_deployment_id=candidate-deploy-42"
        )
    );
}

#[test]
fn grammar_failures_are_fixed_and_value_safe() -> Result<(), Box<dyn std::error::Error>> {
    for suffix in [
        vec![],
        vec!["trace", TRACE_ID],
        vec!["issue", ISSUE_ID, "--authorization=hostile-secret"],
        vec!["issue", ISSUE_ID, "--occurrence=hostile-selector"],
        vec![
            "issue",
            ISSUE_ID,
            "--occurrence",
            OCCURRENCE_ID,
            "--occurrence",
            "latest",
        ],
        vec!["issue", "issue_123"],
    ] {
        let args = [vec!["logbrew", "investigate"], suffix].concat();
        let error = parse_command(args).expect_err("closed investigation grammar rejects input");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["error"], "invalid_investigation_command");
        assert_eq!(
            body["next"],
            "use logbrew investigate issue <issue_id> or logbrew explain issue <issue_id> with \
             optional --occurrence recommended|first|latest|<occurrence_id>, or append verify \
             --baseline-occurrence <occurrence_id> --candidate-deployment <deployment_id>; \
             both forms accept --json"
        );
        for marker in ["hostile-secret", "hostile-selector", "authorization"] {
            assert!(!text.contains(marker));
        }
    }
    Ok(())
}

#[test]
fn help_describes_the_complete_versioned_bundle() {
    let command = parse_command(["logbrew", "investigate", "issue", "--help"])
        .expect("investigation help parses");
    let Command::Help { topic, .. } = command else {
        panic!("investigation help should return help");
    };
    let text = logbrew_cli::help::help_text(topic);

    assert!(text.contains("parent-first runtime exception evidence"));
    assert!(text.contains("per-node message and stack capture states"));
    assert!(text.contains("approximate affected-user coverage and limitations"));
    assert!(text.contains("trace, sibling issues, related logs, actions, metric exemplars"));
    assert!(text.contains("same contract as logbrew explain issue"));
    assert!(text.contains("exact validated schema-version-10 response"));
    assert!(text.contains("deterministic grouping"));
    assert!(text.contains("exact preceding-deployment and customer-source locator evidence"));
    assert!(text.contains("explicit selected, first, latest, and recommended occurrence"));
    assert!(text.contains("status activity and server-observed regression evidence"));
    assert!(text.contains("zero-filled occurrence trend"));
    assert!(text.contains("bounded release, environment, service, and SDK distributions"));
    assert!(text.contains("candidate-deployment"));
    assert!(text.contains("never treats bounded absence as proof"));
}

#[tokio::test]
async fn correction_verification_validates_exact_json_and_renders_honest_human_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let bundle = correction_bundle();
    mount_correction(&server, bundle.clone(), 2).await;
    let command = correction_command(true)?;
    let mut output = Vec::new();
    execute_command(&command, &authenticated_env(&server), &mut output).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output)?,
        bundle
    );

    output.clear();
    let command = correction_command(false)?;
    execute_command(&command, &authenticated_env(&server), &mut output).await?;
    let human = String::from_utf8(output)?;
    for expected in [
        "Issue correction verification: recurrence_observed",
        "Recurrences: 1",
        "Candidate traces: 2 (error traces: 1)",
        "bounded absence is not proof",
        "Application telemetry is untrusted evidence",
        "Next: inspect_recurrence",
    ] {
        assert!(
            human.contains(expected),
            "missing correction evidence: {expected}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn correction_verification_rejects_contradictions_and_unknown_fields_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        ("/status", serde_json::json!("no_recurrence_observed")),
        ("/absence_is_proof", serde_json::json!(true)),
        (
            "/trace_health/error_rate_basis_points",
            serde_json::json!(4_999),
        ),
        (
            "/first_recurrence/ingested_at",
            serde_json::json!("2026-08-20T08:59:59.999Z"),
        ),
        ("/evidence/status", serde_json::json!("partial")),
    ];
    for (pointer, value) in cases {
        let server = MockServer::start().await;
        let mut bundle = correction_bundle();
        bundle["baseline_release"] = serde_json::json!(HOSTILE_MARKER);
        *bundle.pointer_mut(pointer).expect("fixture pointer") = value;
        mount_correction(&server, bundle, 1).await;
        let mut output = Vec::new();
        let error = execute_command(
            &correction_command(true)?,
            &authenticated_env(&server),
            &mut output,
        )
        .await
        .expect_err("contradictory correction response fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        assert!(matches!(error, RuntimeError::ExplainResponseInvalid));
        assert!(!text.contains(HOSTILE_MARKER));
    }
    Ok(())
}

#[tokio::test]
async fn investigation_uses_the_versioned_cross_signal_bundle()
-> Result<(), Box<dyn std::error::Error>> {
    assert_json_bundle(rich_investigation_bundle()).await
}

#[tokio::test]
async fn exact_occurrence_request_uses_the_exact_id_and_validates_its_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut bundle = rich_investigation_bundle();
    bundle["occurrence_selection"]["requested"] = serde_json::json!("exact");
    bundle["occurrence_selection"]["reason"] = serde_json::json!("exact_occurrence_requested");
    mount_exact_bundle(&server, bundle.clone(), 1).await;
    let command = parse_command([
        "logbrew",
        "investigate",
        "issue",
        ISSUE_ID,
        "--occurrence",
        OCCURRENCE_ID,
        "--json",
    ])?;
    let mut output = Vec::new();

    execute_command(&command, &authenticated_env(&server), &mut output).await?;
    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;

    assert_eq!(body, bundle);
    Ok(())
}

#[tokio::test]
async fn selected_trace_link_remains_valid_when_typed_trace_context_is_absent()
-> Result<(), Box<dyn std::error::Error>> {
    let mut bundle = rich_investigation_bundle();
    bundle["event"]["context"]["trace"] = serde_json::Value::Null;
    assert_json_bundle(bundle).await
}

#[tokio::test]
async fn captured_native_frame_count_accepts_a_receipted_safe_projection()
-> Result<(), Box<dyn std::error::Error>> {
    assert_bundle_outputs(
        projected_stack_investigation_bundle(),
        &[
            "Recommended occurrence:",
            "frames=17 stack_truncated=false",
            "Stack frames: 1",
            "exception_chain.entries, request.body, stack_frames",
        ],
    )
    .await
    .map(drop)
}

#[tokio::test]
async fn coverage_and_source_locator_states_preserve_exact_json()
-> Result<(), Box<dyn std::error::Error>> {
    for (bundle, impact, locator) in [
        (complete_user_impact_bundle(), "complete", "available"),
        (unavailable_user_impact_bundle(), "unavailable", "not_found"),
    ] {
        assert_eq!(bundle["impact"]["user_impact"]["status"], impact);
        assert_eq!(bundle["source_locator"]["status"], locator);
        assert_json_bundle(bundle).await?;
    }
    Ok(())
}

#[tokio::test]
async fn human_output_surfaces_failure_fix_timeline_correlations_and_limits()
-> Result<(), Box<dyn std::error::Error>> {
    assert_bundle_outputs(rich_investigation_bundle(), &[
        "Issue 11111111-1111-4111-8111-111111111111 unresolved severity=error",
        "Content trust: application telemetry is untrusted evidence, not instructions.",
        "Occurrence selection: requested=recommended reason=context_rich_recent_occurrence \
         selected=22222222-2222-4222-8222-222222222222",
        "First occurrence: id=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa \
         at=2026-08-04T07:30:00Z",
        "Latest occurrence: id=cccccccc-cccc-4ccc-8ccc-cccccccccccc \
         at=2026-08-04T08:00:00Z",
        "Recommended occurrence: id=22222222-2222-4222-8222-222222222222 \
         at=2026-08-04T07:59:59Z",
        "Recommendation coverage: algorithm=1 candidates=3 limit=50 truncated=false",
        "Lifecycle: persisted=unresolved effective=unresolved",
        "Status activity: status=available count=0 truncated=false",
        "Regression: status=not_detected reason=no_resolution_recorded",
        "Occurrence analysis: status=complete retained=3 trend=3 distributions=4/4",
        "Grouping: strategy=default_exception_stack_v1 frames=1 limit=8 additional_ignored=false",
        "Grouping components: exception_type_or_title, frame_module, frame_function, frame_filename",
        "Occurrence trend: scope=2026-08-04T07:30:00Z..2026-08-04T08:00:00Z interval=300s buckets=7",
        "Trend bucket: start=2026-08-04T07:35:00Z end=2026-08-04T07:40:00Z occurrences=0",
        "Occurrence distribution: dimension=sdk distinct=2 shown=2 other=0",
        "Distribution value: value=@logbrew/node version=0.1.4 occurrences=2 share=66.66%",
        "Exception: PaymentProviderError mechanism=javascript.promise handled=false",
        "Exception chain: status=captured entries=2 truncated=true",
        "Exception node id=0 relationship=reported type=PaymentProviderError \
         module=checkout.payment message_state=captured message=Payment capture failed",
        "Exception node id=1 parent=0 relationship=cause type=UpstreamTimeoutError \
         module=checkout.provider message_state=redacted mechanism=javascript.cause handled=true",
        "Exception stack node=1 state=not_captured frames=0",
        "Request: method=POST route=/checkout/{cart_id} status=503",
        "Frame: module=checkout function=capturePayment file=apps/checkout/payment_gateway.ts line=87",
        "Breadcrumb: at=2026-08-04T07:59:58Z category=checkout.submit",
        "Runtime: service=checkout-api@1.2.3 runtime=node@24",
        "Cause assessment: status=reported_hypothesis provenance=application_reported",
        "Reported hypothesis (unverified): The provider returned 503 after retries.",
        "Fix area: status=reported_location provenance=application_reported",
        "Source locator: status=available provider=github repository=example/checkout \
         component=apps/checkout revision=0123456789abcdef \
         revision_source=deployment_commit file=apps/checkout/payment_gateway.ts evidence=complete",
        "Reproducer: status=ready \
         baseline=22222222-2222-4222-8222-222222222222 evidence=complete",
        "Known affected users: ~2 status=partial method=approximate_uniq_combined64",
        "User-impact coverage: retained=3 indexed=3 identified=2 anonymous=0 missing=0 \
         privacy_filtered=1 historical_unindexed=0 index=100.00% identified_share=66.66%",
        "User-impact limitations: approximate_distinct_count, privacy_filtered_subject_context",
        "Reported impact (unverified): segment=paying failed_action=checkout.submit",
        "Preceding deployment: status=available id=deploy-123 result=succeeded \
         started=2026-08-04T07:50:00Z finished=2026-08-04T07:52:00Z \
         commit=0123456789abcdef before_occurrence_ms=479000 causality=evidence_only",
        "Trace: status=available trace=4bf92f3577b34da6a3ce929d0e0e4736 spans=3 errors=1",
        "Related issues: status=available count=1",
        "Issue: title=Sibling checkout failure severity=error issue=66666666-6666-4666-8666-666666666666",
        "Related logs: status=available count=1",
        "Log: message=provider returned 503 severity=error source=payments service=checkout-api",
        "Related actions: status=available count=1",
        "Action: name=checkout.submit service=checkout-api",
        "Related metrics: status=available count=1",
        "Metric: name=payment.retry.count kind=counter temporality=delta value=3 unit=attempts",
        "Evidence: status=partial",
        "Next 1: code=inspect_code_location target=source_code reason=likely_fix_location_available",
        "Next 7: code=improve_capture target=sdk_configuration reason=evidence_incomplete",
    ]).await.map(drop)
}

#[tokio::test]
async fn exception_chain_absence_and_invalid_storage_are_explicit_in_both_output_modes()
-> Result<(), Box<dyn std::error::Error>> {
    for (bundle, expected_status) in [
        (not_captured_exception_chain_bundle(), "not_captured"),
        (invalid_exception_chain_bundle(), "invalid"),
    ] {
        let expected = format!("Exception chain: status={expected_status} entries=0");
        let human = assert_bundle_outputs(bundle, &[expected.as_str()]).await?;
        assert!(!human.contains("runtime_exception_chain"));
    }
    Ok(())
}

#[tokio::test]
async fn underlying_exception_fix_is_bound_to_its_exact_node_and_frame()
-> Result<(), Box<dyn std::error::Error>> {
    assert_bundle_outputs(
        underlying_exception_fix_bundle(),
        &[
            "Fix area: status=observed_underlying_exception_frame provenance=backend_observed \
         module=checkout.provider function=requestPayment file=apps/checkout/provider_client.ts line=142 \
         column=9 in_app=true source_exception=1",
        ],
    )
    .await
    .map(drop)
}

#[tokio::test]
async fn regression_evidence_is_strictly_validated_and_visible_in_both_output_modes()
-> Result<(), Box<dyn std::error::Error>> {
    assert_bundle_outputs(
        regressed_investigation_bundle(),
        &[
            "Issue 11111111-1111-4111-8111-111111111111 resolved severity=error",
            "Lifecycle: persisted=resolved effective=regressed",
            "Status activity: status=available count=2 truncated=false",
            "Status change: status=resolved at=2026-08-04T07:45:00Z",
            "Regression: status=detected reason=occurrence_ingested_after_resolution",
            "Resolution: 2026-08-04T07:45:00Z",
            "First reappeared: id=22222222-2222-4222-8222-222222222222 \
         occurred=2026-08-04T07:59:59Z ingested=2026-08-04T08:00:01Z",
            "release=checkout@1.2.3 service=checkout-api trace=4bf92f3577b34da6a3ce929d0e0e4736",
            "Next 6: code=compare_release target=release_summary reason=regression_detected",
        ],
    )
    .await
    .map(drop)
}

#[tokio::test]
async fn unavailable_lifecycle_reads_keep_the_primary_issue_and_exact_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    for (bundle, expected) in [
        (
            status_history_unavailable_bundle(),
            "Regression: status=unavailable reason=status_history_unavailable",
        ),
        (
            recurrence_unavailable_bundle(),
            "Regression: status=unavailable reason=recurrence_read_unavailable",
        ),
    ] {
        assert_bundle_outputs(
            bundle,
            &["Issue 11111111-1111-4111-8111-111111111111", expected],
        )
        .await
        .map(drop)?;
    }
    Ok(())
}

#[tokio::test]
async fn partial_and_unavailable_occurrence_analysis_keep_the_primary_investigation()
-> Result<(), Box<dyn std::error::Error>> {
    for (bundle, status, expected) in [
        (
            trend_unavailable_bundle(),
            "partial",
            "Occurrence-analysis limitations: trend_read_unavailable",
        ),
        (
            occurrence_analysis_unavailable_bundle(),
            "unavailable",
            "Occurrence analysis: status=unavailable retained=3 distributions=0/4",
        ),
    ] {
        assert_eq!(bundle["occurrence_analysis"]["status"], status);
        assert_bundle_outputs(
            bundle,
            &[
                "Issue 11111111-1111-4111-8111-111111111111",
                "Occurrence trend: unavailable.",
                expected,
            ],
        )
        .await
        .map(drop)?;
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
        Mock::route(
            "GET",
            format!("/api/telemetry/issues/{ISSUE_ID}/investigation"),
        )
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
async fn contradictory_contract_bundles_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut cases = invalid_user_impact_bundles();
    cases.extend(invalid_occurrence_analysis_bundles());
    cases.extend(invalid_occurrence_selection_bundles());
    cases.extend(invalid_event_evidence_bundles());
    cases.extend(invalid_lifecycle_bundles());
    assert_invalid_bundles(cases).await
}

#[tokio::test]
async fn exact_selector_rejects_a_recommended_server_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut bundle = rich_investigation_bundle();
    bundle["subject"]["message"] = serde_json::json!(HOSTILE_MARKER);
    mount_exact_bundle(&server, bundle, 1).await;
    let command = parse_command([
        "logbrew",
        "investigate",
        "issue",
        ISSUE_ID,
        "--occurrence",
        OCCURRENCE_ID,
        "--json",
    ])?;
    let mut output = Vec::new();

    let error = execute_command(&command, &authenticated_env(&server), &mut output)
        .await
        .expect_err("server selector mismatch fails closed");
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let response: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(response["error"], "investigation_response_invalid");
    assert!(!text.contains(HOSTILE_MARKER));
    Ok(())
}

#[tokio::test]
async fn redirects_are_not_followed_with_authentication() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    Mock::route(
        "GET",
        format!("/api/telemetry/issues/{ISSUE_ID}/investigation"),
    )
    .respond_with(
        ResponseTemplate::new(302)
            .insert_header("location", format!("{}/redirected", server.uri())),
    )
    .expect(1)
    .mount(&server)
    .await;
    Mock::auth("GET", "/redirected", "test-token")
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
    Mock::route(
        "GET",
        format!("/api/telemetry/issues/{ISSUE_ID}/investigation"),
    )
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
    Mock::route(
        "GET",
        format!("/api/telemetry/issues/{ISSUE_ID}/investigation"),
    )
    .and(query_param("response_version", "11"))
    .and(query_param("selection", "recommended"))
    .and(header("authorization", "Bearer test-token"))
    .respond_with(ResponseTemplate::new(200).set_body_json(bundle))
    .expect(expected_requests)
    .mount(server)
    .await;
}

async fn mount_exact_bundle(
    server: &MockServer,
    bundle: serde_json::Value,
    expected_requests: u64,
) {
    Mock::route(
        "GET",
        format!("/api/telemetry/issues/{ISSUE_ID}/investigation"),
    )
    .and(query_param("response_version", "11"))
    .and(query_param("occurrence_id", OCCURRENCE_ID))
    .and(header("authorization", "Bearer test-token"))
    .respond_with(ResponseTemplate::new(200).set_body_json(bundle))
    .expect(expected_requests)
    .mount(server)
    .await;
}

async fn mount_correction(server: &MockServer, bundle: serde_json::Value, expected_requests: u64) {
    Mock::route(
        "GET",
        format!("/api/telemetry/issues/{ISSUE_ID}/correction-verification"),
    )
    .and(query_param("baseline_occurrence_id", OCCURRENCE_ID))
    .and(query_param("candidate_deployment_id", DEPLOYMENT_ID))
    .and(header("authorization", "Bearer test-token"))
    .respond_with(ResponseTemplate::new(200).set_body_json(bundle))
    .expect(expected_requests)
    .mount(server)
    .await;
}

fn correction_command(json: bool) -> Result<Command, CliError> {
    parse_command(
        [
            "logbrew",
            "investigate",
            "issue",
            ISSUE_ID,
            "verify",
            "--baseline-occurrence",
            OCCURRENCE_ID,
            "--candidate-deployment",
            DEPLOYMENT_ID,
            if json { "--json" } else { "" },
        ]
        .into_iter()
        .filter(|value| !value.is_empty()),
    )
}

async fn assert_invalid_bundles(
    cases: Vec<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    for bundle in cases {
        let server = MockServer::start().await;
        mount_bundle(&server, bundle, 1).await;
        let command = parse_command(["logbrew", "investigate", "issue", ISSUE_ID, "--json"])?;
        let mut output = Vec::new();
        let error = execute_command(&command, &authenticated_env(&server), &mut output)
            .await
            .expect_err("contradictory investigation contract fails closed");
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let response: serde_json::Value = serde_json::from_str(text.as_str())?;
        assert_eq!(response["error"], "investigation_response_invalid");
        assert!(!text.contains(HOSTILE_MARKER));
    }
    Ok(())
}

async fn assert_bundle_outputs(
    bundle: serde_json::Value,
    expected_human: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_bundle(&server, bundle.clone(), 2).await;
    let json: serde_json::Value =
        serde_json::from_str(run(&server, true, "investigate").await?.as_str())?;
    assert_eq!(json, bundle);
    let human = run(&server, false, "investigate").await?;
    for expected in expected_human {
        assert!(
            human.contains(expected),
            "missing investigation detail: {expected}"
        );
    }
    Ok(human)
}

async fn assert_json_bundle(bundle: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_bundle(&server, bundle.clone(), 1).await;
    let actual: serde_json::Value =
        serde_json::from_str(run(&server, true, "investigate").await?.as_str())?;
    assert_eq!(actual, bundle);
    Ok(())
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
    super::authenticated_env(server, "test-token", Some("issue-investigation-test"))
}

fn rich_investigation_bundle() -> serde_json::Value {
    let selected = selected_occurrence_summary();
    let first = sparse_occurrence_summary(
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "2026-08-04T07:30:00Z",
        "checkout@1.2.1",
        "0.1.2",
    );
    let latest = sparse_occurrence_summary(
        "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        "2026-08-04T08:00:00Z",
        "checkout@1.2.4",
        "0.1.4",
    );
    serde_json::json!({
        "schema_version": 11,
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
            "id": OCCURRENCE_ID,
            "occurred_at": "2026-08-04T07:59:59Z",
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
            "exception_chain": {
                "status": "captured",
                "entries": [
                    {
                        "id": 0,
                        "parent_id": null,
                        "relationship": "reported",
                        "type": "PaymentProviderError",
                        "message": "Payment capture failed",
                        "message_state": "captured",
                        "module": "checkout.payment",
                        "mechanism": {"type": "javascript.promise", "handled": false},
                        "stack_frames": [captured_stack_frame((
                            "checkout", "capturePayment", "apps/checkout/payment_gateway.ts", 87, 12
                        ))],
                        "stack_frames_state": "captured"
                    },
                    {
                        "id": 1,
                        "parent_id": 0,
                        "relationship": "cause",
                        "type": "UpstreamTimeoutError",
                        "message": null,
                        "message_state": "redacted",
                        "module": "checkout.provider",
                        "mechanism": {"type": "javascript.cause", "handled": true},
                        "stack_frames": [],
                        "stack_frames_state": "not_captured"
                    }
                ],
                "truncated": true
            },
            "request": {
                "method": "POST",
                "route_template": "/checkout/{cart_id}",
                "response_status_code": 503
            },
            "stack_frames": [captured_stack_frame((
                "checkout", "capturePayment", "apps/checkout/payment_gateway.ts", 87, 12
            ))],
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
        "occurrence_selection": {
            "requested": "recommended",
            "reason": "context_rich_recent_occurrence",
            "selected": selected.clone(),
            "first": first,
            "latest": latest,
            "recommended": selected,
            "recommendation": {
                "algorithm_version": 1,
                "candidate_count": 3,
                "candidate_limit": 50,
                "candidate_window_truncated": false
            }
        },
        "lifecycle": {
            "persisted_status": "unresolved",
            "effective_status": "unresolved",
            "activity": {
                "status": "available",
                "changes": [],
                "truncated": false
            },
            "regression": {
                "status": "not_detected",
                "reason": "no_resolution_recorded",
                "resolution_changed_at": null,
                "first_reappeared_occurrence": null
            }
        },
        "occurrence_analysis": occurrence_analysis(),
        "grouping": {
            "strategy": "default_exception_stack_v1",
            "components": ["exception_type_or_title", "frame_module", "frame_function", "frame_filename"],
            "stack": {"considered_frame_count": 1, "frame_limit": 8, "additional_frames_ignored": false}
        },
        "cause": {
            "status": "reported_hypothesis",
            "summary": "The provider returned 503 after retries.",
            "provenance": "application_reported",
            "signals": [
                "reported_root_cause",
                "unhandled_exception",
                "runtime_exception_chain",
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
                "file": "apps/checkout/payment_gateway.ts",
                "line": 87,
                "column": 12,
                "in_app": true
            },
            "provenance": "application_reported"
        },
        "source_locator": {
            "status": "available",
            "repository_provider": "github",
            "repository_full_name": "example/checkout",
            "component_path": "apps/checkout",
            "revision": "0123456789abcdef",
            "revision_source": "deployment_commit",
            "repository_relative_file": "apps/checkout/payment_gateway.ts",
            "evidence": {
                "status": "complete",
                "captured_fields": [
                    "source.component", "source.deployment_revision", "source.repository",
                    "source.repository_relative_file", "source.service"
                ],
                "missing_fields": [],
                "redacted_fields": [],
                "truncated_fields": []
            }
        },
        "reproduction": {
            "status": "ready",
            "baseline_occurrence_id": OCCURRENCE_ID,
            "evidence": {
                "status": "complete",
                "captured_fields": [
                    "reproduction.baseline_occurrence", "reproduction.code_location",
                    "reproduction.expected_failure", "reproduction.trigger"
                ],
                "missing_fields": [],
                "redacted_fields": [],
                "truncated_fields": []
            }
        },
        "impact": {
            "occurrence_count": 3,
            "first_seen_at": "2026-08-04T07:30:00Z",
            "last_seen_at": "2026-08-04T08:00:00Z",
            "affected_users": null,
            "user_impact": {
                "status": "partial",
                "known_affected_users": 2,
                "count_method": "approximate_uniq_combined64",
                "coverage": {
                    "retained_occurrences": 3,
                    "indexed_occurrences": 3,
                    "historical_unindexed_occurrences": 0,
                    "identified_user_occurrences": 2,
                    "anonymous_subject_occurrences": 0,
                    "missing_subject_occurrences": 0,
                    "privacy_filtered_subject_occurrences": 1,
                    "index_coverage_basis_points": 10000,
                    "identified_user_coverage_basis_points": 6666
                },
                "limitations": [
                    "approximate_distinct_count",
                    "privacy_filtered_subject_context"
                ]
            },
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
            "related_issues": {
                "status": "available",
                "items": [{
                    "id": "77777777-7777-4777-8777-777777777777",
                    "issue_id": "66666666-6666-4666-8666-666666666666",
                    "project_id": PROJECT_ID,
                    "severity": "error",
                    "title": "Sibling checkout failure",
                    "message": "Observed on the selected occurrence trace.",
                    "occurred_at": "2026-08-04T07:59:58Z",
                    "service_name": "payment-worker",
                    "environment": "production",
                    "release": "checkout@1.2.3"
                }],
                "truncated": false
            },
            "release": {
                "release": "checkout@1.2.3",
                "environment": "production",
                "service_name": "checkout-api",
                "deployment_status": "available",
                "deployment": {
                    "deployment_id": "deploy-123",
                    "release": "checkout@1.2.3",
                    "environment": "production",
                    "service_name": "checkout-api",
                    "status": "succeeded",
                    "started_at": "2026-08-04T07:50:00Z",
                    "finished_at": "2026-08-04T07:52:00Z",
                    "commit_sha": "0123456789abcdef"
                },
                "time_since_deployment_ms": 479000,
                "deployment_causality": "evidence_only"
            }
        },
        "evidence": {
            "status": "partial",
            "captured_fields": [
                "actions",
                "breadcrumbs",
                "deployment",
                "deployment.commit_sha",
                "deployment.timing",
                "exception",
                "exception_chain",
                "exception_chain.messages",
                "exception_chain.stack_frames",
                "grouping",
                "grouping.components",
                "grouping.stack",
                "grouping.strategy",
                "grouping.strategy_details",
                "logs",
                "metrics",
                "lifecycle.regression",
                "lifecycle.status_history",
                "occurrence.boundaries",
                "occurrence.distribution.environment",
                "occurrence.distribution.release",
                "occurrence.distribution.sdk",
                "occurrence.distribution.service",
                "occurrence.recommendation",
                "occurrence.selection",
                "occurrence.trend",
                "related_issues",
                "release",
                "request",
                "request.method",
                "request.response_status_code",
                "request.route_template",
                "stack_frames",
                "source_locator",
                "trace",
                "affected_users.known"
            ],
            "missing_fields": ["affected_users.complete_coverage"],
            "redacted_fields": ["exception_chain.messages"],
            "truncated_fields": ["exception_chain.entries", "request.body"]
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

fn correction_bundle() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "status": "recurrence_observed",
        "issue_id": ISSUE_ID,
        "project_id": PROJECT_ID,
        "baseline_occurrence_id": OCCURRENCE_ID,
        "baseline_release": "checkout@1.0.0",
        "candidate_deployment": {
            "deployment_id": DEPLOYMENT_ID,
            "release": "checkout@1.1.0",
            "environment": "production",
            "service_name": "checkout-api",
            "status": "succeeded",
            "started_at": "2026-08-20T08:59:00.000Z",
            "finished_at": "2026-08-20T09:00:00.000Z",
            "commit_sha": "abcdef1234567890"
        },
        "observed_after": "2026-08-20T09:00:00.000Z",
        "observed_until": "2026-08-20T09:10:00.000Z",
        "recurrence_status": "available",
        "recurrence_count": 1,
        "first_recurrence": {
            "id": "33333333-3333-4333-8333-333333333333",
            "occurred_at": "2026-08-20T09:04:59.000Z",
            "ingested_at": "2026-08-20T09:05:00.000Z",
            "environment": "production",
            "release": "checkout@1.1.0",
            "service_name": "checkout-api",
            "trace_id": TRACE_ID,
            "sdk": {"name": "logbrew-node", "version": "0.1.8"}
        },
        "trace_health_status": "available",
        "trace_health": {
            "status": "errors_observed",
            "trace_count": 2,
            "error_trace_count": 1,
            "error_rate_basis_points": 5000
        },
        "causality": "evidence_only",
        "absence_is_proof": false,
        "retained_telemetry_only": true,
        "evidence": {
            "status": "complete",
            "captured_fields": [
                "baseline_occurrence",
                "candidate_deployment",
                "candidate_trace_health",
                "observation_window",
                "same_issue_recurrence"
            ],
            "missing_fields": [],
            "redacted_fields": [],
            "truncated_fields": []
        },
        "next_action": "inspect_recurrence"
    })
}

fn occurrence_analysis() -> serde_json::Value {
    serde_json::json!({
        "status": "complete",
        "coverage": {
            "retained_occurrences": 3,
            "trend_occurrences": 3,
            "available_distribution_count": 4,
            "expected_distribution_count": 4,
            "max_buckets": 30,
            "max_values_per_dimension": 10
        },
        "trend": {
            "scope_start": "2026-08-04T07:30:00Z",
            "scope_end": "2026-08-04T08:00:00Z",
            "interval_seconds": 300,
            "buckets": [
                {"bucket_start": "2026-08-04T07:30:00Z", "bucket_end": "2026-08-04T07:35:00Z", "occurrence_count": 1},
                {"bucket_start": "2026-08-04T07:35:00Z", "bucket_end": "2026-08-04T07:40:00Z", "occurrence_count": 0},
                {"bucket_start": "2026-08-04T07:40:00Z", "bucket_end": "2026-08-04T07:45:00Z", "occurrence_count": 0},
                {"bucket_start": "2026-08-04T07:45:00Z", "bucket_end": "2026-08-04T07:50:00Z", "occurrence_count": 0},
                {"bucket_start": "2026-08-04T07:50:00Z", "bucket_end": "2026-08-04T07:55:00Z", "occurrence_count": 0},
                {"bucket_start": "2026-08-04T07:55:00Z", "bucket_end": "2026-08-04T08:00:00Z", "occurrence_count": 1},
                {"bucket_start": "2026-08-04T08:00:00Z", "bucket_end": "2026-08-04T08:05:00Z", "occurrence_count": 1}
            ]
        },
        "distributions": [
            {
                "dimension": "release",
                "distinct_value_count": 1,
                "values": [{"value": "checkout@1.2.3", "version": null, "occurrence_count": 3, "share_basis_points": 10000}],
                "other_occurrence_count": 0
            },
            {
                "dimension": "environment",
                "distinct_value_count": 1,
                "values": [{"value": "production", "version": null, "occurrence_count": 3, "share_basis_points": 10000}],
                "other_occurrence_count": 0
            },
            {
                "dimension": "service",
                "distinct_value_count": 1,
                "values": [{"value": "checkout-api", "version": null, "occurrence_count": 3, "share_basis_points": 10000}],
                "other_occurrence_count": 0
            },
            {
                "dimension": "sdk",
                "distinct_value_count": 2,
                "values": [
                    {"value": "@logbrew/node", "version": "0.1.4", "occurrence_count": 2, "share_basis_points": 6666},
                    {"value": "@logbrew/node", "version": "0.1.3", "occurrence_count": 1, "share_basis_points": 3333}
                ],
                "other_occurrence_count": 0
            }
        ],
        "limitations": []
    })
}

fn selected_occurrence_summary() -> serde_json::Value {
    serde_json::json!({
        "id": OCCURRENCE_ID,
        "occurred_at": "2026-08-04T07:59:59Z",
        "severity": "error",
        "environment": "production",
        "release": "checkout@1.2.3",
        "service_name": "checkout-api",
        "sdk": {"name": "@logbrew/node", "version": "0.1.4"},
        "exception_type": "PaymentProviderError",
        "trace_linked": true,
        "stack": {"frame_count": 1, "truncated": false},
        "breadcrumbs": {"count": 1, "truncated": false},
        "context_captured": true
    })
}

fn sparse_occurrence_summary(
    id: &str,
    occurred_at: &str,
    release: &str,
    sdk_version: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "occurred_at": occurred_at,
        "severity": "error",
        "environment": "production",
        "release": release,
        "service_name": "checkout-api",
        "sdk": {"name": "@logbrew/node", "version": sdk_version},
        "exception_type": null,
        "trace_linked": false,
        "stack": {"frame_count": 0, "truncated": false},
        "breadcrumbs": {"count": 0, "truncated": false},
        "context_captured": false
    })
}

fn regressed_investigation_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["subject"]["status"] = serde_json::json!("resolved");
    bundle["lifecycle"] = serde_json::json!({
        "persisted_status": "resolved",
        "effective_status": "regressed",
        "activity": {
            "status": "available",
            "changes": [
                {
                    "id": "66666666-6666-4666-8666-666666666666",
                    "status": "resolved",
                    "changed_at": "2026-08-04T07:45:00Z"
                },
                {
                    "id": "77777777-7777-4777-8777-777777777777",
                    "status": "unresolved",
                    "changed_at": "2026-08-04T07:30:00Z"
                }
            ],
            "truncated": false
        },
        "regression": {
            "status": "detected",
            "reason": "occurrence_ingested_after_resolution",
            "resolution_changed_at": "2026-08-04T07:45:00Z",
            "first_reappeared_occurrence": {
                "id": OCCURRENCE_ID,
                "occurred_at": "2026-08-04T07:59:59Z",
                "ingested_at": "2026-08-04T08:00:01Z",
                "environment": "production",
                "release": "checkout@1.2.3",
                "service_name": "checkout-api",
                "trace_id": TRACE_ID,
                "sdk": {"name": "@logbrew/node", "version": "0.1.4"}
            }
        }
    });
    bundle["next_actions"][5]["reason"] = serde_json::json!("regression_detected");
    bundle
}

fn status_history_unavailable_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["lifecycle"] = serde_json::json!({
        "persisted_status": "unresolved",
        "effective_status": null,
        "activity": {"status": "unavailable", "changes": [], "truncated": false},
        "regression": {
            "status": "unavailable",
            "reason": "status_history_unavailable",
            "resolution_changed_at": null,
            "first_reappeared_occurrence": null
        }
    });
    move_evidence_field_to_missing(&mut bundle, "lifecycle.status_history");
    move_evidence_field_to_missing(&mut bundle, "lifecycle.regression");
    bundle
}

fn recurrence_unavailable_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["subject"]["status"] = serde_json::json!("resolved");
    bundle["lifecycle"] = serde_json::json!({
        "persisted_status": "resolved",
        "effective_status": null,
        "activity": {
            "status": "available",
            "changes": [{
                "id": "66666666-6666-4666-8666-666666666666",
                "status": "resolved",
                "changed_at": "2026-08-04T07:45:00Z"
            }],
            "truncated": false
        },
        "regression": {
            "status": "unavailable",
            "reason": "recurrence_read_unavailable",
            "resolution_changed_at": "2026-08-04T07:45:00Z",
            "first_reappeared_occurrence": null
        }
    });
    move_evidence_field_to_missing(&mut bundle, "lifecycle.regression");
    bundle
}

fn trend_unavailable_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["occurrence_analysis"]["status"] = serde_json::json!("partial");
    bundle["occurrence_analysis"]["coverage"]["trend_occurrences"] = serde_json::Value::Null;
    bundle["occurrence_analysis"]["trend"] = serde_json::Value::Null;
    bundle["occurrence_analysis"]["limitations"] = serde_json::json!(["trend_read_unavailable"]);
    move_evidence_field_to_missing(&mut bundle, "occurrence.trend");
    bundle
}

fn occurrence_analysis_unavailable_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["occurrence_analysis"] = serde_json::json!({
        "status": "unavailable",
        "coverage": {
            "retained_occurrences": 3,
            "trend_occurrences": null,
            "available_distribution_count": 0,
            "expected_distribution_count": 4,
            "max_buckets": 30,
            "max_values_per_dimension": 10
        },
        "trend": null,
        "distributions": [],
        "limitations": [
            "trend_read_unavailable",
            "release_distribution_unavailable",
            "environment_distribution_unavailable",
            "service_distribution_unavailable",
            "sdk_distribution_unavailable"
        ]
    });
    for field in [
        "occurrence.trend",
        "occurrence.distribution.release",
        "occurrence.distribution.environment",
        "occurrence.distribution.service",
        "occurrence.distribution.sdk",
    ] {
        move_evidence_field_to_missing(&mut bundle, field);
    }
    bundle
}

fn move_evidence_field_to_missing(bundle: &mut serde_json::Value, field: &str) {
    remove_evidence_fields(bundle, &[field]);
    add_evidence_field(bundle, "missing_fields", field);
}

fn projected_stack_investigation_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["occurrence_selection"]["selected"]["stack"]["frame_count"] = serde_json::json!(17);
    bundle["occurrence_selection"]["recommended"]["stack"]["frame_count"] = serde_json::json!(17);
    add_evidence_field(&mut bundle, "truncated_fields", "stack_frames");
    bundle
}

fn not_captured_exception_chain_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["event"]["exception_chain"] = serde_json::json!({
        "status": "not_captured",
        "entries": [],
        "truncated": false
    });
    remove_cause_signal(&mut bundle, "runtime_exception_chain");
    remove_evidence_fields(
        &mut bundle,
        &[
            "exception_chain",
            "exception_chain.messages",
            "exception_chain.stack_frames",
            "exception_chain.entries",
        ],
    );
    add_evidence_field(&mut bundle, "missing_fields", "exception_chain");
    bundle
}

fn invalid_exception_chain_bundle() -> serde_json::Value {
    let mut bundle = not_captured_exception_chain_bundle();
    bundle["event"]["exception_chain"] = serde_json::json!({
        "status": "invalid",
        "entries": [],
        "truncated": true
    });
    add_evidence_field(&mut bundle, "truncated_fields", "exception_chain");
    bundle
}

fn underlying_exception_fix_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["event"]["exception_chain"]["entries"][1]["stack_frames"] =
        serde_json::json!([captured_stack_frame((
            "checkout.provider",
            "requestPayment",
            "apps/checkout/provider_client.ts",
            142,
            9,
        ))]);
    bundle["event"]["exception_chain"]["entries"][1]["stack_frames_state"] =
        serde_json::json!("truncated");
    add_evidence_field(
        &mut bundle,
        "truncated_fields",
        "exception_chain.stack_frames",
    );
    bundle["fix"] = serde_json::json!({
        "status": "observed_underlying_exception_frame",
        "location": {
            "component": null,
            "module": "checkout.provider",
            "function": "requestPayment",
            "file": "apps/checkout/provider_client.ts",
            "line": 142,
            "column": 9,
            "in_app": true,
            "source_exception_id": 1
        },
        "provenance": "backend_observed"
    });
    bundle["source_locator"]["repository_relative_file"] =
        serde_json::json!("apps/checkout/provider_client.ts");
    bundle
}

fn remove_cause_signal(bundle: &mut serde_json::Value, signal: &str) {
    bundle["cause"]["signals"]
        .as_array_mut()
        .expect("cause signals are an array")
        .retain(|value| value != signal);
}

fn remove_evidence_fields(bundle: &mut serde_json::Value, fields: &[&str]) {
    for category in [
        "captured_fields",
        "missing_fields",
        "redacted_fields",
        "truncated_fields",
    ] {
        bundle["evidence"][category]
            .as_array_mut()
            .expect("evidence category is an array")
            .retain(|value| value.as_str().is_none_or(|value| !fields.contains(&value)));
    }
}

fn add_evidence_field(bundle: &mut serde_json::Value, category: &str, field: &str) {
    let fields = bundle["evidence"][category]
        .as_array_mut()
        .expect("evidence category is an array");
    fields.push(serde_json::json!(field));
    fields.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
}

fn captured_stack_frame(frame: (&str, &str, &str, u32, u32)) -> serde_json::Value {
    let (module, function, file, line, column) = frame;
    serde_json::json!({
        "index": 0,
        "module": module,
        "function": function,
        "file": file,
        "line": line,
        "column": column,
        "in_app": true,
        "source": "captured"
    })
}

fn mutated_bundles(cases: Vec<(&str, serde_json::Value)>) -> Vec<serde_json::Value> {
    cases
        .into_iter()
        .map(|(pointer, value)| {
            let mut bundle = rich_investigation_bundle();
            *bundle.pointer_mut(pointer).expect("fixture pointer exists") = value;
            mark_invalid(bundle)
        })
        .collect()
}

fn mark_invalid(mut bundle: serde_json::Value) -> serde_json::Value {
    bundle["subject"]["message"] = serde_json::json!(HOSTILE_MARKER);
    bundle
}

fn invalid_event_evidence_bundles() -> Vec<serde_json::Value> {
    let mut cases = mutated_bundles(vec![
        (
            "/event/exception_chain/entries/1/parent_id",
            serde_json::json!(7),
        ),
        (
            "/event/exception_chain/entries/0/type",
            serde_json::json!("mismatch"),
        ),
        (
            "/event/exception_chain/entries/1/message",
            serde_json::json!("redacted"),
        ),
        (
            "/event/request/route_template",
            serde_json::json!("/checkout/12345"),
        ),
        ("/event/request/method", serde_json::json!("INVALID")),
        (
            "/reproduction/status",
            serde_json::json!("insufficient_evidence"),
        ),
        (
            "/reproduction/baseline_occurrence_id",
            serde_json::json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
        ),
        (
            "/reproduction/evidence/status",
            serde_json::json!("partial"),
        ),
    ]);

    let mut missing_receipt = rich_investigation_bundle();
    remove_evidence_fields(&mut missing_receipt, &["exception_chain"]);
    cases.push(mark_invalid(missing_receipt));

    let mut forged_fix = underlying_exception_fix_bundle();
    forged_fix["fix"]["location"]["file"] = serde_json::json!("forged.rs");
    cases.push(mark_invalid(forged_fix));

    let mut missing_signal = rich_investigation_bundle();
    remove_cause_signal(&mut missing_signal, "runtime_exception_chain");
    cases.push(mark_invalid(missing_signal));

    let mut missing_request_receipt = rich_investigation_bundle();
    remove_evidence_fields(&mut missing_request_receipt, &["request.route_template"]);
    cases.push(mark_invalid(missing_request_receipt));

    let mut forged_reproduction = rich_investigation_bundle();
    forged_reproduction["reproduction"]["status"] = serde_json::json!("insufficient_evidence");
    forged_reproduction["reproduction"]["evidence"] = serde_json::json!({
        "status": "partial",
        "captured_fields": ["reproduction.baseline_occurrence", "reproduction.code_location",
            "reproduction.expected_failure"],
        "missing_fields": ["reproduction.trigger"],
        "redacted_fields": [], "truncated_fields": []
    });
    cases.push(mark_invalid(forged_reproduction));

    for field in ["request.body", "request.raw_url"] {
        let mut invalid_request_receipt = rich_investigation_bundle();
        add_evidence_field(&mut invalid_request_receipt, "captured_fields", field);
        cases.push(mark_invalid(invalid_request_receipt));
    }

    let mut missing_request = rich_investigation_bundle();
    let _removed = missing_request["event"]
        .as_object_mut()
        .expect("event")
        .remove("request");
    remove_evidence_fields(
        &mut missing_request,
        &[
            "request",
            "request.method",
            "request.route_template",
            "request.response_status_code",
        ],
    );
    add_evidence_field(&mut missing_request, "missing_fields", "request");
    missing_request["evidence"]["status"] = serde_json::json!("complete");
    cases.push(mark_invalid(missing_request));

    cases.extend(mutated_bundles(vec![
        ("/grouping/strategy", serde_json::json!("unknown")),
        ("/grouping/components/0", serde_json::json!("title")),
        ("/grouping/stack/frame_limit", serde_json::json!(9)),
        (
            "/grouping/stack/additional_frames_ignored",
            serde_json::json!(true),
        ),
    ]));
    let mut unknown_grouping_receipt = rich_investigation_bundle();
    add_evidence_field(
        &mut unknown_grouping_receipt,
        "captured_fields",
        "grouping.raw_value",
    );
    cases.push(mark_invalid(unknown_grouping_receipt));

    cases
}

fn invalid_occurrence_selection_bundles() -> Vec<serde_json::Value> {
    let mut cases = mutated_bundles(vec![
        (
            "/event/id",
            serde_json::json!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
        ),
        (
            "/occurrence_selection/first/occurred_at",
            serde_json::json!("2026-08-04T07:31:00Z"),
        ),
        (
            "/occurrence_selection/recommendation/candidate_count",
            serde_json::json!(2),
        ),
        (
            "/occurrence_selection/recommendation/candidate_window_truncated",
            serde_json::json!(true),
        ),
        (
            "/occurrence_selection/requested",
            serde_json::json!("first"),
        ),
        (
            "/correlations/release/release",
            serde_json::json!("checkout@hostile-release-marker"),
        ),
        (
            "/correlations/release/time_since_deployment_ms",
            serde_json::json!(478_999),
        ),
        (
            "/correlations/release/deployment/release",
            serde_json::json!("checkout@2.0.0"),
        ),
        (
            "/correlations/release/deployment_causality",
            serde_json::json!("causal"),
        ),
        (
            "/event/context/resource/service/name",
            serde_json::json!("hostile-service-marker"),
        ),
        (
            "/event/context/resource/runtime/version",
            serde_json::json!("/opt/example/runtime"),
        ),
        (
            "/event/context",
            serde_json::json!({
                "schema_version": 1, "resource": null, "trace": null,
                "session": null, "subject": null, "tags": {}
            }),
        ),
        (
            "/event/context/trace/trace_id",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/correlations/trace/summary/trace_id",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/correlations/related_issues/items/0/issue_id",
            serde_json::json!(ISSUE_ID),
        ),
        (
            "/correlations/related_issues/items/0/environment",
            serde_json::json!("staging"),
        ),
        (
            "/correlations/related_issues/items/0/severity",
            serde_json::json!("debug"),
        ),
        (
            "/correlations/related_issues/status",
            serde_json::json!("not_found"),
        ),
        (
            "/correlations/related_issues/truncated",
            serde_json::json!(true),
        ),
        ("/source_locator/status", serde_json::json!("not_found")),
        (
            "/source_locator/repository_provider",
            serde_json::json!("unknown"),
        ),
        (
            "/source_locator/revision_source",
            serde_json::json!("setup_snapshot"),
        ),
        (
            "/source_locator/repository_relative_file",
            serde_json::json!("../payment_gateway.ts"),
        ),
    ]);

    let mut unknown_context = rich_investigation_bundle();
    let _previous = unknown_context["event"]["context"]
        .as_object_mut()
        .expect("context")
        .insert("raw_subject_id".to_owned(), serde_json::json!("private"));
    cases.push(mark_invalid(unknown_context));

    let mut unknown_source_receipt = rich_investigation_bundle();
    unknown_source_receipt["source_locator"]["evidence"]["captured_fields"] =
        serde_json::json!(["source.raw_url"]);
    cases.push(mark_invalid(unknown_source_receipt));

    let mut duplicate_issue = rich_investigation_bundle();
    let mut sibling = duplicate_issue["correlations"]["related_issues"]["items"][0].clone();
    sibling["id"] = serde_json::json!("88888888-8888-4888-8888-888888888888");
    duplicate_issue["correlations"]["related_issues"]["items"]
        .as_array_mut()
        .expect("related issues")
        .push(sibling);
    cases.push(mark_invalid(duplicate_issue));

    let mut missing_projection = projected_stack_investigation_bundle();
    missing_projection["evidence"]["truncated_fields"] = serde_json::json!([]);
    cases.push(mark_invalid(missing_projection));
    let mut projection_exceeds = rich_investigation_bundle();
    let frame = projection_exceeds["event"]["stack_frames"][0].clone();
    projection_exceeds["event"]["stack_frames"]
        .as_array_mut()
        .expect("stack frames")
        .push(frame);
    projection_exceeds["evidence"]["truncated_fields"] = serde_json::json!(["stack_frames"]);
    cases.push(mark_invalid(projection_exceeds));
    let mut unreceipted = rich_investigation_bundle();
    for selection in ["selected", "recommended"] {
        unreceipted["occurrence_selection"][selection]["stack"]["truncated"] =
            serde_json::json!(true);
    }
    cases.push(mark_invalid(unreceipted));
    cases
}

fn invalid_lifecycle_bundles() -> Vec<serde_json::Value> {
    let mut cases = mutated_bundles(vec![
        ("/lifecycle/persisted_status", serde_json::json!("ignored")),
        (
            "/lifecycle/effective_status",
            serde_json::json!("regressed"),
        ),
        (
            "/lifecycle/activity/status",
            serde_json::json!("unavailable"),
        ),
    ]);

    let mut duplicate_activity = regressed_investigation_bundle();
    let duplicate = duplicate_activity["lifecycle"]["activity"]["changes"][0].clone();
    duplicate_activity["lifecycle"]["activity"]["changes"]
        .as_array_mut()
        .expect("activity is an array")[1] = duplicate;
    cases.push(mark_invalid(duplicate_activity));

    for (pointer, value) in [
        (
            "/lifecycle/activity/changes/1/changed_at",
            serde_json::json!("2026-08-04T07:46:00Z"),
        ),
        (
            "/lifecycle/regression/first_reappeared_occurrence",
            serde_json::Value::Null,
        ),
        (
            "/lifecycle/regression/first_reappeared_occurrence/ingested_at",
            serde_json::json!("2026-08-04T07:44:59Z"),
        ),
        (
            "/next_actions/5/reason",
            serde_json::json!("release_identity_available"),
        ),
    ] {
        let mut bundle = regressed_investigation_bundle();
        *bundle
            .pointer_mut(pointer)
            .expect("regression fixture pointer") = value;
        cases.push(mark_invalid(bundle));
    }

    let mut missing_evidence_receipt = rich_investigation_bundle();
    missing_evidence_receipt["evidence"]["captured_fields"]
        .as_array_mut()
        .expect("captured fields are an array")
        .retain(|field| field != "lifecycle.regression");
    cases.push(mark_invalid(missing_evidence_receipt));

    cases
}

fn invalid_occurrence_analysis_bundles() -> Vec<serde_json::Value> {
    let mut cases = mutated_bundles(vec![
        (
            "/occurrence_analysis/coverage/retained_occurrences",
            serde_json::json!(9_007_199_254_740_992_u64),
        ),
        (
            "/occurrence_analysis/trend/buckets/0/occurrence_count",
            serde_json::json!(2),
        ),
        (
            "/occurrence_analysis/trend/buckets/1/bucket_start",
            serde_json::json!("2026-08-04T07:36:00Z"),
        ),
        (
            "/occurrence_analysis/distributions/3/values/0/share_basis_points",
            serde_json::json!(6667),
        ),
        (
            "/occurrence_analysis/distributions/0/other_occurrence_count",
            serde_json::json!(1),
        ),
        (
            "/occurrence_analysis/distributions/3/values/0/occurrence_count",
            serde_json::json!(1),
        ),
        (
            "/occurrence_analysis/distributions/3/values/0/version",
            serde_json::Value::Null,
        ),
        (
            "/occurrence_analysis/coverage/available_distribution_count",
            serde_json::json!(3),
        ),
        ("/occurrence_analysis/status", serde_json::json!("partial")),
        (
            "/occurrence_analysis/limitations",
            serde_json::json!(["trend_read_unavailable"]),
        ),
    ]);

    let mut missing_receipt = rich_investigation_bundle();
    missing_receipt["evidence"]["captured_fields"]
        .as_array_mut()
        .expect("captured fields")
        .retain(|field| field != "occurrence.trend");
    cases.push(mark_invalid(missing_receipt));
    for field in [
        "occurrence.trend",
        "occurrence.distribution.release.unknown",
    ] {
        let mut bundle = rich_investigation_bundle();
        bundle["evidence"]["captured_fields"]
            .as_array_mut()
            .expect("captured fields")
            .push(serde_json::json!(field));
        cases.push(mark_invalid(bundle));
    }

    let mut unreceipted_truncation = rich_investigation_bundle();
    unreceipted_truncation["occurrence_analysis"]["distributions"][0]["distinct_value_count"] =
        serde_json::json!(2);
    unreceipted_truncation["occurrence_analysis"]["distributions"][0]["values"][0]["occurrence_count"] =
        serde_json::json!(2);
    unreceipted_truncation["occurrence_analysis"]["distributions"][0]["values"][0]["share_basis_points"] =
        serde_json::json!(6666);
    unreceipted_truncation["occurrence_analysis"]["distributions"][0]["other_occurrence_count"] =
        serde_json::json!(1);
    cases.push(mark_invalid(unreceipted_truncation));
    cases
}

fn invalid_user_impact_bundles() -> Vec<serde_json::Value> {
    mutated_bundles(vec![
        (
            "/impact/user_impact/known_affected_users",
            serde_json::json!(9_007_199_254_740_992_u64),
        ),
        (
            "/impact/user_impact/coverage/retained_occurrences",
            serde_json::json!(4),
        ),
        (
            "/impact/user_impact/coverage/index_coverage_basis_points",
            serde_json::json!(9_999),
        ),
        (
            "/impact/user_impact/known_affected_users",
            serde_json::json!(3),
        ),
        (
            "/impact/user_impact/limitations",
            serde_json::json!(["approximate_distinct_count", "approximate_distinct_count"]),
        ),
        (
            "/impact/user_impact/limitations",
            serde_json::json!([
                "unknown-limitation-marker",
                "privacy_filtered_subject_context"
            ]),
        ),
        ("/impact/affected_users", serde_json::json!(2)),
        ("/impact/user_impact/coverage", serde_json::Value::Null),
        (
            "/impact/user_impact/status",
            serde_json::json!("unavailable"),
        ),
        (
            "/impact/user_impact/coverage/identified_user_coverage_basis_points",
            serde_json::Value::Null,
        ),
    ])
}

fn complete_user_impact_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["impact"]["affected_users"] = serde_json::json!(2);
    bundle["impact"]["user_impact"] = serde_json::json!({
        "status": "complete",
        "known_affected_users": 2,
        "count_method": "approximate_uniq_combined64",
        "coverage": {
            "retained_occurrences": 3,
            "indexed_occurrences": 3,
            "historical_unindexed_occurrences": 0,
            "identified_user_occurrences": 3,
            "anonymous_subject_occurrences": 0,
            "missing_subject_occurrences": 0,
            "privacy_filtered_subject_occurrences": 0,
            "index_coverage_basis_points": 10000,
            "identified_user_coverage_basis_points": 10000
        },
        "limitations": ["approximate_distinct_count"]
    });
    bundle["source_locator"]["revision"] = serde_json::json!("setup-snapshot-123");
    bundle["source_locator"]["revision_source"] = serde_json::json!("setup_snapshot");
    bundle["source_locator"]["repository_relative_file"] = serde_json::Value::Null;
    bundle["source_locator"]["evidence"] = serde_json::json!({
        "status": "partial",
        "captured_fields": ["source.component", "source.repository", "source.service", "source.setup_snapshot_revision"],
        "missing_fields": ["source.deployment_revision", "source.repository_relative_file"],
        "redacted_fields": [], "truncated_fields": []
    });
    add_evidence_field(&mut bundle, "missing_fields", "source_locator.complete");
    bundle
}

fn unavailable_user_impact_bundle() -> serde_json::Value {
    let mut bundle = rich_investigation_bundle();
    bundle["impact"]["user_impact"] = serde_json::json!({
        "status": "unavailable",
        "known_affected_users": null,
        "count_method": "unavailable",
        "coverage": null,
        "limitations": ["user_impact_read_unavailable"]
    });
    bundle["source_locator"] = serde_json::json!({
        "status": "not_found",
        "repository_provider": null, "repository_full_name": null, "component_path": null,
        "revision": null, "revision_source": null, "repository_relative_file": null,
        "evidence": {
            "status": "partial", "captured_fields": ["source.service"],
            "missing_fields": ["source.component"], "redacted_fields": [], "truncated_fields": []
        }
    });
    add_evidence_field(&mut bundle, "missing_fields", "source_locator.complete");
    bundle
}
