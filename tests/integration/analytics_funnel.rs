//! Built-binary contract proof for bounded, identity-safe funnel analytics.

use logbrew_cli::{Command, HelpTopic, HttpMethod, help, parse_command};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_request_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "funnel", "--help"],
        vec!["logbrew", "help", "analytics", "funnels"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::AnalyticsFunnel,
                json: false,
            }
        );
    }
    let command = parse_command([
        "logbrew",
        "--json",
        "analytics",
        "funnel",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--step",
        "page-view",
        "/pricing",
        "--step",
        "interaction",
        "signup_completed",
        "--conversion-window",
        "1h",
    ])?;
    assert_eq!(
        command.http_path().as_deref(),
        Some("/api/telemetry/analytics/funnel")
    );
    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert!(command.wants_json());
    let body = command.request_body().ok_or("funnel body missing")?;
    assert_eq!(body["analysis_unit"], "session");
    assert_eq!(body["conversion_window_seconds"], 3_600);
    assert_eq!(body["steps"][0]["kind"], "page_view");
    assert_eq!(body["steps"][1]["event_name"], "signup_completed");

    let text = help::help_text(HelpTopic::AnalyticsFunnel);
    assert!(text.contains("two through eight exact classified events"));
    assert!(text.contains("strictly greater timestamp"));
    assert!(text.contains("Raw IDs are never returned"));
    assert!(text.contains("exact validated schema-version-1 response"));
    Ok(())
}

#[tokio::test]
async fn built_binary_posts_exact_scope_and_preserves_validated_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = funnel_response();
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/funnel"))
        .and(header("authorization", "Bearer account-token"))
        .and(body_json(serde_json::json!({
            "project_id": PROJECT_ID,
            "since": "24h",
            "environment": "production",
            "analysis_unit": "session",
            "conversion_window_seconds": 3600,
            "steps": [
                {"kind": "page_view", "event_name": "/pricing"},
                {"kind": "interaction", "event_name": "signup_started"},
                {"kind": "interaction", "event_name": "signup_completed"}
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, true).await?;

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
async fn built_binary_human_output_explains_conversion_drop_off_coverage_and_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/funnel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(funnel_response()))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false).await?;

    assert!(process.status.success());
    assert!(process.stderr.is_empty());
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Product funnel session",
        "50 entered of 60 candidates (83.3%); 15 completed (30.0% of entered)",
        "1. page_view /pricing: 50 units | entry | 20 drop before next (40.0%)",
        "2. interaction signup_started: 30 units | 60.0% from previous | 60.0% from first | 15 drop before next (50.0%)",
        "3. interaction signup_completed: 15 units | 50.0% from previous | 30.0% from first",
        "Coverage: named 180/200; unit-identified 160/200; selected usable 100/120",
        "Capture gap: 20 matching events lacked an explicit session ID and were excluded.",
        "counts are visits or app sessions, not people",
        "Next: compare the same exact funnel across bounded releases, environments, or services",
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
    let mut response = funnel_response();
    response["steps"][0]["session_id"] = "hostile-session-marker".into();
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/funnel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, true).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    let error: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(error["error"], "analytics_funnel_response_invalid");
    assert!(!text.contains("hostile-session-marker"));
    Ok(())
}

/// Runs the actual CLI process while the async loopback server remains responsive.
async fn run_binary(
    server: &MockServer,
    json: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let mut args = vec![
        "analytics",
        "funnel",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--step",
        "page-view",
        "/pricing",
        "--step",
        "interaction",
        "signup_started",
        "--step",
        "interaction",
        "signup_completed",
        "--environment",
        "production",
        "--conversion-window",
        "1h",
    ];
    if json {
        args.push("--json");
    }
    super::run_cli(server, args).await
}

/// Stable schema-version-1 fixture matching the API contract.
fn funnel_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "query": {
            "project_id": PROJECT_ID,
            "since": "2026-08-01T00:00:00Z",
            "until": "2026-08-02T00:00:00Z",
            "environment": "production",
            "analysis_unit": "session",
            "conversion_window_seconds": 3600
        },
        "purpose": "Measures exact ordered conversion without returning raw identity.",
        "summary": {
            "candidate_units": 60,
            "entered_units": 50,
            "completed_units": 15,
            "entry_rate": 0.833_333_333_333_333_4,
            "overall_conversion_rate": 0.3
        },
        "coverage": {
            "classified_events": 200,
            "named_events": 180,
            "unnamed_events": 20,
            "unit_identified_events": 160,
            "selected_step_events": 120,
            "usable_selected_step_events": 100,
            "excluded_selected_step_events": 20,
            "event_name_rate": 0.9,
            "selected_unit_coverage_rate": 0.833_333_333_333_333_4,
            "limitations": [
                "Only exact named classified events participate.",
                "Steps require strictly increasing timestamps."
            ]
        },
        "steps": [
            {
                "position": 1,
                "kind": "page_view",
                "event_name": "/pricing",
                "units": 50,
                "conversion_from_first": 1.0,
                "drop_off_to_next_units": 20,
                "drop_off_to_next_rate": 0.4
            },
            {
                "position": 2,
                "kind": "interaction",
                "event_name": "signup_started",
                "units": 30,
                "conversion_from_previous": 0.6,
                "conversion_from_first": 0.6,
                "drop_off_to_next_units": 15,
                "drop_off_to_next_rate": 0.5
            },
            {
                "position": 3,
                "kind": "interaction",
                "event_name": "signup_completed",
                "units": 15,
                "conversion_from_previous": 0.5,
                "conversion_from_first": 0.3
            }
        ],
        "next_action": {
            "code": "compare_funnel_contexts",
            "target": "/api/telemetry/analytics/funnel",
            "reason": "Repeat this server-authored reason verbatim."
        }
    })
}
