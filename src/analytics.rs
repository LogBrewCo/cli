//! Versioned, bounded product-analytics path exploration.

#![expect(
    clippy::missing_docs_in_private_items,
    reason = "response fields mirror the exact public analytics contract"
)]

use serde::Deserialize;

use crate::analytics_contract::{COUNT_LIMIT, NextAction, bounded_counts, ratio_matches};
use crate::analytics_request::{self, Kind, scoped_body, valid_event_name};
use crate::http::{nonempty_control_safe as bounded_contract_text, terminal_safe as display_text};
use crate::{
    AnalyticsPathDirection, AnalyticsPathEventKind, AnalyticsPathOptions,
    AnalyticsPathPropertyFilter, CliEnvironment, RuntimeError,
};

/// Public version implemented by this CLI.
const SCHEMA_VERSION: u8 = 1;
/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Storage contract's direction-nearest event cap.
const ORDERED_EVENT_CAP: u16 = 1024;
/// Maximum material limitations accepted from the bounded API.
const LIMITATION_LIMIT: usize = 10;
/// Maximum trace exemplars accepted per aggregate path.
const TRACE_EXEMPLAR_LIMIT: usize = 3;

/// Builds the exact public POST body with explicit CLI defaults.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command model consumes this private-module helper"
)]
pub(super) fn request_body(options: &AnalyticsPathOptions) -> serde_json::Value {
    let mut body = scoped_body(
        &options.project_id,
        &options.since,
        options.until.as_deref(),
        options.service_name.as_deref(),
        options.release.as_deref(),
        options.environment.as_deref(),
    );
    drop(body.insert(
        "direction".to_owned(),
        serde_json::Value::String(options.direction.as_str().to_owned()),
    ));
    drop(body.insert(
        "anchor".to_owned(),
        serde_json::json!({
            "kind": options.anchor_kind.as_str(),
            "event_name": options.anchor_event,
        }),
    ));
    if !options.property_filters.is_empty() {
        drop(
            body.insert(
                "property_filters".to_owned(),
                serde_json::Value::Array(
                    options
                        .property_filters
                        .iter()
                        .map(|filter| serde_json::json!({"key": filter.key, "value": filter.value}))
                        .collect(),
                ),
            ),
        );
    }
    drop(body.insert("depth".to_owned(), options.depth.into()));
    drop(body.insert(
        "collapse_repeated".to_owned(),
        options.collapse_repeated.into(),
    ));
    drop(body.insert("path_limit".to_owned(), options.path_limit.into()));
    serde_json::Value::Object(body)
}

/// Executes one aggregate, identity-safe product-path request.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    options: &AnalyticsPathOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let request = request_body(options);
    let body = analytics_request::send(
        env,
        "/api/telemetry/analytics/paths",
        Kind::Paths,
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathsResponse {
    schema_version: u8,
    query: PathQuery,
    purpose: String,
    summary: PathSummary,
    coverage: PathCoverage,
    paths: Vec<AggregatePath>,
    next_action: NextAction,
}

/// Normalized effective query echoed by the backend.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathQuery {
    project_id: String,
    since: String,
    until: String,
    service_name: Option<String>,
    release: Option<String>,
    environment: Option<String>,
    direction: AnalyticsPathDirection,
    anchor: PathAnchor,
    #[serde(default)]
    property_filters: Vec<PropertyFilter>,
    depth: u8,
    collapse_repeated: bool,
    path_limit: u8,
}

/// Exact classified event anchoring each returned sequence.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathAnchor {
    kind: AnalyticsPathEventKind,
    event_name: String,
}

/// Exact privacy-safe property predicate echoed by the backend.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PropertyFilter {
    key: String,
    value: String,
}

/// Headline aggregate coverage.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathSummary {
    anchored_sessions: u64,
    represented_sessions: u64,
    unrepresented_sessions: u64,
    returned_paths: u8,
    paths_truncated: bool,
}

/// Capture and query coverage qualifying the result.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathCoverage {
    classified_events: u64,
    named_events: u64,
    unnamed_events: u64,
    sessionized_events: u64,
    unsessionized_events: u64,
    anchor_events: u64,
    anchor_property_filters: Option<PathPropertyCoverage>,
    usable_anchor_events: u64,
    excluded_anchor_events: u64,
    traced_anchor_events: u64,
    event_name_rate: Option<f64>,
    sessionization_rate: Option<f64>,
    anchor_session_coverage_rate: Option<f64>,
    anchor_trace_link_rate: Option<f64>,
    ordered_event_cap_per_session: u16,
    limitations: Vec<String>,
}

