//! Built-binary contract proof for bounded product-analytics overview reads.

use logbrew_cli::{Command, HelpTopic, HttpMethod, help, parse_command};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_get_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "overview", "--help"],
        vec!["logbrew", "help", "analytics", "overview"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::AnalyticsOverview,
                json: false,
            }
        );
    }

    let command = overview_command(true)?;
    assert_eq!(command.http_method(), Some(HttpMethod::Get));
    assert!(command.wants_json());
    assert!(command.request_body().is_none());
    let path = command
        .http_path()
        .ok_or("analytics overview path missing")?;
    for expected in [
        "/api/telemetry/analytics/overview?",
        "project_id=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "since=24h",
        "interval=5m",
        "environment=production",
        "top_limit=2",
        "response_version=2",
    ] {
        assert!(path.contains(expected), "missing request scope: {expected}");
    }

    let text = help::help_text(HelpTopic::AnalyticsOverview);
    for expected in [
        "active identified users",
        "typed anonymous subjects",
        "subject-kind coverage",
        "exact event names",
        "Capture-quality counts disclose",
        "without user or session identifiers",
    ] {
        assert!(text.contains(expected), "missing help detail: {expected}");
    }
    let namespace = help::help_text(HelpTopic::Analytics);
    assert!(namespace.contains("logbrew analytics overview --help"));
    Ok(())
}

#[tokio::test]
async fn built_binary_gets_exact_scope_and_preserves_validated_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = overview_response();
    let response_body = serde_json::to_string(&response)?;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/analytics/overview"))
        .and(header("authorization", "Bearer account-token"))
        .and(query_param("project_id", PROJECT_ID))
        .and(query_param("since", "24h"))
        .and(query_param("interval", "5m"))
        .and(query_param("environment", "production"))
        .and(query_param("top_limit", "2"))
        .and(query_param("response_version", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(response_body.clone(), "application/json"),
        )
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
    let actual = String::from_utf8(process.stdout)?;
    assert_eq!(actual.trim_end(), response_body);
    Ok(())
}

