//! Validation of exact-trace and exact-span correlated evidence.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::topology::TopologyFacts;
use super::{
    SubjectFacts, availability, invalid_response, is_w3c_id, nullable_string, nullable_w3c_id,
    require_bool, require_exact_fields, require_safe_u64, require_string, require_timestamp,
    required_array, required_nullable_object, required_object,
};
use crate::RuntimeError;
use crate::ids::is_uuid;

/// Maximum evidence rows returned per related signal kind.
const CORRELATION_LIMIT: usize = 20;
/// Maximum spans participating in one bounded containing-trace summary.
const TRACE_SPAN_LIMIT: usize = 1_000;

/// Validated correlation state used by analysis, evidence, and follow-up checks.
#[derive(Debug, Clone, Copy)]
pub(super) struct CorrelationFacts<'a> {
    /// Containing-trace read state.
    pub(super) trace: SourceState<'a>,
    /// Same-trace issue occurrences.
    pub(super) issues: &'a [Value],
    /// Issue collection read state.
    pub(super) issue_state: SourceState<'a>,
    /// Exact-span logs.
    pub(super) logs: &'a [Value],
    /// Log collection read state.
    pub(super) log_state: SourceState<'a>,
    /// Action collection read state.
    pub(super) action_state: SourceState<'a>,
    /// Metric collection read state.
    pub(super) metric_state: SourceState<'a>,
    /// Whether an exact-span log carries error or critical severity.
    pub(super) exact_error_log: bool,
}

/// One collection's validated availability and truncation receipt.
#[derive(Debug, Clone, Copy)]
pub(super) struct SourceState<'a> {
    /// Read status.
    pub(super) status: &'a str,
    /// Whether additional rows were omitted.
    pub(super) truncated: bool,
}

/// Validates all exact-span and exact-trace correlation containers.
pub(super) fn validate_correlations<'a>(
    correlations: &'a Map<String, Value>,
    subject: &SubjectFacts<'_>,
    topology: &TopologyFacts<'_>,
) -> Result<CorrelationFacts<'a>, RuntimeError> {
    require_exact_fields(
        correlations,
        &["trace", "issues", "logs", "actions", "metrics", "release"],
    )?;
    let trace = required_object(correlations, "trace")?;
    let trace_state = validate_trace(trace, subject, topology)?;
    let issues_object = required_object(correlations, "issues")?;
    let logs_object = required_object(correlations, "logs")?;
    let actions_object = required_object(correlations, "actions")?;
    let metrics_object = required_object(correlations, "metrics")?;
    let issues = required_array(issues_object, "items", CORRELATION_LIMIT)?;
    let logs = required_array(logs_object, "items", CORRELATION_LIMIT)?;
    let actions = required_array(actions_object, "items", CORRELATION_LIMIT)?;
    let metrics = required_array(metrics_object, "items", CORRELATION_LIMIT)?;
    let issue_state = validate_collection(issues_object, issues, validate_issue, subject)?;
    let log_state = validate_collection(logs_object, logs, validate_log, subject)?;
    let action_state = validate_collection(actions_object, actions, validate_action, subject)?;
    let metric_state = validate_collection(metrics_object, metrics, validate_metric, subject)?;
    validate_release(required_object(correlations, "release")?, subject)?;
    Ok(CorrelationFacts {
        trace: trace_state,
        issues,
        issue_state,
        logs,
        log_state,
        action_state,
        metric_state,
        exact_error_log: logs.iter().any(|log| {
            log.get("severity")
                .and_then(Value::as_str)
                .is_some_and(|severity| matches!(severity, "error" | "critical"))
        }),
    })
}

/// Validates containing-trace availability and a strict aggregate summary.
fn validate_trace<'a>(
    trace: &'a Map<String, Value>,
    subject: &SubjectFacts<'_>,
    topology: &TopologyFacts<'_>,
) -> Result<SourceState<'a>, RuntimeError> {
    require_exact_fields(trace, &["status", "summary", "truncated"])?;
    let status = availability(trace, "status")?;
    let summary = required_nullable_object(trace.get("summary"))?;
    let truncated = require_bool(trace, "truncated")?;
    match status {
        "available" if summary.is_some() => {
            validate_trace_summary(summary.ok_or_else(invalid_response)?, subject)?;
        }
        "not_found" | "unavailable" if summary.is_none() && !truncated => {}
        _ => return Err(invalid_response()),
    }
    if status != topology.status || truncated && !topology.truncated {
        return Err(invalid_response());
    }
    Ok(SourceState { status, truncated })
}

