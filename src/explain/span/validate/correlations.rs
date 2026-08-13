//! Validation of exact-trace and exact-span correlated evidence.

use serde_json::{Map, Value};

use super::super::super::{
    TraceSummaryExpectation, validate_correlated_collection, validate_correlated_log,
    validate_correlated_signal, validate_exact_release_scope, validate_shared_trace_summary,
};
use super::topology::TopologyFacts;
use super::{
    SubjectFacts, availability, invalid_response, require_bool, require_exact_fields,
    required_nullable_object, required_object,
};
use crate::RuntimeError;

/// Maximum evidence rows returned per related signal kind.
const CORRELATION_LIMIT: usize = 20;
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
    let (issues, issue_state) = validate_collection(issues_object, validate_issue, subject)?;
    let (logs, log_state) = validate_collection(logs_object, validate_log, subject)?;
    let (_, action_state) = validate_collection(actions_object, validate_action, subject)?;
    let (_, metric_state) = validate_collection(metrics_object, validate_metric, subject)?;
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
            validate_shared_trace_summary(
                summary.ok_or_else(invalid_response)?,
                Some(&TraceSummaryExpectation {
                    trace_id: subject.trace_id,
                    service_name: subject.service_name,
                    release: subject.release,
                    environment: subject.environment,
                }),
            )?;
        }
        "not_found" | "unavailable" if summary.is_none() && !truncated => {}
        _ => return Err(invalid_response()),
    }
    if status != topology.status || truncated && !topology.truncated {
        return Err(invalid_response());
    }
    Ok(SourceState { status, truncated })
}

/// Validates a collection state, chronological order, and every scoped item.
fn validate_collection<'a>(
    collection: &'a Map<String, Value>,
    validate_item: fn(&Map<String, Value>, &SubjectFacts<'_>) -> Result<(), RuntimeError>,
    subject: &SubjectFacts<'_>,
) -> Result<(&'a [Value], SourceState<'a>), RuntimeError> {
    let (status, truncated, items) =
        validate_correlated_collection(collection, CORRELATION_LIMIT, false, |item| {
            validate_item(item, subject)
        })?;
    Ok((items, SourceState { status, truncated }))
}

/// Validates one exact-trace issue occurrence.
fn validate_issue(
    item: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    validate_correlated_signal(
        item,
        subject.project_id,
        subject.environment,
        subject.release,
        "issue",
        None,
    )
    .map(drop)
}

/// Validates one exact-span structured log.
fn validate_log(item: &Map<String, Value>, subject: &SubjectFacts<'_>) -> Result<(), RuntimeError> {
    validate_correlated_log(
        item,
        subject.project_id,
        subject.environment,
        subject.release,
        Some(subject.span_id),
        None,
    )
}

/// Validates one exact-trace product action.
fn validate_action(
    item: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    validate_correlated_signal(
        item,
        subject.project_id,
        subject.environment,
        subject.release,
        "action",
        None,
    )
    .map(drop)
}

/// Validates one exact-trace metric exemplar without inferring an anomaly.
fn validate_metric(
    item: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    validate_correlated_signal(
        item,
        subject.project_id,
        subject.environment,
        subject.release,
        "metric",
        None,
    )
    .map(drop)
}

/// Validates the release-investigation scope echoed by the exact-span response.
fn validate_release(
    release: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    validate_exact_release_scope(
        release,
        subject.project_id,
        subject.release,
        subject.environment,
        subject.service_name,
    )
}