#[tokio::test]
async fn built_binary_human_output_explains_activity_coverage_and_next_step()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = overview_response();
    response["top_actions"][0]["name"] = "checkout\u{202e}started".into();
    Mock::given(method("GET"))
        .and(path("/api/telemetry/analytics/overview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false).await?;

    assert!(process.status.success());
    assert!(process.stderr.is_empty());
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Product analytics overview",
        "Analysis readiness: identified-user and session analysis",
        "Actions: 120; active identified users: 18; active anonymous subjects: 7; sessions: 24; distinct names: 3",
        "Classified events: 110 (page views 50, screen views 20, interactions 40)",
        "Classified subjects: active identified users 17; active anonymous subjects 6; sessions 23",
        "Action capture: typed-user eligible 100/120; sessionized 90/120; trace-linked 60/120",
        "Action subject coverage (index v1): user 100; anonymous 8; legacy kind unknown 5; missing 4; historical unindexed 3",
        "Classified capture: surfaced 100/110; named 105/110; typed-user eligible 90/110; sessionized 80/110; trace-linked 95/110",
        "Classified subject coverage (index v1): user 90; anonymous 8; legacy kind unknown 5; missing 4; historical unindexed 3",
        "Top actions:",
        "checkout\\u{202e}started — 50 actions (41.7%)",
        "Top surfaces:",
        "page_view /checkout/:step — 40 events (36.4%)",
        "Exact events for paths, funnels, retention, and lifecycle:",
        "interaction signup_started — 30 events (27.3%)",
        "Identity gap: 5 actions had an opaque ID without a typed subject kind.",
        "Identity gap: 4 actions lacked usable subject context.",
        "History gap: 3 actions predate subject-kind indexing.",
        "Correlation gap: 60 actions could not link to a trace.",
        "Classification gap: 60 actions were absent from version-1 screen-view or interaction breakdowns.",
        "Limit: at least one lower-volume ranking was omitted by --top-limit.",
        "Accuracy: unique user, anonymous-subject, session, action-name, surface, and event-name counts are approximate",
        "Next: choose two exact events above and measure their ordered conversion with analytics funnel",
    ] {
        assert!(text.contains(expected), "missing human detail: {expected}");
    }
    assert!(!text.contains('\u{202e}'));
    assert!(!text.contains("Use this server-authored reason verbatim."));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_unknown_identity_fields_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = overview_response();
    response["query"]["user_id"] = "hostile-user-marker".into();
    Mock::given(method("GET"))
        .and(path("/api/telemetry/analytics/overview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, true).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    let error: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(error["error"], "analytics_overview_response_invalid");
    assert!(!text.contains("hostile-user-marker"));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_contradictory_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = overview_response();
    response["coverage"]["subject_coverage"]["legacy_unknown_kind_events"] = 6.into();
    response["next_action"]["reason"] = "contradictory-response-marker".into();
    Mock::given(method("GET"))
        .and(path("/api/telemetry/analytics/overview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    assert!(text.contains("product analytics overview response is invalid"));
    assert!(!text.contains("contradictory-response-marker"));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_impossible_anonymous_cardinality()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = overview_response();
    response["summary"]["active_anonymous_subjects"] = 9.into();
    response["next_action"]["reason"] = "impossible-cardinality-marker".into();
    Mock::given(method("GET"))
        .and(path("/api/telemetry/analytics/overview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false).await?;

    assert!(!process.status.success());
    assert!(process.stdout.is_empty());
    let text = String::from_utf8(process.stderr)?;
    assert!(text.contains("product analytics overview response is invalid"));
    assert!(!text.contains("impossible-cardinality-marker"));
    Ok(())
}

/// Parses the representative public command shape.
fn overview_command(json: bool) -> Result<Command, logbrew_cli::CliError> {
    let mut args = vec![
        "logbrew",
        "analytics",
        "overview",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--interval",
        "5m",
        "--environment",
        "production",
        "--top-limit",
        "2",
    ];
    if json {
        args.push("--json");
    }
    parse_command(args)
}

/// Runs the actual CLI process while the async loopback server remains responsive.
async fn run_binary(
    server: &MockServer,
    json: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let mut args = vec![
        "analytics",
        "overview",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--interval",
        "5m",
        "--environment",
        "production",
        "--top-limit",
        "2",
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

/// Stable schema-version-2 fixture matching the public API contract.
fn overview_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 2,
        "query": {
            "project_id": PROJECT_ID,
            "since": "2026-08-02T00:00:00Z",
            "until": "2026-08-02T00:10:00Z",
            "interval": "5m",
            "interval_seconds": 300,
            "environment": "production",
            "top_limit": 2
        },
        "purpose": "Shows bounded product activity without inferring people from anonymous events.",
        "analysis_level": "user_and_session",
        "summary": {
            "actions": 120,
            "active_identified_users": 18,
            "active_anonymous_subjects": 7,
            "sessions": 24,
            "distinct_action_names": 3,
            "actions_per_identified_user": 100.0 / 18.0,
            "actions_per_session": 3.75
        },
        "coverage": {
            "identified_actions": 100,
            "anonymous_actions": 20,
            "subject_coverage": {
                "index_version": 1,
                "identified_user_events": 100,
                "anonymous_subject_events": 8,
                "legacy_unknown_kind_events": 5,
                "missing_subject_events": 4,
                "historical_unindexed_events": 3
            },
            "sessionized_actions": 90,
            "traced_actions": 60,
            "identification_rate": 100.0 / 120.0,
            "sessionization_rate": 0.75,
            "trace_link_rate": 0.5,
            "expected_buckets": 2,
            "returned_points": 2,
            "returned_top_actions": 2,
            "top_actions_truncated": true,
            "first_seen_at": "2026-08-02T00:00:10Z",
            "last_seen_at": "2026-08-02T00:09:50Z",
            "limitations": ["Some actions lack identity, session, or trace context."]
        },
        "estimation": {
            "unique_counts_are_approximate": true,
            "method": "clickhouse_uniq_combined64",
            "description": "Unique counts use bounded approximate aggregation."
        },
        "series": [
            {
                "bucket_start": "2026-08-02T00:00:00Z",
                "bucket_end": "2026-08-02T00:05:00Z",
                "actions": 50,
                "active_identified_users": 8,
                "sessions": 10,
                "identified_actions": 45,
                "sessionized_actions": 40,
                "traced_actions": 25
            },
            {
                "bucket_start": "2026-08-02T00:05:00Z",
                "bucket_end": "2026-08-02T00:10:00Z",
                "actions": 70,
                "active_identified_users": 12,
                "sessions": 14,
                "identified_actions": 55,
                "sessionized_actions": 50,
                "traced_actions": 35
            }
        ],
        "top_actions": [
            {
                "name": "checkout_started",
                "actions": 50,
                "active_identified_users": 12,
                "sessions": 14,
                "share_of_actions": 50.0 / 120.0
            },
            {
                "name": "purchase_completed",
                "actions": 30,
                "active_identified_users": 9,
                "sessions": 10,
                "share_of_actions": 0.25
            }
        ],
        "classified_activity": {
            "summary": {
                "events": 110,
                "page_views": 50,
                "screen_views": 20,
                "interactions": 40,
                "distinct_surfaces": 4,
                "distinct_event_names": 5,
                "active_identified_users": 17,
                "active_anonymous_subjects": 6,
                "sessions": 23
            },
            "coverage": {
                "classified_actions": 60,
                "unclassified_actions": 60,
                "action_classification_rate": 0.5,
                "surfaced_events": 100,
                "named_events": 105,
                "identified_events": 90,
                "subject_coverage": {
                    "index_version": 1,
                    "identified_user_events": 90,
                    "anonymous_subject_events": 8,
                    "legacy_unknown_kind_events": 5,
                    "missing_subject_events": 4,
                    "historical_unindexed_events": 3
                },
                "sessionized_events": 80,
                "traced_events": 95,
                "surface_rate": 100.0 / 110.0,
                "event_name_rate": 105.0 / 110.0,
                "identification_rate": 90.0 / 110.0,
                "sessionization_rate": 80.0 / 110.0,
                "trace_link_rate": 95.0 / 110.0,
                "expected_buckets": 2,
                "returned_points": 2,
                "returned_top_surfaces": 2,
                "returned_top_events": 2,
                "top_surfaces_truncated": true,
                "top_events_truncated": true,
                "first_seen_at": "2026-08-02T00:00:20Z",
                "last_seen_at": "2026-08-02T00:09:40Z",
                "limitations": [
                    "Versioned classification scope.",
                    "Unclassified action coverage.",
                    "Surface coverage.",
                    "Event-name coverage.",
                    "Anonymous-subject coverage.",
                    "Legacy subject-kind coverage.",
                    "Missing subject coverage.",
                    "Historical subject-index coverage.",
                    "Session coverage.",
                    "Trace coverage."
                ]
            },
            "series": [
                {
                    "bucket_start": "2026-08-02T00:00:00Z",
                    "bucket_end": "2026-08-02T00:05:00Z",
                    "events": 45,
                    "page_views": 20,
                    "screen_views": 10,
                    "interactions": 15
                },
                {
                    "bucket_start": "2026-08-02T00:05:00Z",
                    "bucket_end": "2026-08-02T00:10:00Z",
                    "events": 65,
                    "page_views": 30,
                    "screen_views": 10,
                    "interactions": 25
                }
            ],
            "top_surfaces": [
                {
                    "kind": "page_view",
                    "surface": "/checkout/:step",
                    "events": 40,
                    "active_identified_users": 12,
                    "sessions": 14,
                    "share_of_classified_events": 40.0 / 110.0
                },
                {
                    "kind": "screen_view",
                    "surface": "Checkout",
                    "events": 25,
                    "active_identified_users": 9,
                    "sessions": 10,
                    "share_of_classified_events": 25.0 / 110.0
                }
            ],
            "top_events": [
                {
                    "kind": "page_view",
                    "event_name": "/pricing",
                    "events": 45,
                    "active_identified_users": 13,
                    "sessions": 15,
                    "share_of_classified_events": 45.0 / 110.0
                },
                {
                    "kind": "interaction",
                    "event_name": "signup_started",
                    "events": 30,
                    "active_identified_users": 10,
                    "sessions": 12,
                    "share_of_classified_events": 30.0 / 110.0
                }
            ]
        },
        "next_action": {
            "code": "build_product_funnel",
            "target": "/api/telemetry/analytics/funnel",
            "reason": "Use this server-authored reason verbatim."
        }
    })
}
