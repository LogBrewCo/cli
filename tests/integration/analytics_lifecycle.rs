//! Built-binary contract proof for bounded, identity-safe lifecycle analytics.

use crate::matchers::body_json;
use crate::{Mock, MockServer, ResponseTemplate};
use logbrew_cli::{Command, HelpTopic, HttpMethod, help, parse_command};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_request_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "lifecycle", "--help"],
        vec!["logbrew", "help", "analytics", "lifecycle"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::AnalyticsLifecycle,
                json: false,
            }
        );
    }
    let command = parse_command([
        "logbrew",
        "--json",
        "analytics",
        "lifecycle",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--event-kind",
        "interaction",
        "--event",
        "checkout_completed",
        "--interval",
        "hour",
    ])?;
    assert_eq!(
        command.http_path().as_deref(),
        Some("/api/telemetry/analytics/lifecycle")
    );
    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert!(command.wants_json());
    let body = command.request_body().ok_or("lifecycle body missing")?;
    assert_eq!(body["event"]["kind"], "interaction");
    assert_eq!(body["event"]["event_name"], "checkout_completed");
    assert_eq!(body["interval"], "hour");
    assert_eq!(body["history_period_count"], 2);

    let text = help::help_text(HelpTopic::AnalyticsLifecycle);
    assert!(text.contains("New in observed history"));
    assert!(text.contains("does not prove a lifetime-new user"));
    assert!(text.contains("Raw subject IDs are never returned"));
    assert!(text.contains("exact validated schema-version-1 response"));
    Ok(())
}

async_test!(built_binary_posts_exact_scope_and_preserves_validated_json -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    let response = lifecycle_response();
    Mock::auth(
        "POST",
        "/api/telemetry/analytics/lifecycle",
        "account-token",
    )
    .and(body_json(serde_json::json!({
        "project_id": PROJECT_ID,
        "since": "24h",
        "environment": "production",
        "event": {"kind": "interaction", "event_name": "checkout_completed"},
        "interval": "hour",
        "history_period_count": 2
    })))
    .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
    .expect(1)
    .mount(&server);

    let process = run_binary(&server, true).await?;

    super::assert_cli_success(&process);
    let actual: serde_json::Value = serde_json::from_slice(process.stdout.as_slice())?;
    assert_eq!(actual, response);
    Ok(())
});

async_test!(built_binary_human_output_explains_states_coverage_and_provisional_data -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::route("POST", "/api/telemetry/analytics/lifecycle")
        .respond_with(ResponseTemplate::new(200).set_body_json(lifecycle_response()))
        .expect(1)
        .mount(&server);

    let process = run_binary(&server, false).await?;

    super::assert_cli_success(&process);
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Product lifecycle interaction checkout_completed",
        "Subjects: 8 active in analysis; 2 history-only; 10 observed",
        "P0 2026-08-01T00:00:00Z to 2026-08-01T01:00:00Z: 6 active | 3 new in observed history | 2 returning | 1 resurrected | 2 dormant | net +2",
        "P3 2026-08-01T03:00:00Z to 2026-08-01T03:30:00Z (partial): 4 active",
        "Coverage: named 95/100; selected identity 20/30; sessionized 18/20; trace-linked 15/20; history identity 8/10",
        "Capture gap: 10 selected analysis events lacked an explicit opaque subject ID.",
        "Provisional: 3/4 lifecycle buckets are fully observed; period 3 is partial.",
        "Next: compare the same event across bounded releases, environments, or services",
    ] {
        assert!(text.contains(expected), "missing human detail: {expected}");
    }
    assert!(!text.contains("Repeat this server-authored reason verbatim."));
    Ok(())
});

async_test!(built_binary_fails_closed_on_unknown_identity_fields_without_reflection -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    let mut response = lifecycle_response();
    response["buckets"][0]["distinct_id"] = "hostile-subject-marker".into();
    Mock::route("POST", "/api/telemetry/analytics/lifecycle")
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server);

    let process = run_binary(&server, true).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    let error: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(error["error"], "analytics_lifecycle_response_invalid");
    assert!(!text.contains("hostile-subject-marker"));
    Ok(())
});

/// Runs the actual CLI process while the async loopback server remains responsive.
async fn run_binary(
    server: &MockServer,
    json: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let mut args = vec![
        "analytics",
        "lifecycle",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--event-kind",
        "interaction",
        "--event",
        "checkout_completed",
        "--environment",
        "production",
        "--interval",
        "hour",
    ];
    if json {
        args.push("--json");
    }
    super::run_cli(server, args.as_slice()).await
}