/// Validates aggregate trace counts, bounded span summaries, services, and exact scope.
fn validate_trace_summary(
    summary: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        summary,
        &[
            "trace_id",
            "span_count",
            "error_span_count",
            "service_count",
            "project_count",
            "started_at",
            "duration_ms",
            "root_span",
            "slowest_child_span",
            "slowest_path",
            "error_spans",
            "services",
            "releases",
            "environments",
        ],
    )?;
    if require_string(summary, "trace_id")? != subject.trace_id {
        return Err(invalid_response());
    }
    let span_count = require_safe_u64(summary, "span_count")?;
    let error_count = require_safe_u64(summary, "error_span_count")?;
    let service_count = require_safe_u64(summary, "service_count")?;
    if span_count == 0
        || span_count > u64::try_from(TRACE_SPAN_LIMIT).map_err(|_error| invalid_response())?
        || error_count > span_count
        || require_safe_u64(summary, "project_count")? != 1
    {
        return Err(invalid_response());
    }
    let _started_at = require_timestamp(summary, "started_at")?;
    let _duration = require_safe_u64(summary, "duration_ms")?;
    let root = required_nullable_object(summary.get("root_span"))?;
    if root.is_none() {
        return Err(invalid_response());
    }
    if let Some(root) = root {
        validate_span_summary(root)?;
    }
    if let Some(slowest) = required_nullable_object(summary.get("slowest_child_span"))? {
        validate_span_summary(slowest)?;
    }
    let slowest_path = required_array(summary, "slowest_path", TRACE_SPAN_LIMIT)?;
    let error_spans = required_array(summary, "error_spans", TRACE_SPAN_LIMIT)?;
    validate_summary_span_array(slowest_path, false)?;
    validate_summary_span_array(error_spans, true)?;
    if u64::try_from(error_spans.len()).map_err(|_error| invalid_response())? > error_count {
        return Err(invalid_response());
    }
    let services = required_array(summary, "services", TRACE_SPAN_LIMIT)?;
    if service_count != u64::try_from(services.len()).map_err(|_error| invalid_response())? {
        return Err(invalid_response());
    }
    validate_services(services, span_count, error_count, subject.service_name)?;
    validate_exact_single_string(summary, "releases", subject.release)?;
    validate_exact_single_string(summary, "environments", subject.environment)
}

/// Validates a summary span and its canonical identity/timing fields.
fn validate_span_summary(value: &Map<String, Value>) -> Result<(), RuntimeError> {
    require_exact_fields(
        value,
        &[
            "span_id",
            "parent_span_id",
            "name",
            "operation",
            "status",
            "started_at",
            "duration_ms",
            "service_name",
        ],
    )?;
    if !is_w3c_id(require_string(value, "span_id")?, 16) {
        return Err(invalid_response());
    }
    let _parent = nullable_w3c_id(value, "parent_span_id", 16)?;
    let _name = require_string(value, "name")?;
    let _operation = require_string(value, "operation")?;
    let _status = nullable_string(value, "status")?;
    let _started_at = require_timestamp(value, "started_at")?;
    let _duration = require_safe_u64(value, "duration_ms")?;
    let _service = require_string(value, "service_name")?;
    Ok(())
}

