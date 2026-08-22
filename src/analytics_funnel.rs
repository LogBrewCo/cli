//! Versioned, bounded product-analytics funnel reporting.

#![expect(
    clippy::missing_docs_in_private_items,
    reason = "response fields mirror the exact public analytics contract"
)]

use serde::Deserialize;

use crate::analytics_contract::{bounded_counts, ratio_matches};
use crate::analytics_request::{self, Kind, insert_optional};
use crate::http::{nonempty_control_safe as bounded_contract_text, terminal_safe as display_text};
use crate::time::{parse_utc_timestamp, timestamp_nanos};
use crate::{
    AnalyticsFunnelEventKind, AnalyticsFunnelOptions, AnalyticsFunnelUnit, CliEnvironment,
    RuntimeError,
};

/// Public response version implemented by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Maximum ordered funnel steps returned by the public API.
const STEP_LIMIT: usize = 8;
/// Maximum material limitations accepted from the bounded API.
const LIMITATION_LIMIT: usize = 16;
/// Maximum backend-default conversion window.
const DEFAULT_CONVERSION_WINDOW_SECONDS: u32 = 24 * 60 * 60;
/// Maximum selected analytics range.
const MAX_RANGE_SECONDS: i128 = 31 * 24 * 60 * 60;
/// Nanoseconds in one second.
const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Builds the exact public POST body with explicit CLI unit semantics.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_body(options: &AnalyticsFunnelOptions) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    drop(body.insert(
        "project_id".to_owned(),
        serde_json::Value::String(options.project_id.clone()),
    ));
    drop(body.insert(
        "since".to_owned(),
        serde_json::Value::String(options.since.clone()),
    ));
    insert_optional(&mut body, "until", options.until.as_deref());
    insert_optional(&mut body, "service_name", options.service_name.as_deref());
    insert_optional(&mut body, "release", options.release.as_deref());
    insert_optional(&mut body, "environment", options.environment.as_deref());
    drop(body.insert(
        "analysis_unit".to_owned(),
        serde_json::Value::String(options.analysis_unit.as_str().to_owned()),
    ));
    if let Some(seconds) = options.conversion_window_seconds {
        drop(body.insert("conversion_window_seconds".to_owned(), seconds.into()));
    }
    drop(
        body.insert(
            "steps".to_owned(),
            serde_json::Value::Array(
                options
                    .steps
                    .iter()
                    .map(|step| {
                        serde_json::json!({
                            "kind": step.kind.as_str(),
                            "event_name": step.event_name,
                        })
                    })
                    .collect(),
            ),
        ),
    );
    serde_json::Value::Object(body)
}

/// Executes one aggregate, identity-safe funnel request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsFunnelOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let request = request_body(options);
    let body = analytics_request::send(
        env,
        "/api/telemetry/analytics/funnel",
        Kind::Funnel,
        Some(&request),
        RESPONSE_LIMIT,
    )
    .await?;
    let response = validated_response(options, body.as_str())?;
    if json {
        writeln!(output, "{body}")?;
    } else {
        write!(output, "{}", render_response(&response))?;
    }
    Ok(())
}

/// Complete response with unknown fields rejected at every level.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunnelResponse {
    schema_version: u8,
    query: FunnelQuery,
    purpose: String,
    summary: FunnelSummary,
    coverage: FunnelCoverage,
    steps: Vec<FunnelStep>,
    next_action: NextAction,
}

/// Normalized effective query echoed by the backend.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunnelQuery {
    project_id: String,
    since: String,
    until: String,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    analysis_unit: AnalyticsFunnelUnit,
    conversion_window_seconds: u32,
}

/// Aggregate conversion outcome for the selected identity boundary.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunnelSummary {
    candidate_units: u64,
    entered_units: u64,
    completed_units: u64,
    entry_rate: Option<f64>,
    overall_conversion_rate: Option<f64>,
}

/// Capture coverage that qualifies one funnel result.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunnelCoverage {
    classified_events: u64,
    named_events: u64,
    unnamed_events: u64,
    unit_identified_events: u64,
    selected_step_events: u64,
    usable_selected_step_events: u64,
    excluded_selected_step_events: u64,
    event_name_rate: Option<f64>,
    selected_unit_coverage_rate: Option<f64>,
    limitations: Vec<String>,
}