/// Stable schema-version-1 fixture matching the API contract.
fn lifecycle_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "query": {
            "project_id": PROJECT_ID,
            "since": "2026-08-01T00:00:00Z",
            "until": "2026-08-01T03:30:00Z",
            "history_since": "2026-07-31T22:00:00Z",
            "environment": "production",
            "event": {"kind": "interaction", "event_name": "checkout_completed"},
            "interval": "hour",
            "interval_seconds": 3600,
            "history_period_count": 2,
            "expected_buckets": 4
        },
        "purpose": "Classifies bounded identified-user lifecycle state.",
        "summary": {
            "observed_subjects": 10,
            "analysis_active_subjects": 8,
            "history_only_subjects": 2,
            "returned_buckets": 4,
            "fully_observed_buckets": 3,
            "latest_fully_observed_period": 2,
            "buckets_with_resurrection": 4,
            "buckets_with_dormancy": 4
        },
        "coverage": {
            "analysis_classified_events": 100,
            "analysis_named_events": 95,
            "analysis_unnamed_events": 5,
            "analysis_identified_events": 80,
            "selected_events": 30,
            "usable_selected_events": 20,
            "usable_selected_sessionized_events": 18,
            "usable_selected_trace_linked_events": 15,
            "excluded_selected_events": 10,
            "history_selected_events": 10,
            "usable_history_selected_events": 8,
            "excluded_history_selected_events": 2,
            "event_name_rate": 0.95,
            "selected_identity_coverage_rate": 0.666_666_666_666_666_6,
            "selected_sessionization_rate": 0.9,
            "selected_trace_link_rate": 0.75,
            "history_identity_coverage_rate": 0.8,
            "limitations": [
                "New in observed history does not prove lifetime-new activity.",
                "The final bucket is incomplete and provisional."
            ]
        },
        "buckets": [
            {
                "period": 0,
                "started_at": "2026-08-01T00:00:00Z",
                "ended_at": "2026-08-01T01:00:00Z",
                "fully_observed": true,
                "active_subjects": 6,
                "new_within_observed_history_subjects": 3,
                "returning_subjects": 2,
                "resurrected_subjects": 1,
                "dormant_subjects": 2,
                "previous_active_subjects": 4,
                "new_share_of_active": 0.5,
                "returning_share_of_active": 0.333_333_333_333_333_3,
                "resurrected_share_of_active": 0.166_666_666_666_666_66,
                "dormant_share_of_previous_active": 0.5,
                "net_active_change": 2
            },
            {
                "period": 1,
                "started_at": "2026-08-01T01:00:00Z",
                "ended_at": "2026-08-01T02:00:00Z",
                "fully_observed": true,
                "active_subjects": 5,
                "new_within_observed_history_subjects": 1,
                "returning_subjects": 3,
                "resurrected_subjects": 1,
                "dormant_subjects": 3,
                "previous_active_subjects": 6,
                "new_share_of_active": 0.2,
                "returning_share_of_active": 0.6,
                "resurrected_share_of_active": 0.2,
                "dormant_share_of_previous_active": 0.5,
                "net_active_change": -1
            },
            {
                "period": 2,
                "started_at": "2026-08-01T02:00:00Z",
                "ended_at": "2026-08-01T03:00:00Z",
                "fully_observed": true,
                "active_subjects": 5,
                "new_within_observed_history_subjects": 0,
                "returning_subjects": 4,
                "resurrected_subjects": 1,
                "dormant_subjects": 1,
                "previous_active_subjects": 5,
                "new_share_of_active": 0.0,
                "returning_share_of_active": 0.8,
                "resurrected_share_of_active": 0.2,
                "dormant_share_of_previous_active": 0.2,
                "net_active_change": 0
            },
            {
                "period": 3,
                "started_at": "2026-08-01T03:00:00Z",
                "ended_at": "2026-08-01T03:30:00Z",
                "fully_observed": false,
                "active_subjects": 4,
                "new_within_observed_history_subjects": 0,
                "returning_subjects": 3,
                "resurrected_subjects": 1,
                "dormant_subjects": 2,
                "previous_active_subjects": 5,
                "new_share_of_active": 0.0,
                "returning_share_of_active": 0.75,
                "resurrected_share_of_active": 0.25,
                "dormant_share_of_previous_active": 0.4,
                "net_active_change": -1
            }
        ],
        "next_action": {
            "code": "compare_lifecycle_contexts",
            "target": "/api/telemetry/analytics/lifecycle",
            "reason": "Repeat this server-authored reason verbatim."
        }
    })
}
