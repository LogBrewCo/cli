//! Built-binary contract proof for maturity-aware, identity-safe retention.

use crate::matchers::body_json;
use crate::{Mock, MockServer, ResponseTemplate};
use logbrew_cli::{Command, HelpTopic, HttpMethod, help, parse_command};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_request_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "retention", "--help"],
        vec!["logbrew", "help", "analytics", "retention"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::AnalyticsRetention,
                json: false,
            }
        );
    }
    let command = parse_command([
        "logbrew",
        "--json",
        "analytics",
        "retention",
        "--project",
        PROJECT_ID,
        "--since",
        "30d",
        "--start-kind",
        "page-view",
        "--start-event",
        "/signup",
        "--return-kind",
        "interaction",
        "--return-event",
        "dashboard_opened",
        "--interval-count",
        "2",
    ])?;
    assert_eq!(
        command.http_path().as_deref(),
        Some("/api/telemetry/analytics/retention")
    );
    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert!(command.wants_json());
    let body = command.request_body().ok_or("retention body missing")?;
    assert_eq!(body["start_event"]["kind"], "page_view");
    assert_eq!(body["return_event"]["event_name"], "dashboard_opened");
    assert_eq!(body["interval"], "day");
    assert_eq!(body["mode"], "return_on");
    assert_eq!(body["cohort_mode"], "first_in_range");

    let text = help::help_text(HelpTopic::AnalyticsRetention);
    assert!(text.contains("Maturity-aware denominators"));
    assert!(text.contains("Raw IDs are never returned"));
    assert!(text.contains("first-in-range does not prove"));
    assert!(text.contains("exact validated schema-version-1 response"));
    Ok(())
}

#[tokio::test]
async fn built_binary_posts_exact_scope_and_preserves_validated_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = retention_response();
    Mock::auth(
        "POST",
        "/api/telemetry/analytics/retention",
        "account-token",
    )
    .and(body_json(serde_json::json!({
        "project_id": PROJECT_ID,
        "since": "30d",
        "environment": "production",
        "start_event": {"kind": "page_view", "event_name": "/signup"},
        "return_event": {"kind": "interaction", "event_name": "dashboard_opened"},
        "interval": "day",
        "interval_count": 2,
        "mode": "return_on",
        "cohort_mode": "first_in_range"
    })))
    .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
    .expect(1)
    .mount(&server)
    .await;

    let process = run_binary(&server, true).await?;

    super::assert_cli_success(&process);
    let actual: serde_json::Value = serde_json::from_slice(process.stdout.as_slice())?;
    assert_eq!(actual, response);
    Ok(())
}

#[tokio::test]
async fn built_binary_human_output_explains_retention_maturity_coverage_and_next_step()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::route("POST", "/api/telemetry/analytics/retention")
        .respond_with(ResponseTemplate::new(200).set_body_json(retention_response()))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false).await?;

    assert!(process.status.success());
    assert!(process.stderr.is_empty());
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Product retention page_view /signup -> interaction dashboard_opened",
        "Cohort subjects: 10; returned after start: 6 (60.0%)",
        "P1 [86400s, 172800s): 2/6 retained (33.3%)",
        "C0 2026-08-01T00:00:00Z to 2026-08-02T00:00:00Z: 6 subjects",
        "Coverage: named 90/100; identified 80/100; usable starts 16/20; usable returns 14/18",
        "Capture gap: 20 classified events lacked an explicit opaque subject ID.",
        "Observation limit: 1/2 periods are fully observed",
        "Next: move since earlier, until later, or request fewer periods",
    ] {
        assert!(text.contains(expected), "missing human detail: {expected}");
    }
    assert!(!text.contains("Repeat this server-authored reason verbatim."));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_unknown_identity_fields_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = retention_response();
    response["cohorts"][0]["distinct_id"] = "hostile-subject-marker".into();
    Mock::route("POST", "/api/telemetry/analytics/retention")
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, true).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    let error: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(error["error"], "analytics_retention_response_invalid");
    assert!(!text.contains("hostile-subject-marker"));
    Ok(())
}