/// One ordered funnel step and its derived conversion values.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunnelStep {
    position: u8,
    kind: AnalyticsFunnelEventKind,
    event_name: String,
    units: u64,
    conversion_from_previous: Option<f64>,
    conversion_from_first: Option<f64>,
    drop_off_to_next_units: Option<u64>,
    drop_off_to_next_rate: Option<f64>,
}

/// Stable server-selected follow-up.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NextAction {
    code: String,
    target: String,
    reason: String,
}

/// Parses and proves the complete schema-version-1 response.
fn validated_response(
    options: &AnalyticsFunnelOptions,
    body: &str,
) -> Result<FunnelResponse, RuntimeError> {
    let response =
        serde_json::from_str::<FunnelResponse>(body).map_err(|_error| Kind::Funnel.invalid())?;
    if response.schema_version != SCHEMA_VERSION
        || !bounded_contract_text(response.purpose.as_str(), 4096)
        || !valid_query(options, &response.query)
        || !valid_coverage(&response.coverage)
        || !valid_summary_and_steps(options, &response)
        || !valid_next_action(&response)
    {
        return Err(Kind::Funnel.invalid());
    }
    Ok(response)
}

/// Requires the backend echo to match every exact client-selected scope field.
fn valid_query(options: &AnalyticsFunnelOptions, query: &FunnelQuery) -> bool {
    let (Some(since), Some(until)) = (
        parse_utc_timestamp(query.since.as_str()),
        parse_utc_timestamp(query.until.as_str()),
    ) else {
        return false;
    };
    let Some(range_nanos) = timestamp_nanos(until).checked_sub(timestamp_nanos(since)) else {
        return false;
    };
    let Some(range_seconds) = range_nanos
        .checked_add(NANOS_PER_SECOND - 1)
        .and_then(|value| value.checked_div(NANOS_PER_SECOND))
        .and_then(|value| u32::try_from(value).ok())
    else {
        return false;
    };
    let expected_window = options
        .conversion_window_seconds
        .unwrap_or_else(|| DEFAULT_CONVERSION_WINDOW_SECONDS.min(range_seconds));

    query.project_id == options.project_id
        && query.service_name == options.service_name
        && query.release == options.release
        && query.environment == options.environment
        && query.analysis_unit == options.analysis_unit
        && range_nanos > 0
        && range_nanos <= MAX_RANGE_SECONDS * NANOS_PER_SECOND
        && (1..=range_seconds).contains(&query.conversion_window_seconds)
        && query.conversion_window_seconds == expected_window
}

/// Proves every derived coverage count, ratio, and limitation bound.
fn valid_coverage(coverage: &FunnelCoverage) -> bool {
    if !bounded_counts(&[
        coverage.classified_events,
        coverage.named_events,
        coverage.unnamed_events,
        coverage.unit_identified_events,
        coverage.selected_step_events,
        coverage.usable_selected_step_events,
        coverage.excluded_selected_step_events,
    ]) || coverage.named_events > coverage.classified_events
        || coverage.unit_identified_events > coverage.classified_events
        || coverage.selected_step_events > coverage.named_events
        || coverage.usable_selected_step_events > coverage.selected_step_events
        || coverage.usable_selected_step_events > coverage.unit_identified_events
        || coverage.unnamed_events != coverage.classified_events - coverage.named_events
        || coverage.excluded_selected_step_events
            != coverage.selected_step_events - coverage.usable_selected_step_events
        || coverage.limitations.is_empty()
        || coverage.limitations.len() > LIMITATION_LIMIT
        || !coverage
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 768))
    {
        return false;
    }
    ratio_matches(
        coverage.event_name_rate,
        coverage.named_events,
        coverage.classified_events,
    ) && ratio_matches(
        coverage.selected_unit_coverage_rate,
        coverage.usable_selected_step_events,
        coverage.selected_step_events,
    )
}