/// Exact anchor-property classification coverage.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathPropertyCoverage {
    context_events: u64,
    property_ready_events: u64,
    missing_property_events: u64,
    matching_events: u64,
    nonmatching_value_events: u64,
    property_ready_rate: Option<f64>,
    match_rate: Option<f64>,
}

/// One highest-volume exact aggregate sequence.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AggregatePath {
    rank: u8,
    sessions: u64,
    share_of_anchored_sessions: f64,
    traced_sessions: u64,
    trace_link_rate: Option<f64>,
    trace_exemplars: Vec<String>,
    nodes: Vec<PathNode>,
}

/// One named event positioned relative to the anchor.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathNode {
    relative_position: i8,
    kind: AnalyticsPathEventKind,
    event_name: String,
}

/// Parses and proves the complete schema-version-1 response.
fn validated_response(
    options: &AnalyticsPathOptions,
    body: &str,
) -> Result<PathsResponse, RuntimeError> {
    let response =
        serde_json::from_str::<PathsResponse>(body).map_err(|_error| Kind::Paths.invalid())?;
    if !valid_query(options, &response.query)
        || response.schema_version != SCHEMA_VERSION
        || !bounded_contract_text(response.purpose.as_str(), 2048)
        || !valid_summary(&response)
        || !valid_coverage(
            &response.coverage,
            !response.query.property_filters.is_empty(),
        )
        || response.summary.anchored_sessions > response.coverage.usable_anchor_events
        || !valid_paths(options, &response)
        || !valid_next_action(&response)
    {
        return Err(Kind::Paths.invalid());
    }
    Ok(response)
}

/// Requires the backend echo to match every exact client-selected scope field.
fn valid_query(options: &AnalyticsPathOptions, query: &PathQuery) -> bool {
    query.project_id == options.project_id
        && bounded_timestamp(query.since.as_str())
        && bounded_timestamp(query.until.as_str())
        && query.service_name == options.service_name
        && query.release == options.release
        && query.environment == options.environment
        && query.direction == options.direction
        && query.anchor.kind == options.anchor_kind
        && query.anchor.event_name == options.anchor_event
        && property_filters_match(
            query.property_filters.as_slice(),
            options.property_filters.as_slice(),
        )
        && query.depth == options.depth
        && query.collapse_repeated == options.collapse_repeated
        && query.path_limit == options.path_limit
}

/// Requires the normalized property echo to match every exact client predicate.
fn property_filters_match(
    actual: &[PropertyFilter],
    expected: &[AnalyticsPathPropertyFilter],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.key == expected.key && actual.value == expected.value)
}

/// Validates the UTC RFC 3339 shape emitted by the versioned API.
fn bounded_timestamp(value: &str) -> bool {
    crate::time::parse_rfc3339(value).is_some()
}

/// Proves headline counts against the returned path aggregates.
fn valid_summary(response: &PathsResponse) -> bool {
    let summary = &response.summary;
    if !bounded_counts(&[
        summary.anchored_sessions,
        summary.represented_sessions,
        summary.unrepresented_sessions,
    ]) || usize::from(summary.returned_paths) != response.paths.len()
        || summary.returned_paths > response.query.path_limit
    {
        return false;
    }
    summary
        .represented_sessions
        .checked_add(summary.unrepresented_sessions)
        .is_some_and(|total| total == summary.anchored_sessions)
        && summary.paths_truncated == (summary.unrepresented_sessions > 0)
}

