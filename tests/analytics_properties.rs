//! Built-binary contract proof for privacy-safe product-analytics property discovery.

use logbrew_cli::{Command, HelpTopic, HttpMethod, help, parse_command};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_get_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "properties", "--help"],
        vec!["logbrew", "help", "analytics", "property"],
        vec!["logbrew", "analytics", "dimensions", "--help"],
    ] {
        assert_eq!(
            parse_command(args)?,
            Command::Help {
                topic: HelpTopic::AnalyticsProperties,
                json: false,
            }
        );
    }

    let command = properties_command(true)?;
    assert_eq!(command.http_method(), Some(HttpMethod::Get));
    assert!(command.wants_json());
    assert!(command.request_body().is_none());
    let path = command
        .http_path()
        .ok_or("analytics property path missing")?;
    for expected in [
        "/api/telemetry/analytics/properties?",
        "project_id=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "since=24h",
        "environment=production",
        "limit=2",
    ] {
        assert!(path.contains(expected), "missing request scope: {expected}");
    }

    let text = help::help_text(HelpTopic::AnalyticsProperties);
    for expected in [
        "non-sensitive tag.* keys",
        "Property values",
        "never returned",
        "--segment-property",
    ] {
        assert!(text.contains(expected), "missing help detail: {expected}");
    }
    assert!(help::help_text(HelpTopic::Analytics).contains("logbrew analytics properties --help"));
    Ok(())
}

#[tokio::test]
async fn built_binary_gets_exact_scope_and_preserves_validated_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = property_response();
    let response_body = serde_json::to_string(&response)?;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/analytics/properties"))
        .and(header("authorization", "Bearer account-token"))
        .and(query_param("project_id", PROJECT_ID))
        .and(query_param("since", "24h"))
        .and(query_param("environment", "production"))
        .and(query_param("limit", "2"))
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
    assert_eq!(String::from_utf8(process.stdout)?.trim_end(), response_body);
    Ok(())
}

#[tokio::test]
async fn built_binary_human_output_separates_capture_privacy_and_truncation()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/analytics/properties"))
        .respond_with(ResponseTemplate::new(200).set_body_json(property_response()))
        .expect(1)
        .mount(&server)
        .await;

    let process = run_binary(&server, false).await?;

    assert!(process.status.success());
    assert!(process.stderr.is_empty());
    let text = String::from_utf8(process.stdout)?;
    for expected in [
        "Product analytics properties",
        "Index coverage: 90/100 classified events (90.0%)",
        "Privacy and migration: 10 unindexed; 5 incomplete; 3 privacy-filtered",
        "Values and identities are not returned",
        "Properties: 2 returned; approximately 3 available (truncated)",
        "tag.plan [custom tag]: 80 events (80.0%) | approximately 3 distinct values",
        "resource.framework.name [standard context]: 50 events (50.0%)",
        "Next: narrow service, release, environment, or time scope",
    ] {
        assert!(text.contains(expected), "missing human detail: {expected}");
    }
    assert!(!text.contains("Server-authored reason marker"));
    Ok(())
}

#[tokio::test]
async fn built_binary_fails_closed_on_values_sensitive_keys_and_contradictory_counts()
-> Result<(), Box<dyn std::error::Error>> {
    let mutations: [fn(&mut serde_json::Value); 3] = [
        |response: &mut serde_json::Value| {
            response["properties"][0]["values"] = serde_json::json!(["secret-marker"]);
        },
        |response: &mut serde_json::Value| {
            response["properties"][0]["key"] = "tag.user_id".into();
        },
        |response: &mut serde_json::Value| {
            response["coverage"]["unindexed_events"] = 9.into();
        },
    ];
    for mutate in mutations {
        let server = MockServer::start().await;
        let mut response = property_response();
        mutate(&mut response);
        Mock::given(method("GET"))
            .and(path("/api/telemetry/analytics/properties"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;

        let process = run_binary(&server, true).await?;

        assert!(!process.status.success());
        assert!(process.stdout.is_empty());
        let text = String::from_utf8(process.stderr)?;
        let error: serde_json::Value = serde_json::from_str(text.as_str())?;
        assert_eq!(error["error"], "analytics_properties_response_invalid");
        assert!(!text.contains("secret-marker"));
    }
    Ok(())
}

/// Parses the representative public command shape.
fn properties_command(json: bool) -> Result<Command, logbrew_cli::CliError> {
    let mut args = vec![
        "logbrew",
        "analytics",
        "properties",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--environment",
        "production",
        "--limit",
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
        "properties",
        "--project",
        PROJECT_ID,
        "--since",
        "24h",
        "--environment",
        "production",
        "--limit",
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

/// Stable schema-version-1 fixture matching the private API contract.
fn property_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "query": {
            "project_id": PROJECT_ID,
            "since": "2026-08-03T00:00:00Z",
            "until": "2026-08-04T00:00:00Z",
            "environment": "production",
            "limit": 2
        },
        "purpose": "Discovers aggregate privacy-safe property keys without values.",
        "summary": {
            "classified_events": 100,
            "available_property_keys": 3,
            "returned_properties": 2
        },
        "coverage": {
            "indexed_events": 90,
            "unindexed_events": 10,
            "complete_index_events": 85,
            "incomplete_index_events": 5,
            "privacy_filtered_events": 3,
            "events_with_properties": 80,
            "index_coverage_rate": 0.9,
            "property_capture_rate": 0.8,
            "properties_truncated": true,
            "values_returned": false,
            "limitations": [
                "Keys and aggregate counts only.",
                "Exact filters are case-sensitive.",
                "The response is truncated."
            ]
        },
        "estimation": {
            "count_accuracy": "approximate",
            "method": "clickhouse_uniq_combined64",
            "description": "Distinct key and value counts use bounded estimates."
        },
        "properties": [
            {
                "key": "tag.plan",
                "source": "custom_tag",
                "value_type": "string",
                "events": 80,
                "coverage_rate": 0.8,
                "distinct_values": 3
            },
            {
                "key": "resource.framework.name",
                "source": "standard_context",
                "value_type": "string",
                "events": 50,
                "coverage_rate": 0.5,
                "distinct_values": 2
            }
        ],
        "next_action": {
            "code": "narrow_property_scope",
            "target": "/api/telemetry/analytics/properties",
            "reason": "Server-authored reason marker"
        }
    })
}
