//! Validation of causal ordering, evidence receipts, and deterministic pivots.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::correlations::CorrelationFacts;
use super::topology::{BaselineFacts, TopologyFacts};
use super::{
    PayloadFacts, SubjectFacts, invalid_response, is_w3c_id, nullable_safe_i64, nullable_safe_u64,
    nullable_string, nullable_timestamp, require_bool, require_exact_fields, require_safe_u64,
    require_string, require_timestamp, required_array, required_object, validate_sorted_strings,
};
use crate::RuntimeError;
use crate::ids::is_uuid;
use crate::time::parse_utc_evidence;

/// Maximum mixed-signal timeline items returned by the server.
const TIMELINE_LIMIT: usize = 50;
/// Maximum evidence field receipts per category.
const EVIDENCE_FIELD_LIMIT: usize = 256;
/// Maximum generated follow-up actions.
const NEXT_ACTION_LIMIT: usize = 16;

/// Evidence facts that control capture-improvement follow-up generation.
#[derive(Clone, Copy)]
pub(super) struct EvidenceFacts {
    /// Whether typed context or SDK identity is explicitly missing.
    pub(super) capture_incomplete: bool,
}

/// One expected stable follow-up action derived from returned evidence.
#[derive(Clone, Copy)]
struct ExpectedAction<'a> {
    /// Stable action code.
    code: &'a str,
    /// Stable destination type.
    target: &'a str,
    /// Stable reason code.
    reason: &'a str,
    /// Exact span pivot when applicable.
    span_id: Option<&'a str>,
    /// Grouped issue pivot when applicable.
    issue_id: Option<&'a str>,
}

/// Validates bounded chronological evidence and exact source-item projections.
pub(super) fn validate_timeline(
    timeline: &Map<String, Value>,
    payload: &Map<String, Value>,
    correlations: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    require_exact_fields(timeline, &["items", "truncated"])?;
    let items = required_array(timeline, "items", TIMELINE_LIMIT)?;
    let truncated = require_bool(timeline, "truncated")?;
    if items.is_empty() || truncated && items.len() != TIMELINE_LIMIT {
        return Err(invalid_response());
    }
    let sources = TimelineSources::new(payload, correlations)?;
    let expected_count = 2_usize
        .saturating_add(sources.timestamped_event_count())
        .saturating_add(sources.logs.len())
        .saturating_add(sources.issues.len())
        .saturating_add(sources.actions.len())
        .saturating_add(sources.metrics.len());
    if truncated != (expected_count > TIMELINE_LIMIT) || !truncated && items.len() != expected_count
    {
        return Err(invalid_response());
    }
    let mut seen = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    let started_at = parse_utc_evidence(subject.started_at).ok_or_else(invalid_response)?;
    let mut previous = None;
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        validate_timeline_item_shape(item)?;
        let kind = require_string(item, "kind")?;
        let id = require_string(item, "id")?;
        if !seen.insert((kind, id)) {
            return Err(invalid_response());
        }
        let _ = kinds.insert(kind);
        validate_timeline_projection(item, kind, id, subject, &sources)?;
        let occurred_at = parse_utc_evidence(require_timestamp(item, "occurred_at")?)
            .ok_or_else(invalid_response)?;
        let expected_offset = crate::time::evidence_offset_millis(occurred_at, started_at)
            .ok_or_else(invalid_response)?;
        if nullable_safe_i64(item, "offset_ms")? != Some(expected_offset) {
            return Err(invalid_response());
        }
        let order = (occurred_at, timeline_kind_rank(kind)?, id);
        if previous.is_some_and(|previous| previous >= order) {
            return Err(invalid_response());
        }
        previous = Some(order);
    }
    for required in ["span_start", "span_end"] {
        if !kinds.contains(required) {
            return Err(invalid_response());
        }
    }
    for (kind, present) in [
        ("span_event", sources.timestamped_event_count() > 0),
        ("log", !sources.logs.is_empty()),
        ("issue", !sources.issues.is_empty()),
        ("action", !sources.actions.is_empty()),
        ("metric", !sources.metrics.is_empty()),
    ] {
        if present && !kinds.contains(kind) {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Strict references to the payload and correlation sources projected into a timeline.
#[derive(Clone, Copy)]
struct TimelineSources<'a> {
    /// Application-reported span events.
    events: &'a [Value],
    /// Exact-span logs.
    logs: &'a [Value],
    /// Same-trace issue occurrences.
    issues: &'a [Value],
    /// Same-trace actions.
    actions: &'a [Value],
    /// Same-trace metric exemplars.
    metrics: &'a [Value],
}

impl<'a> TimelineSources<'a> {
    /// Reads already-validated source arrays from the response envelope.
    fn new(
        payload: &'a Map<String, Value>,
        correlations: &'a Map<String, Value>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            events: required_array(payload, "events", 8)?,
            logs: correlation_items(correlations, "logs")?,
            issues: correlation_items(correlations, "issues")?,
            actions: correlation_items(correlations, "actions")?,
            metrics: correlation_items(correlations, "metrics")?,
        })
    }

    /// Counts events carrying valid timestamps and therefore represented in the timeline.
    fn timestamped_event_count(self) -> usize {
        self.events
            .iter()
            .filter(|event| event.get("timestamp").is_some_and(|value| !value.is_null()))
            .count()
    }
}