/// Proves every derived coverage count and ratio.
fn valid_coverage(coverage: &PathCoverage, has_property_filters: bool) -> bool {
    if !bounded_counts(&[
        coverage.classified_events,
        coverage.named_events,
        coverage.unnamed_events,
        coverage.sessionized_events,
        coverage.unsessionized_events,
        coverage.anchor_events,
        coverage.usable_anchor_events,
        coverage.excluded_anchor_events,
        coverage.traced_anchor_events,
    ]) || coverage.named_events > coverage.classified_events
        || coverage.sessionized_events > coverage.classified_events
        || coverage.anchor_events > coverage.named_events
        || coverage.usable_anchor_events > coverage.anchor_events
        || coverage.usable_anchor_events > coverage.sessionized_events
        || coverage.traced_anchor_events > coverage.anchor_events
        || coverage.unnamed_events != coverage.classified_events - coverage.named_events
        || coverage.unsessionized_events != coverage.classified_events - coverage.sessionized_events
        || coverage.excluded_anchor_events != coverage.anchor_events - coverage.usable_anchor_events
        || coverage.ordered_event_cap_per_session != ORDERED_EVENT_CAP
        || coverage.limitations.is_empty()
        || coverage.limitations.len() > LIMITATION_LIMIT
        || !coverage
            .limitations
            .iter()
            .all(|value| bounded_contract_text(value, 512))
    {
        return false;
    }
    ratio_matches(
        coverage.event_name_rate,
        coverage.named_events,
        coverage.classified_events,
    ) && ratio_matches(
        coverage.sessionization_rate,
        coverage.sessionized_events,
        coverage.classified_events,
    ) && ratio_matches(
        coverage.anchor_session_coverage_rate,
        coverage.usable_anchor_events,
        coverage.anchor_events,
    ) && ratio_matches(
        coverage.anchor_trace_link_rate,
        coverage.traced_anchor_events,
        coverage.anchor_events,
    ) && valid_property_coverage(coverage, has_property_filters)
}

/// Proves exact property-ready, missing-key, matching, and value-mismatch populations.
fn valid_property_coverage(coverage: &PathCoverage, has_property_filters: bool) -> bool {
    let Some(property) = coverage.anchor_property_filters.as_ref() else {
        return !has_property_filters;
    };
    if property.context_events > coverage.named_events {
        return false;
    }
    if !has_property_filters
        || !bounded_counts(&[
            property.context_events,
            property.property_ready_events,
            property.missing_property_events,
            property.matching_events,
            property.nonmatching_value_events,
        ])
        || property.property_ready_events > property.context_events
        || property.matching_events > property.property_ready_events
        || property.matching_events != coverage.anchor_events
        || property.missing_property_events
            != property.context_events - property.property_ready_events
        || property.nonmatching_value_events
            != property.property_ready_events - property.matching_events
    {
        return false;
    }
    ratio_matches(
        property.property_ready_rate,
        property.property_ready_events,
        property.context_events,
    ) && ratio_matches(
        property.match_rate,
        property.matching_events,
        property.property_ready_events,
    )
}

/// Proves ordering, anchors, shares, ranking, and aggregate totals.
fn valid_paths(options: &AnalyticsPathOptions, response: &PathsResponse) -> bool {
    let mut represented = 0_u64;
    let mut previous_sessions = None;
    let mut signatures = std::collections::HashSet::new();
    for (index, path) in response.paths.iter().enumerate() {
        if path.rank != u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX)
            || path.sessions == 0
            || path.sessions > COUNT_LIMIT
            || path.traced_sessions > path.sessions
            || path.nodes.is_empty()
            || path.nodes.len() > usize::from(options.depth.saturating_add(1))
            || !ratio_matches(
                Some(path.share_of_anchored_sessions),
                path.sessions,
                response.summary.anchored_sessions,
            )
            || !ratio_matches(path.trace_link_rate, path.traced_sessions, path.sessions)
            || !valid_trace_exemplars(path)
            || previous_sessions.is_some_and(|previous| path.sessions > previous)
            || !valid_nodes(options, path.nodes.as_slice())
        {
            return false;
        }
        previous_sessions = Some(path.sessions);
        let signature = path
            .nodes
            .iter()
            .map(|node| format!("{}\u{1f}{}", node.kind.as_str(), node.event_name))
            .collect::<Vec<_>>()
            .join("\u{1e}");
        if !signatures.insert(signature) {
            return false;
        }
        let Some(total) = represented.checked_add(path.sessions) else {
            return false;
        };
        represented = total;
    }
    represented == response.summary.represented_sessions
}