/// Proves summary totals and every exact ordered step.
fn valid_summary_and_steps(options: &AnalyticsFunnelOptions, response: &FunnelResponse) -> bool {
    let summary = &response.summary;
    if !bounded_counts(&[
        summary.candidate_units,
        summary.entered_units,
        summary.completed_units,
    ]) || summary.entered_units > summary.candidate_units
        || summary.completed_units > summary.entered_units
        || summary.candidate_units > response.coverage.unit_identified_events
        || summary.candidate_units > response.coverage.usable_selected_step_events
        || !ratio_matches(
            summary.entry_rate,
            summary.entered_units,
            summary.candidate_units,
        )
        || !ratio_matches(
            summary.overall_conversion_rate,
            summary.completed_units,
            summary.entered_units,
        )
        || response.steps.len() != options.steps.len()
        || !(2..=STEP_LIMIT).contains(&response.steps.len())
    {
        return false;
    }

    response
        .steps
        .iter()
        .enumerate()
        .all(|(index, step)| valid_step(options, response, index, step))
        && response.steps.first().map(|step| step.units) == Some(summary.entered_units)
        && response.steps.last().map(|step| step.units) == Some(summary.completed_units)
}

/// Proves one step echo, monotonic count, conversion, and drop-off values.
fn valid_step(
    options: &AnalyticsFunnelOptions,
    response: &FunnelResponse,
    index: usize,
    step: &FunnelStep,
) -> bool {
    let Some(requested) = options.steps.get(index) else {
        return false;
    };
    let Some(position) = index
        .checked_add(1)
        .and_then(|value| u8::try_from(value).ok())
    else {
        return false;
    };
    let previous = index
        .checked_sub(1)
        .and_then(|previous| response.steps.get(previous))
        .map(|value| value.units);
    let next = response
        .steps
        .get(index.saturating_add(1))
        .map(|value| value.units);
    let expected_drop = next.map(|next_units| step.units.saturating_sub(next_units));

    step.position == position
        && step.kind == requested.kind
        && step.event_name == requested.event_name
        && valid_event_name(step.kind, step.event_name.as_str())
        && step.units <= response.summary.candidate_units
        && previous.is_none_or(|previous_units| step.units <= previous_units)
        && previous.map_or_else(
            || step.conversion_from_previous.is_none(),
            |previous_units| {
                ratio_matches(step.conversion_from_previous, step.units, previous_units)
            },
        )
        && ratio_matches(
            step.conversion_from_first,
            step.units,
            response.summary.entered_units,
        )
        && step.drop_off_to_next_units == expected_drop
        && expected_drop.map_or_else(
            || step.drop_off_to_next_rate.is_none(),
            |drop_off| ratio_matches(step.drop_off_to_next_rate, drop_off, step.units),
        )
}

/// Applies the server's exact event-name contract to response echoes.
fn valid_event_name(kind: AnalyticsFunnelEventKind, value: &str) -> bool {
    bounded_contract_text(value, 256)
        && (kind != AnalyticsFunnelEventKind::Interaction
            || (value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
                })))
}

/// Verifies bounded next-action text and the exact state-derived code and target.
fn valid_next_action(response: &FunnelResponse) -> bool {
    if !bounded_contract_text(response.next_action.code.as_str(), 128)
        || !bounded_contract_text(response.next_action.target.as_str(), 256)
        || !bounded_contract_text(response.next_action.reason.as_str(), 768)
    {
        return false;
    }
    let expected = expected_next_action(response);
    response.next_action.code == expected.0 && response.next_action.target == expected.1
}

/// Derives the backend's stable next action from validated result state.
const fn expected_next_action(response: &FunnelResponse) -> (&'static str, &'static str) {
    let coverage = &response.coverage;
    if coverage.classified_events == 0 {
        return ("capture_product_activity", "analyticsSchemaVersion=1");
    }
    if coverage.selected_step_events == 0 {
        return (
            "choose_captured_funnel_steps",
            "/api/telemetry/analytics/overview",
        );
    }
    if coverage.usable_selected_step_events == 0 {
        return match response.query.analysis_unit {
            AnalyticsFunnelUnit::Session => ("sessionize_product_activity", "context.session.id"),
            AnalyticsFunnelUnit::IdentifiedUser => {
                ("identify_product_users", "context.subject.kind=user")
            }
        };
    }
    if response.summary.entered_units == 0 {
        return ("verify_funnel_entry", "/api/telemetry/analytics/overview");
    }
    if response.summary.completed_units == 0 {
        return ("investigate_funnel_drop_off", "/api/telemetry/traces");
    }
    ("compare_funnel_contexts", "/api/telemetry/analytics/funnel")
}