/// Reads one correlation collection's already-validated item slice.
fn correlation_items<'a>(
    correlations: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a [Value], RuntimeError> {
    required_array(required_object(correlations, name)?, "items", 20)
}

/// Validates exact timeline vocabulary and scalar types before source matching.
fn validate_timeline_item_shape(item: &Map<String, Value>) -> Result<(), RuntimeError> {
    require_exact_fields(
        item,
        &[
            "id",
            "kind",
            "occurred_at",
            "offset_ms",
            "name",
            "service_name",
            "span_id",
            "severity",
            "status",
            "duration_ms",
        ],
    )?;
    let _id = require_string(item, "id")?;
    let _kind = timeline_kind_rank(require_string(item, "kind")?)?;
    let _time = require_timestamp(item, "occurred_at")?;
    let _name = require_string(item, "name")?;
    let _service = require_string(item, "service_name")?;
    if nullable_string(item, "span_id")?.is_some_and(|span| !is_w3c_id(span, 16))
        || nullable_string(item, "severity")?
            .is_some_and(|severity| !matches!(severity, "info" | "warning" | "error" | "critical"))
    {
        return Err(invalid_response());
    }
    let _status = nullable_string(item, "status")?;
    let _duration = nullable_safe_u64(item, "duration_ms")?;
    Ok(())
}

/// Validates one timeline item as an exact projection of its source evidence.
fn validate_timeline_projection(
    item: &Map<String, Value>,
    kind: &str,
    id: &str,
    subject: &SubjectFacts<'_>,
    sources: &TimelineSources<'_>,
) -> Result<(), RuntimeError> {
    match kind {
        "span_start" => validate_span_boundary(item, id, subject, false),
        "span_end" => validate_span_boundary(item, id, subject, true),
        "span_event" => validate_span_event(item, id, subject, sources.events),
        "log" => find_source(sources.logs, id)
            .and_then(|source| validate_log_projection(item, source, subject)),
        "issue" => find_source(sources.issues, id)
            .and_then(|source| validate_issue_projection(item, source)),
        "action" => find_source(sources.actions, id)
            .and_then(|source| validate_named_signal_projection(item, source)),
        "metric" => find_source(sources.metrics, id)
            .and_then(|source| validate_named_signal_projection(item, source)),
        _ => Err(invalid_response()),
    }
}

