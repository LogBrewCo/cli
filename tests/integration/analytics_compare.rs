//! Built-binary contract proof for bounded, descriptive segment comparisons.

use logbrew_cli::{Command, HelpTopic, HttpMethod, help, parse_command};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_request_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "compare", "--help"],
        vec!["logbrew", "help", "analytics", "segments"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::AnalyticsCompare,
                json: false,
            }
        );
    }
    let command = parse_command(compare_args(true, "old=Old release"))?;
    assert_eq!(
        command.http_path().as_deref(),
        Some("/api/telemetry/analytics/segments/compare")
    );
    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert!(command.wants_json());
    let body = command.request_body().ok_or("comparison body missing")?;
    assert_eq!(body["analysis_unit"], "session");
    assert_eq!(body["target"]["kind"], "interaction");
    assert_eq!(body["target"]["event_name"], "checkout_completed");
    assert_eq!(body["segments"][0]["key"], "old");
    assert_eq!(body["segments"][0]["release"], "1.0.0");
    assert_eq!(body["segments"][1]["release"], "1.1.0");

    let text = help::help_text(HelpTopic::AnalyticsCompare);
    assert!(text.contains("first segment is the descriptive baseline"));
    assert!(text.contains("Segments are evaluated independently and may overlap"));
    assert!(text.contains("no causal inference or statistical significance"));
    assert!(text.contains("exact validated schema-version-1 response"));
    Ok(())
}

#[test]
fn property_predicates_are_bounded_canonical_and_part_of_segment_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(property_compare_args(true))?;
    let body = command.request_body().ok_or("comparison body missing")?;

    assert_eq!(
        body["segments"][0]["property_filters"][0]["key"],
        "resource.framework.name"
    );
    assert_eq!(body["segments"][0]["property_filters"][0]["value"], "React");
    assert_eq!(
        body["segments"][0]["property_filters"][1]["key"],
        "tag.plan"
    );
    assert_eq!(body["segments"][0]["property_filters"][1]["value"], "free");
    assert_eq!(
        body["segments"][1]["property_filters"][0]["key"],
        "tag.plan"
    );
    assert_eq!(body["segments"][1]["property_filters"][0]["value"], "pro");

    let help = help::help_text(HelpTopic::AnalyticsCompare);
    assert!(help.contains("--segment-property <segment>:<key>=<value>"));
    assert!(help.contains("missing-key coverage"));
    Ok(())
}

