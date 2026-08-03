//! Built-binary contract proof for privacy-safe product-analytics paths.

use logbrew_cli::{Command, HelpTopic, HttpMethod, help, parse_command};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_request_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "paths", "--help"],
        vec!["logbrew", "analytics", "paths", "following", "--help"],
        vec!["logbrew", "help", "analytics", "paths"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::AnalyticsPaths,
                json: false,
            }
        );
    }
    for args in [
        vec!["logbrew", "analytics", "--help"],
        vec!["logbrew", "help", "analytics"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::Analytics,
                json: false,
            }
        );
    }
    let command = parse_command([
        "logbrew",
        "--json",
        "analytics",
        "paths",
        "before",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--anchor-kind",
        "screen-view",
        "--anchor-event",
        "Checkout",
    ])?;
    assert_eq!(
        command.http_path().as_deref(),
        Some("/api/telemetry/analytics/paths")
    );
    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert!(command.wants_json());
    let body = command.request_body().ok_or("analytics body missing")?;
    assert_eq!(body["direction"], "preceding");
    assert_eq!(body["anchor"]["kind"], "screen_view");
    assert_eq!(body["anchor"]["event_name"], "Checkout");

    let text = help::help_text(HelpTopic::AnalyticsPaths);
    assert!(text.contains("explicit opaque session boundaries"));
    assert!(text.contains("never return session or user identifiers"));
    assert!(text.contains("JSON emits the exact validated schema-version-1 response"));
    let overview = help::help_text(HelpTopic::Analytics);
    assert!(overview.contains("logbrew analytics retention --help"));
    Ok(())
}

#[tokio::test]
async fn built_binary_posts_exact_scope_and_preserves_validated_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = paths_response();
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/paths"))
        .and(header("authorization", "Bearer account-token"))
        .and(body_json(serde_json::json!({
            "project_id": PROJECT_ID,
            "since": "24h",
            "environment": "production",
            "direction": "following",
            "anchor": {"kind": "page_view", "event_name": "/pricing"},
            "depth": 4,
            "collapse_repeated": true,
            "path_limit": 10
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
async fn built_binary_human_output_explains_journey_coverage_and_next_step()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/paths"))
        .respond_with(ResponseTemplate::new(200).set_body_json(paths_response()))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false).await?;

    assert!(process.status.success());
    assert!(process.stderr.is_empty());
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Product paths following from page_view /pricing",
        "Anchored sessions: 20; represented: 12; unrepresented: 8",
        "1. 12 sessions (60.0%): [0] page_view /pricing -> [+1] interaction signup_started",
        "Coverage: named 90/100; sessionized 80/100; usable anchors 24/30",
        "Capture gap: 20 classified events lacked an explicit session ID.",
        "Limit: lower-volume or per-session-capped journeys are not represented.",
        "Next: measure the most material returned sequence as an exact funnel",
    ] {
        assert!(text.contains(expected), "missing human detail: {expected}");
    }
    assert!(!text.contains("Measure this server-authored reason verbatim."));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_unknown_identity_fields_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = paths_response();
    response["query"]["session_id"] = "hostile-session-marker".into();
    Mock::given(method("POST"))
        .and(path("/api/telemetry/analytics/paths"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, true).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    let error: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(error["error"], "analytics_response_invalid");
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
        "paths",
        "following",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--anchor-kind",
        "page-view",
        "--anchor-event",
        "/pricing",
        "--environment",
        "production",
    ];
    if json {
        args.push("--json");
    }
    let base_url = server.uri();
    let process = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_logbrew"))
            .env_clear()
            .env("HOME", std::env::temp_dir())
            .env("LOGBREW_API_URL", base_url)
            .env("LOGBREW_TOKEN", "account-token")
            .args(args)
            .output()
    })
    .await??;
    Ok(process)
}

/// Stable schema-version-1 fixture matching the private API contract.
fn paths_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "query": {
            "project_id": PROJECT_ID,
            "since": "2026-08-02T00:00:00Z",
            "until": "2026-08-03T00:00:00Z",
            "environment": "production",
            "direction": "following",
            "anchor": {"kind": "page_view", "event_name": "/pricing"},
            "depth": 4,
            "collapse_repeated": true,
            "path_limit": 10
        },
        "purpose": "Shows aggregate paths around one exact event.",
        "summary": {
            "anchored_sessions": 20,
            "represented_sessions": 12,
            "unrepresented_sessions": 8,
            "returned_paths": 1,
            "paths_truncated": true
        },
        "coverage": {
            "classified_events": 100,
            "named_events": 90,
            "unnamed_events": 10,
            "sessionized_events": 80,
            "unsessionized_events": 20,
            "anchor_events": 30,
            "usable_anchor_events": 24,
            "excluded_anchor_events": 6,
            "event_name_rate": 0.9,
            "sessionization_rate": 0.8,
            "anchor_session_coverage_rate": 0.8,
            "ordered_event_cap_per_session": 1024,
            "limitations": ["Only classified events are included."]
        },
        "paths": [{
            "rank": 1,
            "sessions": 12,
            "share_of_anchored_sessions": 0.6,
            "nodes": [
                {"relative_position": 0, "kind": "page_view", "event_name": "/pricing"},
                {"relative_position": 1, "kind": "interaction", "event_name": "signup_started"}
            ]
        }],
        "next_action": {
            "code": "measure_top_path_as_funnel",
            "target": "/api/telemetry/analytics/funnel",
            "reason": "Measure this server-authored reason verbatim."
        }
    })
}