/// Validates the exact selected-span start or end boundary.
fn validate_span_boundary(
    item: &Map<String, Value>,
    id: &str,
    subject: &SubjectFacts<'_>,
    end: bool,
) -> Result<(), RuntimeError> {
    let suffix = if end { ":end" } else { ":start" };
    let expected_id = format!("{}{}", subject.span_id, suffix);
    let expected_offset = if end {
        i64::try_from(subject.duration_ms).map_err(|_error| invalid_response())?
    } else {
        0
    };
    if id != expected_id
        || require_string(item, "name")? != subject.name
        || require_string(item, "service_name")? != subject.service_name
        || nullable_string(item, "span_id")? != Some(subject.span_id)
        || nullable_string(item, "severity")?.is_some()
        || nullable_string(item, "status")? != subject.status
        || nullable_safe_u64(item, "duration_ms")? != Some(subject.duration_ms)
        || item.get("offset_ms").and_then(Value::as_i64) != Some(expected_offset)
        || !end && require_timestamp(item, "occurred_at")? != subject.started_at
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates one indexed application event projection.
fn validate_span_event(
    item: &Map<String, Value>,
    id: &str,
    subject: &SubjectFacts<'_>,
    events: &[Value],
) -> Result<(), RuntimeError> {
    let index = id
        .strip_prefix("span-event:")
        .and_then(|index| index.parse::<usize>().ok())
        .filter(|index| index.to_string() == id.trim_start_matches("span-event:"))
        .ok_or_else(invalid_response)?;
    let event = events
        .get(index)
        .and_then(Value::as_object)
        .ok_or_else(invalid_response)?;
    let timestamp = nullable_timestamp(event, "timestamp")?.ok_or_else(invalid_response)?;
    let offset = nullable_safe_i64(event, "offset_ms")?.ok_or_else(invalid_response)?;
    if require_timestamp(item, "occurred_at")? != timestamp
        || item.get("offset_ms").and_then(Value::as_i64) != Some(offset)
        || require_string(item, "name")? != require_string(event, "name")?
        || require_string(item, "service_name")? != subject.service_name
        || nullable_string(item, "span_id")? != Some(subject.span_id)
        || nullable_string(item, "severity")?.is_some()
        || nullable_string(item, "status")?.is_some()
        || nullable_safe_u64(item, "duration_ms")?.is_some()
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Returns the exact source object carrying one stable row ID.
fn find_source<'a>(values: &'a [Value], id: &str) -> Result<&'a Map<String, Value>, RuntimeError> {
    values
        .iter()
        .filter_map(Value::as_object)
        .find(|source| source.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(invalid_response)
}

/// Validates one exact-span log timeline projection.
fn validate_log_projection(
    item: &Map<String, Value>,
    source: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    if require_timestamp(item, "occurred_at")? != require_timestamp(source, "occurred_at")?
        || require_string(item, "name")? != require_string(source, "source")?
        || require_string(item, "service_name")? != require_string(source, "service_name")?
        || nullable_string(item, "span_id")? != Some(subject.span_id)
        || nullable_string(item, "severity")? != Some(require_string(source, "severity")?)
        || nullable_string(item, "status")?.is_some()
        || nullable_safe_u64(item, "duration_ms")?.is_some()
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates one issue occurrence timeline projection.
fn validate_issue_projection(
    item: &Map<String, Value>,
    source: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    if require_timestamp(item, "occurred_at")? != require_timestamp(source, "occurred_at")?
        || require_string(item, "name")? != require_string(source, "title")?
        || require_string(item, "service_name")? != require_string(source, "service_name")?
        || nullable_string(item, "severity")? != Some(require_string(source, "severity")?)
        || nullable_string(item, "span_id")?.is_some()
        || nullable_string(item, "status")?.is_some()
        || nullable_safe_u64(item, "duration_ms")?.is_some()
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates the shared action and metric timeline projection.
fn validate_named_signal_projection(
    item: &Map<String, Value>,
    source: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    if require_timestamp(item, "occurred_at")? != require_timestamp(source, "occurred_at")?
        || require_string(item, "name")? != require_string(source, "name")?
        || require_string(item, "service_name")? != require_string(source, "service_name")?
        || any_optional_signal_field(item)
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Returns whether an event/action/metric unexpectedly carries span-only optional fields.
fn any_optional_signal_field(item: &Map<String, Value>) -> bool {
    item.get("span_id").is_none_or(|value| !value.is_null())
        || item.get("severity").is_none_or(|value| !value.is_null())
        || item.get("status").is_none_or(|value| !value.is_null())
        || item.get("duration_ms").is_none_or(|value| !value.is_null())
}

/// Returns the server enum's deterministic timeline-kind order.
fn timeline_kind_rank(kind: &str) -> Result<u8, RuntimeError> {
    match kind {
        "span_start" => Ok(0),
        "span_event" => Ok(1),
        "log" => Ok(2),
        "issue" => Ok(3),
        "action" => Ok(4),
        "metric" => Ok(5),
        "span_end" => Ok(6),
        _ => Err(invalid_response()),
    }
}

/// Validates explicit captured, missing, redacted, and truncated evidence state.
#[expect(
    clippy::too_many_arguments,
    reason = "evidence validation binds every independently serialized source receipt"
)]
pub(super) fn validate_evidence(
    evidence: &Map<String, Value>,
    context: Option<&Value>,
    subject: &SubjectFacts<'_>,
    payload: PayloadFacts,
    topology: &TopologyFacts<'_>,
    baseline: &BaselineFacts<'_>,
    correlations: &CorrelationFacts<'_>,
    timeline: &Map<String, Value>,
) -> Result<EvidenceFacts, RuntimeError> {
    require_exact_fields(
        evidence,
        &[
            "status",
            "captured_fields",
            "missing_fields",
            "redacted_fields",
            "truncated_fields",
        ],
    )?;
    let captured = validate_sorted_strings(evidence, "captured_fields", EVIDENCE_FIELD_LIMIT)?;
    let missing = validate_sorted_strings(evidence, "missing_fields", EVIDENCE_FIELD_LIMIT)?;
    let redacted = validate_sorted_strings(evidence, "redacted_fields", EVIDENCE_FIELD_LIMIT)?;
    let truncated = validate_sorted_strings(evidence, "truncated_fields", EVIDENCE_FIELD_LIMIT)?;
    let status = require_string(evidence, "status")?;
    let complete = missing.is_empty() && redacted.is_empty() && truncated.is_empty();
    if status != if complete { "complete" } else { "partial" } {
        return Err(invalid_response());
    }
    for required in [
        "subject.content_trust",
        "subject.deployment",
        "subject.identity",
        "subject.timing",
    ] {
        require_membership(&captured, &missing, required, true)?;
    }
    require_membership(&captured, &missing, "subject.sdk", subject.sdk_captured)?;
    let context_present = context.is_some_and(Value::is_object);
    let context_redacted = redacted.iter().any(|field| field.starts_with("context"));
    if context_present {
        require_membership(&captured, &missing, "context", true)?;
    } else if context_redacted {
        if captured.contains(&"context") || missing.contains(&"context") {
            return Err(invalid_response());
        }
    } else {
        require_membership(&captured, &missing, "context", false)?;
    }
    for (field, status) in [
        ("topology", topology.status),
        ("baseline", baseline.status),
        ("correlations.trace", correlations.trace.status),
        ("correlations.issues", correlations.issue_state.status),
        ("correlations.logs", correlations.log_state.status),
        ("correlations.actions", correlations.action_state.status),
        ("correlations.metrics", correlations.metric_state.status),
    ] {
        require_membership(&captured, &missing, field, status != "unavailable")?;
    }
    for (field, expected) in [
        ("topology", topology.truncated),
        ("correlations.trace", correlations.trace.truncated),
        ("correlations.issues", correlations.issue_state.truncated),
        ("correlations.logs", correlations.log_state.truncated),
        ("correlations.actions", correlations.action_state.truncated),
        ("correlations.metrics", correlations.metric_state.truncated),
        ("timeline", require_bool(timeline, "truncated")?),
    ] {
        if truncated.contains(&field) != expected {
            return Err(invalid_response());
        }
    }
    let attribute_redactions = redacted
        .iter()
        .any(|field| field.starts_with("attributes."));
    let attribute_truncations = truncated
        .iter()
        .any(|field| field.starts_with("attributes."));
    if payload.redacted != attribute_redactions || payload.truncated != attribute_truncations {
        return Err(invalid_response());
    }
    validate_evidence_field_states(&captured, &missing, &redacted, &truncated)?;
    Ok(EvidenceFacts {
        capture_incomplete: missing.contains(&"context") || missing.contains(&"subject.sdk"),
    })
}

/// Requires exactly one captured-or-missing membership for one normalized source field.
fn require_membership(
    captured: &[&str],
    missing: &[&str],
    field: &str,
    should_be_captured: bool,
) -> Result<(), RuntimeError> {
    if captured.contains(&field) == should_be_captured
        && missing.contains(&field) != should_be_captured
    {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Allows partial capture while rejecting missing/present contradictions and invalid field paths.
fn validate_evidence_field_states(
    captured: &[&str],
    missing: &[&str],
    redacted: &[&str],
    truncated: &[&str],
) -> Result<(), RuntimeError> {
    let mut categories = BTreeMap::<&str, u8>::new();
    for (index, fields) in [captured, missing, redacted, truncated]
        .into_iter()
        .enumerate()
    {
        let bit = 1_u8
            .checked_shl(u32::try_from(index).map_err(|_error| invalid_response())?)
            .ok_or_else(invalid_response)?;
        for field in fields {
            if field.chars().count() > 256 || field.chars().any(char::is_control) {
                return Err(invalid_response());
            }
            let entry = categories.entry(field).or_default();
            *entry |= bit;
            // Attribute paths can summarize both retained and withheld values. A bounded
            // collection can remain present while its retained history is incomplete.
            let partial = field.starts_with("attributes.") && *entry & 2 == 0
                || *entry == 9
                    && matches!(
                        *field,
                        "context"
                            | "topology"
                            | "timeline"
                            | "correlations.trace"
                            | "correlations.issues"
                            | "correlations.logs"
                            | "correlations.actions"
                            | "correlations.metrics"
                    );
            if entry.count_ones() > 1 && !partial {
                return Err(invalid_response());
            }
        }
    }
    Ok(())
}

/// Validates stable ordered follow-up actions as an exact evidence projection.
pub(super) fn validate_next_actions(
    value: Option<&Value>,
    subject: &SubjectFacts<'_>,
    topology: &TopologyFacts<'_>,
    baseline: &BaselineFacts<'_>,
    correlations: &CorrelationFacts<'_>,
    evidence: EvidenceFacts,
) -> Result<(), RuntimeError> {
    let expected = expected_actions(subject, topology, baseline, correlations, evidence);
    let actions = value
        .and_then(Value::as_array)
        .filter(|actions| !actions.is_empty() && actions.len() <= NEXT_ACTION_LIMIT)
        .ok_or_else(invalid_response)?;
    if actions.len() != expected.len() {
        return Err(invalid_response());
    }
    for (index, (action, expected)) in actions.iter().zip(expected).enumerate() {
        validate_next_action(action, index, expected)?;
    }
    Ok(())
}

/// Derives ordered investigation pivots from one canonical action catalog.
fn expected_actions<'a>(
    subject: &SubjectFacts<'a>,
    topology: &TopologyFacts<'a>,
    baseline: &BaselineFacts<'_>,
    correlations: &CorrelationFacts<'a>,
    evidence: EvidenceFacts,
) -> Vec<ExpectedAction<'a>> {
    let issue = correlations
        .issues
        .first()
        .and_then(|issue| issue.get("issue_id")?.as_str());
    let unavailable = [
        correlations.trace.status,
        baseline.status,
        correlations.issue_state.status,
        correlations.log_state.status,
        correlations.action_state.status,
        correlations.metric_state.status,
    ]
    .contains(&"unavailable");
    let span = Some(subject.span_id);
    [
        (
            topology.parent_span_id.is_some(),
            "inspect_parent_span",
            "span_investigation",
            "parent_context_available",
            topology.parent_span_id,
            None,
        ),
        (
            topology.error_child_span_id.is_some(),
            "inspect_error_child",
            "span_investigation",
            "child_error_observed",
            topology.error_child_span_id,
            None,
        ),
        (
            !correlations.logs.is_empty(),
            "review_exact_span_logs",
            "telemetry_logs",
            "exact_span_logs_available",
            span,
            None,
        ),
        (
            issue.is_some(),
            "review_related_issue",
            "issue_investigation",
            "related_issue_available",
            None,
            issue,
        ),
        (
            correlations.trace.status == "available",
            "inspect_trace",
            "trace_investigation",
            "trace_context_available",
            span,
            None,
        ),
        (
            baseline.status == "available" && baseline.at_or_above_p95,
            "compare_peer_baseline",
            "peer_baseline",
            "subject_latency_elevated",
            span,
            None,
        ),
        (
            true,
            "compare_release",
            "release_investigation",
            "exact_release_available",
            None,
            None,
        ),
        (
            unavailable,
            "retry_unavailable_evidence",
            "exact_span_investigation",
            "optional_evidence_unavailable",
            span,
            None,
        ),
        (
            evidence.capture_incomplete,
            "improve_capture",
            "sdk_capture",
            "capture_incomplete",
            span,
            None,
        ),
    ]
    .into_iter()
    .filter(|(present, ..)| *present)
    .map(
        |(_, code, target, reason, span_id, issue_id)| ExpectedAction {
            code,
            target,
            reason,
            span_id,
            issue_id,
        },
    )
    .collect()
}

/// Validates one generated action's exact vocabulary, identity, and one-based priority.
fn validate_next_action(
    value: &Value,
    index: usize,
    expected: ExpectedAction<'_>,
) -> Result<(), RuntimeError> {
    let action = value.as_object().ok_or_else(invalid_response)?;
    require_exact_fields(
        action,
        &[
            "priority", "code", "target", "reason", "span_id", "issue_id",
        ],
    )?;
    if require_safe_u64(action, "priority")?
        != u64::try_from(index.saturating_add(1)).map_err(|_error| invalid_response())?
        || require_string(action, "code")? != expected.code
        || require_string(action, "target")? != expected.target
        || require_string(action, "reason")? != expected.reason
        || nullable_string(action, "span_id")? != expected.span_id
        || nullable_string(action, "issue_id")? != expected.issue_id
        || nullable_string(action, "span_id")?.is_some_and(|span| !is_w3c_id(span, 16))
        || nullable_string(action, "issue_id")?.is_some_and(|issue| !is_uuid(issue))
    {
        return Err(invalid_response());
    }
    Ok(())
}