/// Requires bounded, canonical trace evidence consistent with each path count.
fn valid_trace_exemplars(path: &AggregatePath) -> bool {
    path.trace_exemplars.len() <= TRACE_EXEMPLAR_LIMIT
        && u64::try_from(path.trace_exemplars.len())
            .is_ok_and(|count| count <= path.traced_sessions)
        && (path.traced_sessions == 0) == path.trace_exemplars.is_empty()
        && path
            .trace_exemplars
            .iter()
            .all(|trace_id| trace_id.len() == 32 && crate::ids::is_trace_id(trace_id.as_str()))
        && path
            .trace_exemplars
            .windows(2)
            .all(|pair| pair[0] < pair[1])
}

/// Proves one chronological path's relative positions and exact anchor.
fn valid_nodes(options: &AnalyticsPathOptions, nodes: &[PathNode]) -> bool {
    let last = i8::try_from(nodes.len().saturating_sub(1)).unwrap_or(i8::MAX);
    for (index, node) in nodes.iter().enumerate() {
        let index = i8::try_from(index).unwrap_or(i8::MAX);
        let expected = match options.direction {
            AnalyticsPathDirection::Following => index,
            AnalyticsPathDirection::Preceding => index.saturating_sub(last),
        };
        if node.relative_position != expected
            || !valid_event_name(
                node.kind == AnalyticsPathEventKind::Interaction,
                node.event_name.as_str(),
            )
        {
            return false;
        }
    }
    if options.collapse_repeated
        && nodes
            .windows(2)
            .any(|pair| pair[0].kind == pair[1].kind && pair[0].event_name == pair[1].event_name)
    {
        return false;
    }
    let anchor = match options.direction {
        AnalyticsPathDirection::Following => nodes.first(),
        AnalyticsPathDirection::Preceding => nodes.last(),
    };
    anchor.is_some_and(|anchor| {
        anchor.relative_position == 0
            && anchor.kind == options.anchor_kind
            && anchor.event_name == options.anchor_event
    })
}

/// Requires the stable action code and target implied by the response state.
fn valid_next_action(response: &PathsResponse) -> bool {
    let has_property_filters = !response.query.property_filters.is_empty();
    let property = response.coverage.anchor_property_filters.as_ref();
    if has_property_filters != property.is_some() {
        return false;
    }
    let context_anchor_events = property.map_or(response.coverage.anchor_events, |value| {
        value.context_events
    });
    let property_ready_events = property.map_or(response.coverage.anchor_events, |value| {
        value.property_ready_events
    });
    let incomplete_property_coverage =
        property.is_some_and(|value| value.property_ready_events * 100 < value.context_events * 80);
    let has_trace_exemplar = response
        .paths
        .iter()
        .any(|path| !path.trace_exemplars.is_empty());
    let expected = if response.coverage.classified_events == 0 {
        ("capture_product_activity", "analyticsSchemaVersion=1")
    } else if context_anchor_events == 0 {
        (
            "choose_captured_path_anchor",
            "/api/telemetry/analytics/overview",
        )
    } else if has_property_filters && property_ready_events == 0 {
        (
            "capture_anchor_properties",
            "context.resource or context.tags",
        )
    } else if has_property_filters && incomplete_property_coverage {
        (
            "improve_anchor_property_coverage",
            "/api/telemetry/analytics/properties",
        )
    } else if response.coverage.anchor_events == 0 {
        (
            "verify_anchor_property_values",
            "/api/telemetry/analytics/paths",
        )
    } else if response.coverage.usable_anchor_events == 0 {
        ("sessionize_product_activity", "context.session.id")
    } else if response.summary.returned_paths == 0 {
        (
            "narrow_or_move_path_anchor",
            "/api/telemetry/analytics/paths",
        )
    } else if has_trace_exemplar {
        (
            "inspect_path_trace",
            "/api/telemetry/traces/{trace_id}/investigation",
        )
    } else if response.summary.paths_truncated {
        (
            "measure_top_path_as_funnel",
            "/api/telemetry/analytics/funnel",
        )
    } else {
        ("compare_path_contexts", "/api/telemetry/analytics/paths")
    };
    response.next_action.matches(expected.0, expected.1, 512)
}

/// Renders the useful human interpretation without reflecting backend prose.
fn render_response(response: &PathsResponse) -> String {
    let mut output = String::new();
    render_path_query(response, &mut output);
    render_paths(response, &mut output);
    render_path_coverage(response, &mut output);
    if response.summary.paths_truncated {
        output
            .push_str("Limit: lower-volume or per-session-capped journeys are not represented.\n");
    }
    output.push_str("Next: ");
    output.push_str(next_step(response.next_action.code.as_str()));
    output.push('\n');
    output
}

