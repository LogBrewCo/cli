//! Built-binary contract proof for privacy-safe product-analytics property discovery.

use crate::matchers::query_param;
use crate::{Mock, MockServer, ResponseTemplate};
use logbrew_cli::{HelpTopic, HttpMethod, help, parse_command};

const PROJECT_ID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

#[test]
fn public_grammar_help_and_get_model_stay_aligned() -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["logbrew", "analytics", "properties", "--help"],
        vec!["logbrew", "help", "analytics", "property"],
        vec!["logbrew", "analytics", "dimensions", "--help"],
    ] {
        crate::assert_help(&args, HelpTopic::AnalyticsProperties, false);
    }

    let command = parse_command(std::iter::once("logbrew").chain(properties_args(true)))?;
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
        "top 5 values",
        "tag.utm_source",
        "--segment-property",
    ] {
        assert!(text.contains(expected), "missing help detail: {expected}");
    }
    assert!(help::help_text(HelpTopic::Analytics).contains("logbrew analytics properties --help"));
    Ok(())
}

async_test!(built_binary_gets_exact_scope_and_preserves_validated_json -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await?;
    let response = property_response();
    let response_body = serde_json::to_string(&response)?;
    Mock::auth(
        "GET",
        "/api/telemetry/analytics/properties",
        "account-token",
    )
    .and(query_param("project_id", PROJECT_ID))
    .and(query_param("since", "24h"))
    .and(query_param("environment", "production"))
    .and(query_param("limit", "2"))
    .and(query_param("response_version", "3"))
    .respond_with(
        ResponseTemplate::new(200).set_body_raw(response_body.clone(), "application/json"),
    )
    .expect(1)
    .mount(&server);

    let process = super::run_cli(&server, &properties_args(true)).await?;

    super::assert_cli_success(&process);
    assert_eq!(String::from_utf8(process.stdout)?.trim_end(), response_body);
    Ok(())
});

async_test!(built_binary_human_output_separates_capture_privacy_and_truncation -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/api/telemetry/analytics/properties")
        .respond_with(ResponseTemplate::new(200).set_body_json(&property_response()))
        .expect(1)
        .mount(&server);

    let process = super::run_cli(&server, &properties_args(false)).await?;

    super::assert_cli_success(&process);
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
});

async_test!(built_binary_fails_closed_on_values_sensitive_keys_and_contradictory_counts -> Result<(), Box<dyn std::error::Error>>, {
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
        let server = MockServer::start().await?;
        let mut response = property_response();
        mutate(&mut response);
        Mock::route("GET", "/api/telemetry/analytics/properties")
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .expect(1)
            .mount(&server);

        let process = super::run_cli(&server, &properties_args(true)).await?;

        let text = super::cli_json_error(process, "analytics_properties_response_invalid")?;
        assert!(!text.contains("secret-marker"));
    }
    Ok(())
});

/// Shared arguments for parser and built-binary coverage.
fn properties_args(json: bool) -> Vec<&'static str> {
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
    args
}

/// Stable schema-version-3 fixture matching the API contract.
fn property_response() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 3,
        "detail": super::analytics_detail(
            "session",
            "2026-08-02T00:00:00Z",
            "2026-08-03T00:00:00Z",
            ("property:tag.plan=pro", "tag.plan=pro", 5, 10),
        ),
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
            "method": "approximate_distinct",
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