/// Validates one bounded unique span-summary list.
fn validate_summary_span_array(values: &[Value], require_error: bool) -> Result<(), RuntimeError> {
    let mut seen = BTreeSet::new();
    for value in values {
        let span = value.as_object().ok_or_else(invalid_response)?;
        validate_span_summary(span)?;
        let span_id = require_string(span, "span_id")?;
        if !seen.insert(span_id)
            || require_error && !super::is_error_status(nullable_string(span, "status")?)
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates deterministic per-service count arithmetic and subject-service coverage.
fn validate_services(
    services: &[Value],
    span_count: u64,
    error_count: u64,
    subject_service: &str,
) -> Result<(), RuntimeError> {
    let mut previous = None;
    let mut spans = 0_u64;
    let mut errors = 0_u64;
    let mut includes_subject = false;
    for service in services {
        let service = service.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            service,
            &[
                "service_name",
                "span_count",
                "error_span_count",
                "max_duration_ms",
            ],
        )?;
        let name = require_string(service, "service_name")?;
        if previous.is_some_and(|previous: &str| previous >= name) {
            return Err(invalid_response());
        }
        previous = Some(name);
        includes_subject |= name == subject_service;
        let service_spans = require_safe_u64(service, "span_count")?;
        let service_errors = require_safe_u64(service, "error_span_count")?;
        if service_spans == 0 || service_errors > service_spans {
            return Err(invalid_response());
        }
        let _max_duration = require_safe_u64(service, "max_duration_ms")?;
        spans = spans.saturating_add(service_spans);
        errors = errors.saturating_add(service_errors);
    }
    if spans != span_count || errors != error_count || !includes_subject {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates an exact one-element sorted release or environment list.
fn validate_exact_single_string(
    value: &Map<String, Value>,
    name: &str,
    expected: &str,
) -> Result<(), RuntimeError> {
    let values = required_array(value, name, 1)?;
    if values.len() == 1 && values[0].as_str() == Some(expected) {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates a collection state, chronological order, and every scoped item.
fn validate_collection<'a>(
    collection: &'a Map<String, Value>,
    items: &'a [Value],
    validate_item: fn(&Map<String, Value>, &SubjectFacts<'_>) -> Result<(), RuntimeError>,
    subject: &SubjectFacts<'_>,
) -> Result<SourceState<'a>, RuntimeError> {
    require_exact_fields(collection, &["status", "items", "truncated"])?;
    let status = availability(collection, "status")?;
    let truncated = require_bool(collection, "truncated")?;
    match status {
        "available" if !items.is_empty() => {}
        "not_found" | "unavailable" if items.is_empty() && !truncated => {}
        _ => return Err(invalid_response()),
    }
    if truncated && items.len() != CORRELATION_LIMIT {
        return Err(invalid_response());
    }
    let mut previous: Option<(&str, &str)> = None;
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        validate_item(item, subject)?;
        let tuple = (
            require_timestamp(item, "occurred_at")?,
            require_string(item, "id")?,
        );
        if previous.is_some_and(|previous| previous >= tuple) {
            return Err(invalid_response());
        }
        previous = Some(tuple);
    }
    Ok(SourceState { status, truncated })
}

/// Validates one exact-trace issue occurrence.
fn validate_issue(
    item: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        item,
        &[
            "id",
            "issue_id",
            "project_id",
            "severity",
            "title",
            "message",
            "occurred_at",
            "service_name",
            "environment",
            "release",
        ],
    )?;
    if !is_uuid(require_string(item, "id")?) || !is_uuid(require_string(item, "issue_id")?) {
        return Err(invalid_response());
    }
    validate_common_scope(item, subject)?;
    validate_severity(item)?;
    let _title = require_string(item, "title")?;
    let _message = require_string(item, "message")?;
    Ok(())
}

/// Validates one exact-span structured log.
fn validate_log(item: &Map<String, Value>, subject: &SubjectFacts<'_>) -> Result<(), RuntimeError> {
    require_exact_fields(
        item,
        &[
            "id",
            "project_id",
            "severity",
            "source",
            "message",
            "occurred_at",
            "service_name",
            "span_id",
            "environment",
            "release",
        ],
    )?;
    if !is_uuid(require_string(item, "id")?)
        || nullable_w3c_id(item, "span_id", 16)? != Some(subject.span_id)
    {
        return Err(invalid_response());
    }
    validate_common_scope(item, subject)?;
    validate_severity(item)?;
    let _source = require_string(item, "source")?;
    let _message = require_string(item, "message")?;
    Ok(())
}

/// Validates one exact-trace product action.
fn validate_action(
    item: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        item,
        &[
            "id",
            "project_id",
            "name",
            "occurred_at",
            "service_name",
            "environment",
            "release",
        ],
    )?;
    if !is_uuid(require_string(item, "id")?) {
        return Err(invalid_response());
    }
    validate_common_scope(item, subject)?;
    let _name = require_string(item, "name")?;
    Ok(())
}

/// Validates one exact-trace metric exemplar without inferring an anomaly.
fn validate_metric(
    item: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        item,
        &[
            "id",
            "project_id",
            "name",
            "kind",
            "value",
            "unit",
            "temporality",
            "occurred_at",
            "service_name",
            "environment",
            "release",
        ],
    )?;
    if !is_uuid(require_string(item, "id")?)
        || item
            .get("value")
            .and_then(Value::as_f64)
            .is_none_or(|value| !value.is_finite())
    {
        return Err(invalid_response());
    }
    validate_common_scope(item, subject)?;
    let _name = require_string(item, "name")?;
    let _kind = require_string(item, "kind")?;
    let _unit = nullable_string(item, "unit")?;
    let _temporality = nullable_string(item, "temporality")?;
    Ok(())
}

/// Validates common exact project/deployment scope and occurrence time.
fn validate_common_scope(
    item: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    if require_string(item, "project_id")? != subject.project_id
        || require_string(item, "environment")? != subject.environment
        || require_string(item, "release")? != subject.release
        || !is_uuid(require_string(item, "project_id")?)
    {
        return Err(invalid_response());
    }
    let _occurred_at = require_timestamp(item, "occurred_at")?;
    let _service = require_string(item, "service_name")?;
    Ok(())
}

/// Validates one canonical user-facing severity.
fn validate_severity(item: &Map<String, Value>) -> Result<(), RuntimeError> {
    if matches!(
        require_string(item, "severity")?,
        "info" | "warning" | "error" | "critical"
    ) {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Validates the release-investigation scope echoed by the exact-span response.
fn validate_release(
    release: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        release,
        &["project_id", "release", "environment", "service_name"],
    )?;
    for (field, expected) in [
        ("project_id", subject.project_id),
        ("release", subject.release),
        ("environment", subject.environment),
        ("service_name", subject.service_name),
    ] {
        if require_string(release, field)? != expected {
            return Err(invalid_response());
        }
    }
    Ok(())
}