/// Renders a useful human interpretation without reflecting backend prose.
fn render_response(response: &FunnelResponse) -> String {
    let mut output = String::new();
    output.push_str("Product funnel ");
    output.push_str(response.query.analysis_unit.as_str());
    output.push('\n');
    output.push_str(
        format!(
            "Window: {} to {}; conversion window: {}s\n",
            response.query.since, response.query.until, response.query.conversion_window_seconds,
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Summary: {} entered of {} candidates ({}); {} completed ({} of entered)\n",
            response.summary.entered_units,
            response.summary.candidate_units,
            percentage(response.summary.entry_rate),
            response.summary.completed_units,
            percentage(response.summary.overall_conversion_rate),
        )
        .as_str(),
    );
    render_steps(response, &mut output);
    render_coverage(response, &mut output);
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Adds every exact ordered step with conversion and forward drop-off.
fn render_steps(response: &FunnelResponse, output: &mut String) {
    output.push_str("Steps:\n");
    for step in &response.steps {
        output.push_str(
            format!(
                "  {}. {} {}: {} units",
                step.position,
                step.kind.as_str(),
                display_text(step.event_name.as_str()),
                step.units,
            )
            .as_str(),
        );
        if step.position == 1 {
            output.push_str(" | entry");
        } else {
            output.push_str(
                format!(
                    " | {} from previous | {} from first",
                    percentage(step.conversion_from_previous),
                    percentage(step.conversion_from_first),
                )
                .as_str(),
            );
        }
        if let Some(drop_off) = step.drop_off_to_next_units {
            output.push_str(
                format!(
                    " | {drop_off} drop before next ({})",
                    percentage(step.drop_off_to_next_rate),
                )
                .as_str(),
            );
        }
        output.push('\n');
    }
}

/// Adds capture quality, identity gaps, and fixed interpretation semantics.
fn render_coverage(response: &FunnelResponse, output: &mut String) {
    let coverage = &response.coverage;
    output.push_str(
        format!(
            "Coverage: named {}/{}; unit-identified {}/{}; selected usable {}/{}\n",
            coverage.named_events,
            coverage.classified_events,
            coverage.unit_identified_events,
            coverage.classified_events,
            coverage.usable_selected_step_events,
            coverage.selected_step_events,
        )
        .as_str(),
    );
    if coverage.unnamed_events > 0 {
        output.push_str(
            format!(
                "Capture gap: {} classified events lacked an exact derived event name.\n",
                coverage.unnamed_events,
            )
            .as_str(),
        );
    }
    if coverage.excluded_selected_step_events > 0 {
        let unit = match response.query.analysis_unit {
            AnalyticsFunnelUnit::Session => "session",
            AnalyticsFunnelUnit::IdentifiedUser => "opaque subject",
        };
        output.push_str(
            format!(
                "Capture gap: {} matching events lacked an explicit {unit} ID and were excluded.\n",
                coverage.excluded_selected_step_events,
            )
            .as_str(),
        );
    }
    match response.query.analysis_unit {
        AnalyticsFunnelUnit::Session => output.push_str(
            "Interpretation: candidates matched at least one selected event; counts are visits or app sessions, not people; steps require strictly increasing timestamps; one event cannot satisfy multiple steps; raw IDs are not returned.\n",
        ),
        AnalyticsFunnelUnit::IdentifiedUser => output.push_str(
            "Interpretation: candidates matched at least one selected event; counts use stable \
             application-supplied opaque subjects explicitly typed as users and can cross \
             sessions; steps require strictly increasing timestamps; raw IDs are not returned.\n",
        ),
    }
}

/// Formats one optional ratio for compact human output.
fn percentage(value: Option<f64>) -> String {
    value.map_or_else(
        || String::from("n/a"),
        |value| format!("{:.1}%", value * 100.0),
    )
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture version-1 page views, screen views, or interactions, then retry"
        }
        "choose_captured_funnel_steps" => {
            "choose two through eight exact captured events from Product Analytics overview"
        }
        "sessionize_product_activity" => {
            "attach one opaque context.session.id to each selected product event"
        }
        "identify_product_users" => {
            "attach one stable opaque context.subject.id and set context.subject.kind=user on \
             each selected product event"
        }
        "verify_funnel_entry" => {
            "verify the first exact event and context filters in Product Analytics overview"
        }
        "investigate_funnel_drop_off" => {
            "inspect correlated traces around the earliest material funnel drop-off"
        }
        "compare_funnel_contexts" => {
            "compare the same exact funnel across bounded releases, environments, or services"
        }
        _ => "retry the bounded analytics funnel query",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalyticsFunnelStep, AnalyticsPathEventKind};

    /// Builds one stable session funnel query.
    fn options() -> AnalyticsFunnelOptions {
        AnalyticsFunnelOptions {
            project_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_owned(),
            since: "24h".to_owned(),
            until: None,
            service_name: None,
            release: None,
            environment: Some("production".to_owned()),
            analysis_unit: AnalyticsFunnelUnit::Session,
            conversion_window_seconds: Some(3_600),
            steps: vec![
                AnalyticsFunnelStep {
                    kind: AnalyticsPathEventKind::PageView,
                    event_name: "/pricing".to_owned(),
                },
                AnalyticsFunnelStep {
                    kind: AnalyticsPathEventKind::Interaction,
                    event_name: "signup_completed".to_owned(),
                },
            ],
        }
    }

    /// Returns one internally consistent response fixture.
    fn response() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "query": {
                "project_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
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
                "entry_rate": 0.8333333333333334,
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
                "selected_unit_coverage_rate": 0.8333333333333334,
                "limitations": ["Only exact named classified events participate."]
            },
            "steps": [
                {
                    "position": 1,
                    "kind": "page_view",
                    "event_name": "/pricing",
                    "units": 50,
                    "conversion_from_first": 1.0,
                    "drop_off_to_next_units": 35,
                    "drop_off_to_next_rate": 0.7
                },
                {
                    "position": 2,
                    "kind": "interaction",
                    "event_name": "signup_completed",
                    "units": 15,
                    "conversion_from_previous": 0.3,
                    "conversion_from_first": 0.3
                }
            ],
            "next_action": {
                "code": "compare_funnel_contexts",
                "target": "/api/telemetry/analytics/funnel",
                "reason": "Compare the same steps across contexts."
            }
        })
    }

    #[test]
    fn builds_exact_request_and_validates_every_contract_layer() {
        let options = options();
        let body = request_body(&options);
        assert_eq!(body["analysis_unit"], "session");
        assert_eq!(body["conversion_window_seconds"], 3_600);
        assert_eq!(body["steps"].as_array().map(Vec::len), Some(2));

        let response =
            validated_response(&options, response().to_string().as_str()).expect("valid response");
        let human = render_response(&response);
        assert!(human.contains("50 entered of 60 candidates"));
        assert!(human.contains("35 drop before next (70.0%)"));
        assert!(human.contains("raw IDs are not returned"));
    }

    #[test]
    fn rejects_contradictory_counts_rates_steps_and_next_action() {
        for mutation in ["count", "rate", "step", "next"] {
            let mut value = response();
            match mutation {
                "count" => value["summary"]["completed_units"] = 51.into(),
                "rate" => value["steps"][1]["conversion_from_previous"] = 0.9.into(),
                "step" => value["steps"][1]["event_name"] = "other".into(),
                "next" => value["next_action"]["code"] = "capture_product_activity".into(),
                _ => unreachable!(),
            }
            assert!(
                validated_response(&options(), value.to_string().as_str()).is_err(),
                "{mutation} contradiction must fail closed"
            );
        }
    }

    #[test]
    fn rejects_unknown_fields_and_noncanonical_time() {
        let mut unknown = response();
        unknown["unexpected"] = true.into();
        assert!(validated_response(&options(), unknown.to_string().as_str()).is_err());

        let mut time = response();
        time["query"]["until"] = "2026-08-02T03:00:00+03:00".into();
        assert!(validated_response(&options(), time.to_string().as_str()).is_err());
    }

    #[test]
    fn accepts_typed_user_capture_target_for_identified_user_gap() {
        let mut value = response();
        value["query"]["analysis_unit"] = "identified_user".into();
        value["coverage"]["usable_selected_step_events"] = 0.into();
        value["next_action"]["code"] = "identify_product_users".into();
        value["next_action"]["target"] = "context.subject.kind=user".into();
        let response: FunnelResponse = serde_json::from_value(value).expect("fixture deserializes");

        assert!(valid_next_action(&response));
    }
}
