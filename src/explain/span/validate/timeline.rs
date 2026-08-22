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
    let mut previous: Option<(TimestampKey<'_>, u8, &str)> = None;
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
        let order = (
            timestamp_key(require_timestamp(item, "occurred_at")?)?,
            timeline_kind_rank(kind)?,
            id,
        );
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
    let offset = item
        .get("offset_ms")
        .and_then(Value::as_i64)
        .filter(|offset| offset.unsigned_abs() <= super::MAX_SAFE_JSON_INTEGER)
        .ok_or_else(invalid_response)?;
    let _ = offset;
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
            .and_then(|source| validate_action_projection(item, source)),
        "metric" => find_source(sources.metrics, id)
            .and_then(|source| validate_metric_projection(item, source)),
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

/// Validates one product action timeline projection.
fn validate_action_projection(
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

/// Validates one metric exemplar timeline projection.
fn validate_metric_projection(
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

/// Stable chronological key for one UTC RFC3339 timestamp.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TimestampKey<'a> {
    /// Fixed calendar and whole-second prefix.
    seconds: &'a str,
    /// Nanosecond-normalized fractional part.
    nanos: u32,
}

/// Converts a validated UTC timestamp into a comparison key.
fn timestamp_key(value: &str) -> Result<TimestampKey<'_>, RuntimeError> {
    let seconds = value.get(..19).ok_or_else(invalid_response)?;
    let suffix = value.get(19..).ok_or_else(invalid_response)?;
    let without_zone = suffix
        .strip_suffix('Z')
        .or_else(|| suffix.strip_suffix("+00:00"))
        .ok_or_else(invalid_response)?;
    let digits = without_zone.strip_prefix('.').unwrap_or(without_zone);
    if !digits.is_empty() && (digits.len() > 9 || !digits.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(invalid_response());
    }
    let mut nanos = if digits.is_empty() {
        0
    } else {
        digits.parse::<u32>().map_err(|_error| invalid_response())?
    };
    for _ in digits.len()..9 {
        nanos = nanos.saturating_mul(10);
    }
    Ok(TimestampKey { seconds, nanos })
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
    validate_evidence_sets_disjoint(&captured, &missing, &redacted, &truncated)?;
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

/// Rejects one exact evidence field appearing in multiple semantic categories.
fn validate_evidence_sets_disjoint(
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
            if entry.count_ones() > 1 {
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
    let mut expected = Vec::new();
    append_topology_actions(
        &mut expected,
        topology.parent_span_id,
        topology.error_child_span_id,
    );
    let issue_id = correlations
        .issues
        .first()
        .and_then(|issue| issue.get("issue_id"))
        .and_then(Value::as_str);
    append_correlation_actions(
        &mut expected,
        subject.span_id,
        correlations.logs.is_empty(),
        issue_id,
        correlations.trace.status,
    );
    let unavailable = [
        correlations.trace.status,
        baseline.status,
        correlations.issue_state.status,
        correlations.log_state.status,
        correlations.action_state.status,
        correlations.metric_state.status,
    ]
    .contains(&"unavailable");
    append_follow_up_actions(
        &mut expected,
        subject.span_id,
        baseline.status == "available" && baseline.at_or_above_p95,
        unavailable,
        evidence.capture_incomplete,
    );
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

/// Adds deterministic direct-parent and first-error-child pivots.
fn append_topology_actions<'a>(
    actions: &mut Vec<ExpectedAction<'a>>,
    parent: Option<&'a str>,
    error_child: Option<&'a str>,
) {
    if let Some(parent) = parent {
        actions.push(ExpectedAction {
            code: "inspect_parent_span",
            target: "span_investigation",
            reason: "parent_context_available",
            span_id: Some(parent),
            issue_id: None,
        });
    }
    if let Some(child) = error_child {
        actions.push(ExpectedAction {
            code: "inspect_error_child",
            target: "span_investigation",
            reason: "child_error_observed",
            span_id: Some(child),
            issue_id: None,
        });
    }
}

/// Adds exact-span log, issue, and containing-trace pivots.
fn append_correlation_actions<'a>(
    actions: &mut Vec<ExpectedAction<'a>>,
    subject_span: &'a str,
    logs_empty: bool,
    issue_id: Option<&'a str>,
    trace_status: &str,
) {
    if !logs_empty {
        actions.push(ExpectedAction {
            code: "review_exact_span_logs",
            target: "telemetry_logs",
            reason: "exact_span_logs_available",
            span_id: Some(subject_span),
            issue_id: None,
        });
    }
    if let Some(issue_id) = issue_id {
        actions.push(ExpectedAction {
            code: "review_related_issue",
            target: "issue_investigation",
            reason: "related_issue_available",
            span_id: None,
            issue_id: Some(issue_id),
        });
    }
    if trace_status == "available" {
        actions.push(ExpectedAction {
            code: "inspect_trace",
            target: "trace_investigation",
            reason: "trace_context_available",
            span_id: Some(subject_span),
            issue_id: None,
        });
    }
}

/// Adds baseline, release, recovery, and capture-quality pivots.
fn append_follow_up_actions<'a>(
    actions: &mut Vec<ExpectedAction<'a>>,
    subject_span: &'a str,
    latency_elevated: bool,
    evidence_unavailable: bool,
    capture_incomplete: bool,
) {
    if latency_elevated {
        actions.push(ExpectedAction {
            code: "compare_peer_baseline",
            target: "peer_baseline",
            reason: "subject_latency_elevated",
            span_id: Some(subject_span),
            issue_id: None,
        });
    }
    actions.push(ExpectedAction {
        code: "compare_release",
        target: "release_investigation",
        reason: "exact_release_available",
        span_id: None,
        issue_id: None,
    });
    if evidence_unavailable {
        actions.push(ExpectedAction {
            code: "retry_unavailable_evidence",
            target: "exact_span_investigation",
            reason: "optional_evidence_unavailable",
            span_id: Some(subject_span),
            issue_id: None,
        });
    }
    if capture_incomplete {
        actions.push(ExpectedAction {
            code: "improve_capture",
            target: "sdk_capture",
            reason: "capture_incomplete",
            span_id: Some(subject_span),
            issue_id: None,
        });
    }
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