/// Runs the actual CLI process while the async loopback server remains responsive.
async fn run_binary(
    server: &MockServer,
    json: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let mut args = vec![
        "analytics",
        "retention",
        "--project",
        PROJECT_ID,
        "--since",
        "30d",
        "--start-kind",
        "page-view",
        "--start-event",
        "/signup",
        "--return-kind",
        "interaction",
        "--return-event",
        "dashboard_opened",
        "--environment",
        "production",
        "--interval-count",
        "2",
    ];
    if json {
        args.push("--json");
    }
    super::run_cli(server, args.as_slice()).await
}

/// Stable schema-version-1 fixture matching the API contract.
fn retention_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "query": {
            "project_id": PROJECT_ID,
            "since": "2026-08-01T00:00:00Z",
            "until": "2026-08-04T00:00:00Z",
            "environment": "production",
            "start_event": {"kind": "page_view", "event_name": "/signup"},
            "return_event": {"kind": "interaction", "event_name": "dashboard_opened"},
            "interval": "day",
            "interval_seconds": 86400,
            "interval_count": 2,
            "mode": "return_on",
            "cohort_mode": "first_in_range"
        },
        "purpose": "Measures maturity-aware identified-user retention.",
        "summary": {
            "cohort_subjects": 10,
            "subjects_returned_after_start": 6,
            "return_rate_after_start": 0.6,
            "non_empty_cohorts": 2,
            "periods_with_eligible_subjects": 2,
            "fully_observed_periods": 1
        },
        "coverage": {
            "classified_events": 100,
            "named_events": 90,
            "unnamed_events": 10,
            "identified_events": 80,
            "start_events": 20,
            "usable_start_events": 16,
            "excluded_start_events": 4,
            "return_events": 18,
            "usable_return_events": 14,
            "excluded_return_events": 4,
            "event_name_rate": 0.9,
            "start_identity_coverage_rate": 0.8,
            "return_identity_coverage_rate": 0.777_777_777_777_777_8,
            "limitations": ["Only explicit opaque subject IDs qualify."]
        },
        "curve": [
            {
                "period": 0,
                "threshold_seconds": 0,
                "window_end_seconds": 86400,
                "eligible_subjects": 10,
                "retained_subjects": 5,
                "weighted_retention_rate": 0.5,
                "fully_observed_cohorts": 2,
                "fully_observed_cohort_average_rate": 0.5,
                "all_subjects_eligible": true
            },
            {
                "period": 1,
                "threshold_seconds": 86400,
                "window_end_seconds": 172_800,
                "eligible_subjects": 6,
                "retained_subjects": 2,
                "weighted_retention_rate": 0.333_333_333_333_333_3,
                "fully_observed_cohorts": 1,
                "fully_observed_cohort_average_rate": 0.333_333_333_333_333_3,
                "all_subjects_eligible": false
            }
        ],
        "cohorts": [
            {
                "cohort": 0,
                "started_at": "2026-08-01T00:00:00Z",
                "ended_at": "2026-08-02T00:00:00Z",
                "subjects": 6,
                "subjects_returned_after_start": 4,
                "periods": [
                    {"period": 0, "eligible_subjects": 6, "retained_subjects": 3, "retention_rate": 0.5, "all_subjects_eligible": true},
                    {"period": 1, "eligible_subjects": 6, "retained_subjects": 2, "retention_rate": 0.333_333_333_333_333_3, "all_subjects_eligible": true}
                ]
            },
            {
                "cohort": 1,
                "started_at": "2026-08-02T00:00:00Z",
                "ended_at": "2026-08-03T00:00:00Z",
                "subjects": 4,
                "subjects_returned_after_start": 2,
                "periods": [
                    {"period": 0, "eligible_subjects": 4, "retained_subjects": 2, "retention_rate": 0.5, "all_subjects_eligible": true},
                    {"period": 1, "eligible_subjects": 0, "retained_subjects": 0, "all_subjects_eligible": false}
                ]
            }
        ],
        "next_action": {
            "code": "extend_retention_observation_window",
            "target": "/api/telemetry/analytics/retention",
            "reason": "Repeat this server-authored reason verbatim."
        }
    })
}