/// Renders the effective anchor, safe predicate keys, window, and headline counts.
fn render_path_query(response: &PathsResponse, output: &mut String) {
    output.push_str("Product paths ");
    output.push_str(response.query.direction.as_str());
    output.push_str(" from ");
    output.push_str(response.query.anchor.kind.as_str());
    output.push(' ');
    output.push_str(display_text(response.query.anchor.event_name.as_str()).as_str());
    output.push('\n');
    if !response.query.property_filters.is_empty() {
        let keys = response
            .query
            .property_filters
            .iter()
            .map(|filter| display_text(filter.key.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str(
            format!(
                "Anchor properties: {} exact AND predicate{} on {keys}; values are not repeated \
                 in human output.\n",
                response.query.property_filters.len(),
                if response.query.property_filters.len() == 1 {
                    ""
                } else {
                    "s"
                },
            )
            .as_str(),
        );
    }
    output.push_str(
        format!(
            "Window: {} to {}\n",
            response.query.since, response.query.until
        )
        .as_str(),
    );
    output.push_str(
        format!(
            "Anchored sessions: {}; represented: {}; unrepresented: {}\n",
            response.summary.anchored_sessions,
            response.summary.represented_sessions,
            response.summary.unrepresented_sessions,
        )
        .as_str(),
    );
}

/// Renders bounded aggregate journeys and the first three available evidence actions.
fn render_paths(response: &PathsResponse, output: &mut String) {
    if response.paths.is_empty() {
        output.push_str("No usable aggregate path was returned.\n");
    } else {
        for path in &response.paths {
            let nodes = path
                .nodes
                .iter()
                .map(|node| {
                    let position = if node.relative_position > 0 {
                        format!("+{}", node.relative_position)
                    } else {
                        node.relative_position.to_string()
                    };
                    format!(
                        "[{position}] {} {}",
                        node.kind.as_str(),
                        display_text(node.event_name.as_str())
                    )
                })
                .collect::<Vec<_>>()
                .join(" -> ");
            output.push_str(
                format!(
                    "{}. {} sessions ({:.1}%): {nodes}; trace-linked {}/{}\n",
                    path.rank,
                    path.sessions,
                    path.share_of_anchored_sessions * 100.0,
                    path.traced_sessions,
                    path.sessions,
                )
                .as_str(),
            );
            if path.rank <= 3
                && let Some(trace_id) = path.trace_exemplars.first()
            {
                output.push_str(
                    format!(
                        "   Evidence: logbrew explain trace {trace_id} (same-trace anchor; not a \
                         root-cause claim).\n"
                    )
                    .as_str(),
                );
            }
        }
    }
}

/// Renders capture, property-classification, and trace-link quality receipts.
fn render_path_coverage(response: &PathsResponse, output: &mut String) {
    output.push_str(
        format!(
            "Coverage: named {}/{}; sessionized {}/{}; usable anchors {}/{}\n",
            response.coverage.named_events,
            response.coverage.classified_events,
            response.coverage.sessionized_events,
            response.coverage.classified_events,
            response.coverage.usable_anchor_events,
            response.coverage.anchor_events,
        )
        .as_str(),
    );
    if let Some(property) = response.coverage.anchor_property_filters.as_ref() {
        output.push_str(
            format!(
                "Anchor property coverage: ready {}/{}; matched {}/{}; missing keys {}; exact \
                 value mismatch {}\n",
                property.property_ready_events,
                property.context_events,
                property.matching_events,
                property.property_ready_events,
                property.missing_property_events,
                property.nonmatching_value_events,
            )
            .as_str(),
        );
    }
    if response.coverage.anchor_events > 0 {
        output.push_str(
            format!(
                "Trace evidence: {}/{} matching anchors carried a trace ID.\n",
                response.coverage.traced_anchor_events, response.coverage.anchor_events,
            )
            .as_str(),
        );
    }
    if response.coverage.unsessionized_events > 0 {
        output.push_str(
            format!(
                "Capture gap: {} classified events lacked an explicit session ID.\n",
                response.coverage.unsessionized_events
            )
            .as_str(),
        );
    }
}

/// Maps validated stable action codes to local, value-free human guidance.
fn next_step(code: &str) -> &'static str {
    match code {
        "capture_product_activity" => {
            "capture version-1 page views, screen views, or interactions, then retry"
        }
        "choose_captured_path_anchor" => {
            "choose an exact event shown in Product Analytics overview, then retry"
        }
        "capture_anchor_properties" => {
            "capture every requested safe property key on this anchor, then retry"
        }
        "improve_anchor_property_coverage" => {
            "improve property capture on this anchor before treating the paths as representative"
        }
        "verify_anchor_property_values" => {
            "verify the exact case-sensitive property values supplied for this anchor"
        }
        "sessionize_product_activity" => "attach context.session.id to product events, then retry",
        "narrow_or_move_path_anchor" => {
            "narrow the time/context scope or move the anchor closer to retained activity"
        }
        "measure_top_path_as_funnel" => {
            "measure the most material returned sequence as an exact funnel"
        }
        "inspect_path_trace" => {
            "open a returned trace exemplar as evidence in the trace investigation workspace"
        }
        "compare_path_contexts" => "repeat this anchor across releases, environments, or services",
        _ => "retry the bounded analytics path query",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(direction: AnalyticsPathDirection) -> AnalyticsPathOptions {
        AnalyticsPathOptions {
            project_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned(),
            since: "24h".to_owned(),
            until: None,
            service_name: None,
            release: None,
            environment: Some("production".to_owned()),
            direction,
            anchor_kind: AnalyticsPathEventKind::PageView,
            anchor_event: "/pricing".to_owned(),
            property_filters: Vec::new(),
            depth: 4,
            collapse_repeated: true,
            path_limit: 10,
        }
    }

    fn response(direction: &str, nodes: serde_json::Value) -> String {
        serde_json::json!({
            "schema_version": 1,
            "query": {
                "project_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "since": "2026-08-02T00:00:00Z",
                "until": "2026-08-03T00:00:00Z",
                "service_name": null,
                "release": null,
                "environment": "production",
                "direction": direction,
                "anchor": {"kind": "page_view", "event_name": "/pricing"},
                "depth": 4,
                "collapse_repeated": true,
                "path_limit": 10
            },
            "purpose": "Aggregate paths around one exact event.",
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
                "traced_anchor_events": 0,
                "event_name_rate": 0.9,
                "sessionization_rate": 0.8,
                "anchor_session_coverage_rate": 0.8,
                "anchor_trace_link_rate": 0.0,
                "ordered_event_cap_per_session": 1024,
                "limitations": ["Only classified events are included."]
            },
            "paths": [{
                "rank": 1,
                "sessions": 12,
                "share_of_anchored_sessions": 0.6,
                "traced_sessions": 0,
                "trace_link_rate": 0.0,
                "trace_exemplars": [],
                "nodes": nodes
            }],
            "next_action": {
                "code": "measure_top_path_as_funnel",
                "target": "/api/telemetry/analytics/funnel",
                "reason": "Measure the top path as a funnel."
            }
        })
        .to_string()
    }

    #[test]
    fn validates_and_renders_following_paths() {
        let body = response(
            "following",
            serde_json::json!([
                {"relative_position": 0, "kind": "page_view", "event_name": "/pricing"},
                {"relative_position": 1, "kind": "interaction", "event_name": "signup_started"}
            ]),
        );
        let response = validated_response(&options(AnalyticsPathDirection::Following), &body)
            .expect("valid response");
        let rendered = render_response(&response);
        assert!(rendered.contains("[0] page_view /pricing -> [+1] interaction signup_started"));
        assert!(rendered.contains("Capture gap: 20"));
        assert!(!rendered.contains(response.next_action.reason.as_str()));
    }

    #[test]
    fn rejects_wrong_positions_counts_actions_and_unknown_fields() {
        let options = options(AnalyticsPathDirection::Following);
        let valid_nodes = serde_json::json!([
            {"relative_position": 0, "kind": "page_view", "event_name": "/pricing"}
        ]);
        let wrong_position = response(
            "following",
            serde_json::json!([
                {"relative_position": 1, "kind": "page_view", "event_name": "/pricing"}
            ]),
        );
        assert!(validated_response(&options, &wrong_position).is_err());

        let mut wrong_count: serde_json::Value =
            serde_json::from_str(&response("following", valid_nodes.clone())).expect("json");
        wrong_count["summary"]["represented_sessions"] = 11.into();
        assert!(validated_response(&options, &wrong_count.to_string()).is_err());

        let mut wrong_action: serde_json::Value =
            serde_json::from_str(&response("following", valid_nodes.clone())).expect("json");
        wrong_action["next_action"]["target"] = "/unexpected".into();
        assert!(validated_response(&options, &wrong_action.to_string()).is_err());

        let mut unknown: serde_json::Value =
            serde_json::from_str(&response("following", valid_nodes)).expect("json");
        unknown["query"]["session_id"] = "secret".into();
        assert!(validated_response(&options, &unknown.to_string()).is_err());
    }

    #[test]
    fn validates_property_coverage_and_renders_trace_evidence_without_values() {
        let mut options = options(AnalyticsPathDirection::Following);
        options.property_filters = vec![AnalyticsPathPropertyFilter {
            key: "tag.plan".to_owned(),
            value: "sensitive-plan-marker".to_owned(),
        }];
        let nodes = serde_json::json!([
            {"relative_position": 0, "kind": "page_view", "event_name": "/pricing"},
            {"relative_position": 1, "kind": "interaction", "event_name": "signup_started"}
        ]);
        let mut body: serde_json::Value =
            serde_json::from_str(&response("following", nodes)).expect("json");
        body["query"]["property_filters"] = serde_json::json!([{
            "key": "tag.plan",
            "value": "sensitive-plan-marker"
        }]);
        body["summary"]["anchored_sessions"] = 16.into();
        body["summary"]["unrepresented_sessions"] = 4.into();
        body["coverage"]["anchor_events"] = 20.into();
        body["coverage"]["anchor_property_filters"] = serde_json::json!({
            "context_events": 30,
            "property_ready_events": 25,
            "missing_property_events": 5,
            "matching_events": 20,
            "nonmatching_value_events": 5,
            "property_ready_rate": 25.0 / 30.0,
            "match_rate": 0.8
        });
        body["coverage"]["usable_anchor_events"] = 18.into();
        body["coverage"]["excluded_anchor_events"] = 2.into();
        body["coverage"]["traced_anchor_events"] = 15.into();
        body["coverage"]["anchor_session_coverage_rate"] = 0.9.into();
        body["coverage"]["anchor_trace_link_rate"] = 0.75.into();
        body["paths"][0]["share_of_anchored_sessions"] = 0.75.into();
        body["paths"][0]["traced_sessions"] = 8.into();
        body["paths"][0]["trace_link_rate"] = (8.0 / 12.0).into();
        body["paths"][0]["trace_exemplars"] =
            serde_json::json!(["4bf92f3577b34da6a3ce929d0e0e4736"]);
        body["next_action"] = serde_json::json!({
            "code": "inspect_path_trace",
            "target": "/api/telemetry/traces/{trace_id}/investigation",
            "reason": "Inspect retained same-trace evidence without inferring causality."
        });

        let response = validated_response(&options, body.to_string().as_str())
            .expect("property-aware response validates");
        let rendered = render_response(&response);

        assert!(rendered.contains("Anchor properties: 1 exact AND predicate on tag.plan"));
        assert!(rendered.contains("ready 25/30; matched 20/25; missing keys 5"));
        assert!(rendered.contains("trace-linked 8/12"));
        assert!(rendered.contains("logbrew explain trace 4bf92f3577b34da6a3ce929d0e0e4736"));
        assert!(!rendered.contains("sensitive-plan-marker"));

        body["coverage"]["anchor_property_filters"]["missing_property_events"] = 4.into();
        assert!(validated_response(&options, body.to_string().as_str()).is_err());
    }

    #[test]
    fn request_body_omits_absent_context_and_sends_explicit_defaults() {
        let body = request_body(&options(AnalyticsPathDirection::Preceding));
        assert_eq!(body["direction"], "preceding");
        assert_eq!(body["depth"], 4);
        assert_eq!(body["collapse_repeated"], true);
        assert_eq!(body["path_limit"], 10);
        assert!(body.get("service_name").is_none());
        assert!(body.get("property_filters").is_none());
        assert!(body.get("until").is_none());
    }
}