#[tokio::test]
async fn built_binary_posts_exact_segments_and_preserves_validated_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = comparison_response("Old release");
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/segments/compare"))
        .and(header("authorization", "Bearer account-token"))
        .and(body_json(serde_json::json!({
            "project_id": PROJECT_ID,
            "since": "7d",
            "interval": "1h",
            "analysis_unit": "session",
            "target": {"kind": "interaction", "event_name": "checkout_completed"},
            "segments": [
                {
                    "key": "old",
                    "label": "Old release",
                    "service_name": "checkout",
                    "release": "1.0.0",
                    "environment": "production"
                },
                {
                    "key": "new",
                    "label": "New release",
                    "service_name": "checkout",
                    "release": "1.1.0",
                    "environment": "production"
                }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, true, "old=Old release").await?;

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
async fn built_binary_human_output_explains_reach_differences_coverage_and_limits()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/segments/compare"))
        .respond_with(ResponseTemplate::new(200).set_body_json(comparison_response("Old release")))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false, "old=Old release").await?;

    assert!(process.status.success());
    assert!(process.stderr.is_empty());
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Product segment comparison interaction checkout_completed",
        "unit: session; baseline: old",
        "old (Old release) [baseline]",
        "Reach: 10/50 (20.0%) | target events: 20 (18 usable)",
        "new (New release)",
        "Versus baseline: eligible +10; reached +8; target events +16; reach +10.0 pp; relative lift +50.0%",
        "Coverage: unit 90/100 (90.0%) | target unit 18/20 (90.0%) | trace-linked target 15/20 (75.0%)",
        "Capture gap: 10 classified events lacked context.session.id.",
        "Correlation gap: 5 target events lacked a trace ID.",
        "descriptive only; unique-unit counts are approximate",
        "no causality or statistical significance is established",
        "Next: inspect paths around the target in the weakest segment, then follow correlated traces",
    ] {
        assert!(text.contains(expected), "missing human detail: {expected}");
    }
    assert!(!text.contains("Repeat this server-authored reason verbatim."));
    Ok(())
}

#[tokio::test]
async fn built_binary_accepts_typed_user_identity_coverage_target()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = comparison_response("Old release");
    response["query"]["analysis_unit"] = "identified_user".into();
    response["segments"][0]["coverage"]["unit_identified_events"] = 70.into();
    response["segments"][0]["coverage"]["excluded_events"] = 30.into();
    response["segments"][0]["coverage"]["unit_coverage_rate"] = 0.7.into();
    response["next_action"]["code"] = "improve_identity_coverage".into();
    response["next_action"]["target"] = "context.subject.kind=user".into();
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/segments/compare"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let mut args = compare_args(false, "old=Old release");
    args.extend([
        "--unit".to_owned(),
        "identified-user".to_owned(),
        "--json".to_owned(),
    ]);
    let process = run_binary_args(&server, args).await?;

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
async fn built_binary_separates_missing_properties_from_nonmatching_values()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = property_comparison_response();
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/segments/compare"))
        .and(body_json(
            parse_command(property_compare_args(false))?
                .request_body()
                .ok_or("comparison body missing")?,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary_args(&server, property_compare_args(false)).await?;

    assert!(
        process.status.success(),
        "built binary failed: {}",
        String::from_utf8_lossy(process.stderr.as_slice())
    );
    assert!(process.stderr.is_empty());
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Property predicates: 2 exact case-sensitive values across resource.framework.name, tag.plan; values hidden in human output.",
        "Property coverage: ready 120/130 (92.3%) | matched 100/120 (83.3%) | missing keys 10 | nonmatching values 20",
        "Property predicates: 1 exact case-sensitive value across tag.plan; values hidden in human output.",
        "Property coverage: ready 140/150 (93.3%) | matched 120/140 (85.7%) | missing keys 10 | nonmatching values 20",
    ] {
        assert!(
            text.contains(expected),
            "missing property detail: {expected}"
        );
    }
    assert!(!text.contains("tag.plan=free"));
    assert!(!text.contains("tag.plan=pro"));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_contradictory_property_populations()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = property_comparison_response();
    response["segments"][0]["coverage"]["property_filters"]["missing_property_events"] = 9.into();
    response["next_action"]["reason"] = "property-contradiction-marker".into();
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/segments/compare"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary_args(&server, property_compare_args(true)).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    let error: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(error["error"], "analytics_segment_response_invalid");
    assert!(!text.contains("property-contradiction-marker"));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_unknown_identity_and_contradictory_difference_fields()
-> Result<(), Box<dyn std::error::Error>> {
    for response in [
        {
            let mut response = comparison_response("Old release");
            response["segments"][0]["distinct_id"] = "hostile-subject-marker".into();
            response
        },
        {
            let mut response = comparison_response("Old release");
            response["segments"][1]["comparison_to_baseline"]["reached_units_difference"] =
                9.into();
            response
        },
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/telemetry/analytics/segments/compare"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;

        let process = run_binary(&server, true, "old=Old release").await?;

        assert!(!process.status.success());
        assert!(process.stdout.is_empty());
        let text = String::from_utf8(process.stderr)?;
        let error: serde_json::Value = serde_json::from_str(text.as_str())?;
        assert_eq!(error["error"], "analytics_segment_response_invalid");
        assert!(!text.contains("hostile-subject-marker"));
    }
    Ok(())
}

#[tokio::test]
async fn built_binary_escapes_bidirectional_segment_labels_in_human_output()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let label = "Old \u{202e}release";
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/segments/compare"))
        .respond_with(ResponseTemplate::new(200).set_body_json(comparison_response(label)))
        .expect(1)
        .mount(&server)
        .await;

    let argument = format!("old={label}");
    let process = run_binary(&server, false, argument.as_str()).await?;

    assert!(process.status.success());
    let text = String::from_utf8(process.stdout)?;
    assert!(text.contains(r"Old \u{202e}release"));
    assert!(!text.contains(label));
    Ok(())
}

/// Runs the actual CLI process while the async loopback server remains responsive.
async fn run_binary(
    server: &MockServer,
    json: bool,
    baseline: &str,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    run_binary_args(server, compare_args(json, baseline)).await
}

/// Runs the actual CLI process for one fully assembled comparison invocation.
async fn run_binary_args(
    server: &MockServer,
    args: Vec<String>,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    super::run_cli(server, args.into_iter().skip(1)).await
}

/// Builds one property-based comparison with values known only by the caller.
fn property_compare_args(json: bool) -> Vec<String> {
    let mut args = [
        "logbrew",
        "analytics",
        "compare",
        "--project",
        PROJECT_ID,
        "--since",
        "7d",
        "--target-kind",
        "interaction",
        "--target-event",
        "checkout_completed",
        "--segment",
        "old=Old release",
        "--segment",
        "new=New release",
        "--segment-service",
        "old=checkout",
        "--segment-service",
        "new=checkout",
        "--segment-release",
        "old=1.0.0",
        "--segment-release",
        "new=1.1.0",
        "--segment-environment",
        "old=production",
        "--segment-environment",
        "new=production",
        "--segment-property",
        "old:tag.plan=free",
        "--segment-property",
        "old:resource.framework.name=React",
        "--segment-property",
        "new:tag.plan=pro",
        "--interval",
        "1h",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if json {
        args.push("--json".to_owned());
    }
    args
}

/// Builds one exact public comparison invocation.
fn compare_args(json: bool, baseline: &str) -> Vec<String> {
    let mut args = [
        "logbrew",
        "analytics",
        "compare",
        "--project",
        PROJECT_ID,
        "--since",
        "7d",
        "--target-kind",
        "interaction",
        "--target-event",
        "checkout_completed",
        "--segment",
        baseline,
        "--segment",
        "new=New release",
        "--segment-service",
        "old=checkout",
        "--segment-service",
        "new=checkout",
        "--segment-release",
        "old=1.0.0",
        "--segment-release",
        "new=1.1.0",
        "--segment-environment",
        "old=production",
        "--segment-environment",
        "new=production",
        "--interval",
        "1h",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if json {
        args.push("--json".to_owned());
    }
    args
}

/// Stable schema-version-1 fixture matching the API contract.
fn comparison_response(baseline_label: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "query": {
            "project_id": PROJECT_ID,
            "since": "2026-08-01T00:00:00Z",
            "until": "2026-08-01T02:00:00Z",
            "interval": "1h",
            "interval_seconds": 3600,
            "analysis_unit": "session",
            "target": {"kind": "interaction", "event_name": "checkout_completed"},
            "segments": [
                {
                    "key": "old",
                    "label": baseline_label,
                    "service_name": "checkout",
                    "release": "1.0.0",
                    "environment": "production"
                },
                {
                    "key": "new",
                    "label": "New release",
                    "service_name": "checkout",
                    "release": "1.1.0",
                    "environment": "production"
                }
            ]
        },
        "purpose": "Compares one exact outcome across bounded named context segments.",
        "summary": {
            "segment_count": 2,
            "baseline_segment_key": "old",
            "segments_with_eligible_units": 2,
            "segments_with_target_events": 2,
            "segments_with_reached_units": 2
        },
        "confidence": {
            "interpretation": "descriptive_only",
            "unique_count_accuracy": "approximate",
            "estimation_method": "clickhouse_uniq_combined64",
            "causal_inference": "not_established",
            "statistical_significance": "not_tested",
            "segment_overlap": "possible",
            "limitations": [
                "Unique-unit cardinalities are approximate.",
                "Segments may overlap.",
                "Differences are descriptive only.",
                "Bucket unique counts are non-additive.",
                "Sessions are visits, not people.",
                "At least one segment has incomplete unit coverage."
            ]
        },
        "segments": [
            {
                "key": "old",
                "label": baseline_label,
                "eligible_units": 50,
                "reached_units": 10,
                "reach_rate": 0.2,
                "usable_target_events_per_reached_unit": 1.8,
                "coverage": {
                    "classified_events": 100,
                    "unit_identified_events": 90,
                    "excluded_events": 10,
                    "unit_coverage_rate": 0.9,
                    "target_events": 20,
                    "usable_target_events": 18,
                    "excluded_target_events": 2,
                    "target_unit_coverage_rate": 0.9,
                    "traced_target_events": 15,
                    "target_trace_link_rate": 0.75
                },
                "series": [
                    {
                        "bucket_start": "2026-08-01T00:00:00Z",
                        "bucket_end": "2026-08-01T01:00:00Z",
                        "classified_events": 60,
                        "eligible_units": 35,
                        "target_events": 12,
                        "usable_target_events": 10,
                        "reached_units": 6,
                        "reach_rate": 0.171_428_571_428_571_43
                    },
                    {
                        "bucket_start": "2026-08-01T01:00:00Z",
                        "bucket_end": "2026-08-01T02:00:00Z",
                        "classified_events": 40,
                        "eligible_units": 25,
                        "target_events": 8,
                        "usable_target_events": 8,
                        "reached_units": 5,
                        "reach_rate": 0.2
                    }
                ]
            },
            {
                "key": "new",
                "label": "New release",
                "eligible_units": 60,
                "reached_units": 18,
                "reach_rate": 0.3,
                "usable_target_events_per_reached_unit": 2.0,
                "coverage": {
                    "classified_events": 120,
                    "unit_identified_events": 120,
                    "excluded_events": 0,
                    "unit_coverage_rate": 1.0,
                    "target_events": 36,
                    "usable_target_events": 36,
                    "excluded_target_events": 0,
                    "target_unit_coverage_rate": 1.0,
                    "traced_target_events": 30,
                    "target_trace_link_rate": 0.833_333_333_333_333_4
                },
                "series": [
                    {
                        "bucket_start": "2026-08-01T00:00:00Z",
                        "bucket_end": "2026-08-01T01:00:00Z",
                        "classified_events": 70,
                        "eligible_units": 40,
                        "target_events": 20,
                        "usable_target_events": 20,
                        "reached_units": 10,
                        "reach_rate": 0.25
                    },
                    {
                        "bucket_start": "2026-08-01T01:00:00Z",
                        "bucket_end": "2026-08-01T02:00:00Z",
                        "classified_events": 50,
                        "eligible_units": 30,
                        "target_events": 16,
                        "usable_target_events": 16,
                        "reached_units": 9,
                        "reach_rate": 0.3
                    }
                ],
                "comparison_to_baseline": {
                    "eligible_units_difference": 10,
                    "reached_units_difference": 8,
                    "target_events_difference": 16,
                    "reach_rate_difference": 0.1,
                    "relative_reach_rate_lift": 0.5
                }
            }
        ],
        "next_action": {
            "code": "investigate_segment_paths",
            "target": "/api/telemetry/analytics/paths",
            "reason": "Repeat this server-authored reason verbatim."
        }
    })
}

/// Adds exact property predicates and their disjoint readiness/value coverage receipts.
fn property_comparison_response() -> serde_json::Value {
    let mut response = comparison_response("Old release");
    response["query"]["segments"][0]["property_filters"] = serde_json::json!([
        {"key": "resource.framework.name", "value": "React"},
        {"key": "tag.plan", "value": "free"}
    ]);
    response["query"]["segments"][1]["property_filters"] =
        serde_json::json!([{"key": "tag.plan", "value": "pro"}]);
    response["segments"][0]["coverage"]["property_filters"] = serde_json::json!({
        "context_events": 130,
        "property_ready_events": 120,
        "missing_property_events": 10,
        "property_ready_rate": 120.0 / 130.0,
        "matching_events": 100,
        "nonmatching_value_events": 20,
        "match_rate": 100.0 / 120.0
    });
    response["segments"][1]["coverage"]["property_filters"] = serde_json::json!({
        "context_events": 150,
        "property_ready_events": 140,
        "missing_property_events": 10,
        "property_ready_rate": 140.0 / 150.0,
        "matching_events": 120,
        "nonmatching_value_events": 20,
        "match_rate": 120.0 / 140.0
    });
    response["confidence"]["limitations"]
        .as_array_mut()
        .expect("limitations are an array")
        .extend([
            serde_json::json!("Property predicates use exact case-sensitive values."),
            serde_json::json!("Sensitive custom property keys are excluded."),
        ]);
    response
}
