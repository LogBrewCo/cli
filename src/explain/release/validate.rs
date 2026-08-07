//! Fail-closed validation for deployment-aligned release investigation evidence.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::super::time::parse_utc_millis;
use super::super::{
    invalid_response, optional_string, require_bool, require_exact_fields, require_finite_number,
    require_safe_positive_u64, require_safe_u64, require_string, require_string_equals,
    require_timestamp, required_object,
};
use crate::ids::{is_trace_id, is_uuid};
use crate::{ExplainReleaseTarget, RuntimeError};

/// Public cap for SDK/version/stream aggregates.
const SDK_ITEM_LIMIT: usize = 32;
/// Public cap for every release signal collection.
const SIGNAL_ITEM_LIMIT: usize = 20;
/// Public cap for the mixed release timeline.
const TIMELINE_ITEM_LIMIT: usize = 100;
/// Stable base interpretation limits always emitted by release comparison v3.
const BASE_LIMITATIONS: [&str; 3] = [
    "raw_counts_not_rate_normalized",
    "observation_windows_differ",
    "deployment_correlation_not_causation",
];

/// Optional-read availability accepted by release v3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Availability {
    /// The optional read completed with usable evidence.
    Available,
    /// The optional read completed without matching evidence.
    NotFound,
    /// The optional read could not be completed safely.
    Unavailable,
}

/// Validated collection receipt used to recompute evidence coverage.
#[derive(Clone, Copy)]
struct CollectionFacts {
    /// Exact read availability.
    status: Availability,
    /// Whether additional retained rows were omitted by the public cap.
    truncated: bool,
}

/// Exact derived trace health and comparison rate.
#[derive(Clone, Copy)]
struct TraceHealthFacts {
    /// Exact trace-health read availability.
    availability: Availability,
    /// Distinct traces in the exact release scope.
    trace_count: u64,
    /// Distinct traces containing at least one error span.
    error_trace_count: u64,
    /// Derived error-trace rate from zero through ten thousand.
    error_rate_basis_points: u64,
}

impl TraceHealthFacts {
    /// Returns a comparable error rate only for an available nonempty trace population.
    const fn comparable_rate(self) -> Option<u64> {
        if matches!(self.availability, Availability::Available) && self.trace_count > 0 {
            Some(self.error_rate_basis_points)
        } else {
            None
        }
    }
}

/// Exact current release facts reused across comparison checks.
struct SubjectFacts<'a> {
    /// Authorized project identity.
    project_id: &'a str,
    /// Investigated release identity.
    release: &'a str,
    /// Exact deployment environment.
    environment: &'a str,
    /// Exact logical service.
    service_name: &'a str,
    /// Issue, log, span, action, and metric counts in stable order.
    counts: [u64; 5],
    /// First retained telemetry timestamp.
    first_seen_at: &'a str,
    /// Latest retained telemetry timestamp.
    last_seen_at: &'a str,
    /// Current release trace health.
    trace_health: TraceHealthFacts,
}

/// Signal read receipts and derived failure evidence.
struct SignalFacts {
    /// Grouped issue collection receipt.
    issues: CollectionFacts,
    /// Trace collection receipt.
    traces: CollectionFacts,
    /// High-severity log collection receipt.
    logs: CollectionFacts,
    /// Product-action collection receipt.
    actions: CollectionFacts,
    /// Metric collection receipt.
    metrics: CollectionFacts,
    /// Whether retained bounded logs contain error or critical evidence.
    log_failure_observed: bool,
}

/// Validated deployment boundary facts.
struct DeploymentFacts<'a> {
    /// Exact deployed release.
    release: &'a str,
    /// Terminal deployment result.
    status: &'a str,
    /// Parsed deployment start.
    started_millis: i128,
    /// Parsed deployment finish.
    finished_millis: i128,
}

/// Validated previous-release snapshot facts.
struct SnapshotFacts {
    /// Issue, log, span, action, and metric counts in stable order.
    counts: [u64; 5],
    /// Previous release trace health.
    trace_health: TraceHealthFacts,
}

/// Comparison objects and availability needed by the timeline and evidence receipts.
struct ComparisonFacts<'a> {
    /// Exact comparison availability.
    status: Availability,
    /// Captured investigated-release deployment boundary.
    subject_deployment: Option<&'a Map<String, Value>>,
    /// Captured prior successful deployment boundary.
    previous_deployment: Option<&'a Map<String, Value>>,
    /// Retained previous-release aggregate.
    previous_release: Option<&'a Map<String, Value>>,
}

/// One exact derived timeline item used to validate projection completeness and ordering.
struct ExpectedTimelineItem {
    /// Stable timeline item kind.
    kind: &'static str,
    /// Exact wire timestamp.
    occurred_at: String,
    /// Parsed timestamp used for chronological ordering.
    occurred_millis: i128,
    /// Backend-bounded evidence label.
    summary: String,
    /// Exact grouped issue identity when applicable.
    issue_id: Option<String>,
    /// Exact distributed trace identity when applicable.
    trace_id: Option<String>,
}

/// Validates exact schema, identities, signals, deployment comparison, timeline, and receipts.
pub(super) fn validate_response(
    response: &Map<String, Value>,
    expected: &ExplainReleaseTarget,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        response,
        &[
            "schema_version",
            "subject",
            "analysis",
            "sdk_coverage",
            "signals",
            "timeline",
            "comparison",
            "evidence",
            "next_actions",
        ],
    )?;
    if response.get("schema_version").and_then(Value::as_u64) != Some(3) {
        return Err(invalid_response());
    }

    let subject = validate_subject(required_object(response, "subject")?, expected)?;
    let sdk = validate_sdk_coverage(required_object(response, "sdk_coverage")?)?;
    let signals_value = required_object(response, "signals")?;
    let signals = validate_signals(signals_value, &subject)?;
    validate_analysis(required_object(response, "analysis")?, &subject, &signals)?;
    let comparison = validate_comparison(required_object(response, "comparison")?, &subject)?;
    let timeline_truncated = validate_timeline(
        required_object(response, "timeline")?,
        &subject,
        signals_value,
        &comparison,
    )?;
    validate_evidence(
        required_object(response, "evidence")?,
        &subject,
        sdk,
        &signals,
        &comparison,
        timeline_truncated,
    )
}

/// Validates the exact query identity, safe counts, window, and current trace health.
fn validate_subject<'a>(
    subject: &'a Map<String, Value>,
    expected: &ExplainReleaseTarget,
) -> Result<SubjectFacts<'a>, RuntimeError> {
    require_exact_fields(
        subject,
        &[
            "kind",
            "project_id",
            "release",
            "environment",
            "service_name",
            "issue_count",
            "log_count",
            "trace_span_count",
            "action_count",
            "metric_count",
            "first_seen_at",
            "last_seen_at",
            "trace_health_status",
            "trace_health",
        ],
    )?;
    require_string_equals(subject, "kind", "release")?;
    require_string_equals(subject, "project_id", expected.project_id.as_str())?;
    require_string_equals(subject, "release", expected.release.as_str())?;
    require_string_equals(subject, "environment", expected.environment.as_str())?;
    require_string_equals(subject, "service_name", expected.service_name.as_str())?;
    let counts = signal_counts(subject)?;
    let first_seen_at = require_timestamp(subject, "first_seen_at")?;
    let last_seen_at = require_timestamp(subject, "last_seen_at")?;
    if timestamp_millis(first_seen_at)? > timestamp_millis(last_seen_at)? {
        return Err(invalid_response());
    }
    let trace_health = validate_trace_health(
        require_availability(subject, "trace_health_status", false)?,
        required_object(subject, "trace_health")?,
    )?;
    Ok(SubjectFacts {
        project_id: require_string(subject, "project_id")?,
        release: require_string(subject, "release")?,
        environment: require_string(subject, "environment")?,
        service_name: require_string(subject, "service_name")?,
        counts,
        first_seen_at,
        last_seen_at,
        trace_health,
    })
}

/// Reads exact safe signal counts in stable issue/log/span/action/metric order.
fn signal_counts(value: &Map<String, Value>) -> Result<[u64; 5], RuntimeError> {
    Ok([
        require_safe_u64(value, "issue_count")?,
        require_safe_u64(value, "log_count")?,
        require_safe_u64(value, "trace_span_count")?,
        require_safe_u64(value, "action_count")?,
        require_safe_u64(value, "metric_count")?,
    ])
}

/// Validates one derived trace-health object and exact availability semantics.
fn validate_trace_health(
    availability: Availability,
    health: &Map<String, Value>,
) -> Result<TraceHealthFacts, RuntimeError> {
    require_exact_fields(
        health,
        &[
            "status",
            "trace_count",
            "error_trace_count",
            "error_rate_basis_points",
        ],
    )?;
    let trace_count = require_safe_u64(health, "trace_count")?;
    let error_trace_count = require_safe_u64(health, "error_trace_count")?;
    let error_rate_basis_points = require_safe_u64(health, "error_rate_basis_points")?;
    let expected_rate = if trace_count == 0 {
        0
    } else {
        u64::try_from(
            u128::from(error_trace_count)
                .checked_mul(10_000)
                .and_then(|value| value.checked_div(u128::from(trace_count)))
                .ok_or_else(invalid_response)?,
        )
        .map_err(|_error| invalid_response())?
    };
    let expected_status = if trace_count == 0 {
        "unknown"
    } else if error_trace_count == 0 {
        "no_errors_observed"
    } else {
        "errors_observed"
    };
    if error_trace_count > trace_count
        || error_rate_basis_points > 10_000
        || error_rate_basis_points != expected_rate
        || require_string(health, "status")? != expected_status
        || (!matches!(availability, Availability::Available)
            && (trace_count != 0 || error_trace_count != 0))
    {
        return Err(invalid_response());
    }
    Ok(TraceHealthFacts {
        availability,
        trace_count,
        error_trace_count,
        error_rate_basis_points,
    })
}

/// Validates bounded SDK provenance and returns its availability receipt.
fn validate_sdk_coverage(value: &Map<String, Value>) -> Result<CollectionFacts, RuntimeError> {
    require_exact_fields(value, &["status", "items", "truncated"])?;
    let (facts, items) = validate_collection(value, SDK_ITEM_LIMIT)?;
    let mut identities = BTreeSet::new();
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            item,
            &[
                "name",
                "version",
                "stream",
                "item_count",
                "first_seen_at",
                "last_seen_at",
            ],
        )?;
        let identity = (
            require_string(item, "name")?,
            require_string(item, "version")?,
            require_string(item, "stream")?,
        );
        let _count = require_safe_positive_u64(item, "item_count")?;
        validate_ordered_times(item, "first_seen_at", "last_seen_at")?;
        if !identities.insert(identity) {
            return Err(invalid_response());
        }
    }
    Ok(facts)
}

/// Validates every exact release signal item and returns evidence/analysis facts.
fn validate_signals(
    signals: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<SignalFacts, RuntimeError> {
    require_exact_fields(signals, &["issues", "traces", "logs", "actions", "metrics"])?;
    let issues = validate_issues(required_object(signals, "issues")?)?;
    let traces = validate_traces(required_object(signals, "traces")?)?;
    let (logs, log_failure_observed) = validate_logs(required_object(signals, "logs")?)?;
    let actions = validate_actions(required_object(signals, "actions")?)?;
    let metrics = validate_metrics(required_object(signals, "metrics")?)?;
    if subject.counts[3] == 0 && matches!(actions.status, Availability::Available) {
        return Err(invalid_response());
    }
    Ok(SignalFacts {
        issues,
        traces,
        logs,
        actions,
        metrics,
        log_failure_observed,
    })
}

/// Validates grouped issue evidence.
fn validate_issues(value: &Map<String, Value>) -> Result<CollectionFacts, RuntimeError> {
    require_exact_fields(value, &["status", "items", "truncated"])?;
    let (facts, items) = validate_collection(value, SIGNAL_ITEM_LIMIT)?;
    let mut ids = BTreeSet::new();
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            item,
            &[
                "issue_id",
                "severity",
                "title",
                "message",
                "occurrence_count",
                "first_seen_at",
                "last_seen_at",
                "trace_id",
            ],
        )?;
        let id = require_string(item, "issue_id")?;
        if !is_uuid(id) || !ids.insert(id) {
            return Err(invalid_response());
        }
        let _severity = require_string(item, "severity")?;
        let _title = require_string(item, "title")?;
        let _message = require_string(item, "message")?;
        let _count = require_safe_positive_u64(item, "occurrence_count")?;
        validate_ordered_times(item, "first_seen_at", "last_seen_at")?;
        let _trace_id = validate_nullable_trace_id(item, "trace_id")?;
    }
    Ok(facts)
}

/// Validates service-local trace evidence.
fn validate_traces(value: &Map<String, Value>) -> Result<CollectionFacts, RuntimeError> {
    require_exact_fields(value, &["status", "items", "truncated"])?;
    let (facts, items) = validate_collection(value, SIGNAL_ITEM_LIMIT)?;
    let mut ids = BTreeSet::new();
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            item,
            &[
                "trace_id",
                "root_span_name",
                "span_count",
                "error_span_count",
                "started_at",
                "duration_ms",
            ],
        )?;
        let trace_id = require_string(item, "trace_id")?;
        let span_count = require_safe_positive_u64(item, "span_count")?;
        let error_count = require_safe_u64(item, "error_span_count")?;
        if !is_trace_id(trace_id)
            || !ids.insert(trace_id)
            || error_count > span_count
            || item
                .get("duration_ms")
                .and_then(Value::as_i64)
                .is_none_or(|value| value < 0)
        {
            return Err(invalid_response());
        }
        let _name = require_string(item, "root_span_name")?;
        let _started_at = require_timestamp(item, "started_at")?;
    }
    Ok(facts)
}

/// Validates high-severity log evidence and returns whether an error was observed.
fn validate_logs(value: &Map<String, Value>) -> Result<(CollectionFacts, bool), RuntimeError> {
    require_exact_fields(value, &["status", "selection", "items", "truncated"])?;
    require_string_equals(value, "selection", "warning_error_critical")?;
    let (facts, items) = validate_collection(value, SIGNAL_ITEM_LIMIT)?;
    let mut ids = BTreeSet::new();
    let mut failure = false;
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            item,
            &[
                "id",
                "level",
                "source",
                "message",
                "occurred_at",
                "trace_id",
                "span_id",
            ],
        )?;
        let id = require_string(item, "id")?;
        let level = require_string(item, "level")?;
        if !is_uuid(id) || !ids.insert(id) || !matches!(level, "warning" | "error" | "critical") {
            return Err(invalid_response());
        }
        failure |= matches!(level, "error" | "critical");
        let _source = require_string(item, "source")?;
        let _message = require_string(item, "message")?;
        let _occurred_at = require_timestamp(item, "occurred_at")?;
        let trace_id = validate_nullable_trace_id(item, "trace_id")?;
        let span_id = validate_nullable_span_id(item, "span_id")?;
        if span_id.is_some() && trace_id.is_none() {
            return Err(invalid_response());
        }
    }
    Ok((facts, failure))
}

/// Validates typed action aggregates while the parent validator checks exhaustive arithmetic.
fn validate_actions(value: &Map<String, Value>) -> Result<CollectionFacts, RuntimeError> {
    require_exact_fields(value, &["status", "items", "truncated", "estimation"])?;
    let estimation = required_object(value, "estimation")?;
    require_exact_fields(estimation, &["unique_counts_are_approximate", "method"])?;
    if !require_bool(estimation, "unique_counts_are_approximate")?
        || require_string(estimation, "method")? != "approximate_uniq_combined64"
    {
        return Err(invalid_response());
    }
    let (facts, items) = validate_collection(value, SIGNAL_ITEM_LIMIT)?;
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            item,
            &[
                "name",
                "event_count",
                "identified_user_count",
                "anonymous_subject_count",
                "subject_coverage",
                "session_count",
                "first_seen_at",
                "last_seen_at",
                "trace_id",
            ],
        )?;
        let _name = require_string(item, "name")?;
        let _event_count = require_safe_positive_u64(item, "event_count")?;
        let _identified = require_safe_u64(item, "identified_user_count")?;
        let _anonymous = require_safe_u64(item, "anonymous_subject_count")?;
        let _sessions = require_safe_u64(item, "session_count")?;
        validate_ordered_times(item, "first_seen_at", "last_seen_at")?;
        let _trace_id = validate_nullable_trace_id(item, "trace_id")?;
        let coverage = required_object(item, "subject_coverage")?;
        require_exact_fields(
            coverage,
            &[
                "index_version",
                "identified_user_events",
                "anonymous_subject_events",
                "legacy_unknown_kind_events",
                "missing_subject_events",
                "historical_unindexed_events",
            ],
        )?;
    }
    Ok(facts)
}

/// Validates finite attribute-free metric aggregates.
fn validate_metrics(value: &Map<String, Value>) -> Result<CollectionFacts, RuntimeError> {
    require_exact_fields(value, &["status", "items", "truncated"])?;
    let (facts, items) = validate_collection(value, SIGNAL_ITEM_LIMIT)?;
    let mut identities = BTreeSet::new();
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            item,
            &[
                "name",
                "kind",
                "unit",
                "temporality",
                "event_count",
                "minimum_value",
                "maximum_value",
                "average_value",
                "latest_value",
                "latest_at",
                "trace_id",
            ],
        )?;
        let identity = (
            require_string(item, "name")?,
            require_string(item, "kind")?,
            optional_string(item, "unit")?,
            optional_string(item, "temporality")?,
        );
        if !identities.insert(identity) {
            return Err(invalid_response());
        }
        let _events = require_safe_positive_u64(item, "event_count")?;
        let minimum = require_finite_number(item, "minimum_value")?;
        let maximum = require_finite_number(item, "maximum_value")?;
        let average = require_finite_number(item, "average_value")?;
        let latest = require_finite_number(item, "latest_value")?;
        if minimum > maximum
            || !(minimum..=maximum).contains(&average)
            || !(minimum..=maximum).contains(&latest)
        {
            return Err(invalid_response());
        }
        let _latest_at = require_timestamp(item, "latest_at")?;
        let _trace_id = validate_nullable_trace_id(item, "trace_id")?;
    }
    Ok(facts)
}

/// Recomputes the backend's exact noncausal release analysis.
fn validate_analysis(
    analysis: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
    signals: &SignalFacts,
) -> Result<(), RuntimeError> {
    require_exact_fields(analysis, &["status", "causality"])?;
    require_string_equals(analysis, "causality", "evidence_only")?;
    let log_evidence_complete = subject.counts[1] == 0
        || matches!(
            signals.logs.status,
            Availability::Available | Availability::NotFound
        );
    let expected = if subject.counts[0] > 0
        || (matches!(subject.trace_health.availability, Availability::Available)
            && subject.trace_health.error_trace_count > 0)
        || signals.log_failure_observed
    {
        "failures_observed"
    } else if matches!(subject.trace_health.availability, Availability::Available)
        && subject.trace_health.trace_count > 0
        && log_evidence_complete
    {
        "no_failures_observed"
    } else if subject.counts.iter().copied().sum::<u64>() > 0 {
        "telemetry_observed"
    } else {
        "insufficient_evidence"
    };
    require_string_equals(analysis, "status", expected)
}

/// Validates comparison availability, boundaries, snapshot, deltas, direction, and limitations.
fn validate_comparison<'a>(
    comparison: &'a Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<ComparisonFacts<'a>, RuntimeError> {
    require_exact_fields(comparison, &["status", "reason", "details"])?;
    let status = require_availability(comparison, "status", true)?;
    let reason = require_string(comparison, "reason")?;
    let details = required_object(comparison, "details")?;
    require_exact_fields(
        details,
        &[
            "subject_deployment",
            "previous_deployment",
            "previous_release",
            "changes",
            "assessment",
            "limitations",
        ],
    )?;
    let subject_value = optional_object(details, "subject_deployment")?;
    let previous_value = optional_object(details, "previous_deployment")?;
    let snapshot_value = optional_object(details, "previous_release")?;
    let changes_value = optional_object(details, "changes")?;
    let subject_deployment = subject_value
        .map(|value| validate_deployment(value, subject, true))
        .transpose()?;
    let previous_deployment = previous_value
        .map(|value| validate_deployment(value, subject, false))
        .transpose()?;
    if previous_deployment.is_some() && subject_deployment.is_none() {
        return Err(invalid_response());
    }
    if let (Some(current), Some(previous)) = (&subject_deployment, &previous_deployment) {
        if previous.release == current.release
            || previous.status != "succeeded"
            || previous.finished_millis > current.started_millis
        {
            return Err(invalid_response());
        }
    }
    let snapshot = snapshot_value
        .map(|value| {
            let previous = previous_deployment.as_ref().ok_or_else(invalid_response)?;
            validate_snapshot(value, subject, previous)
        })
        .transpose()?;
    let assessment = require_string(details, "assessment")?;

    let shape = u8::from(subject_deployment.is_some())
        | u8::from(previous_deployment.is_some()) << 1
        | u8::from(snapshot.is_some()) << 2
        | u8::from(changes_value.is_some()) << 3;
    if !comparison_shape_matches(status, reason, shape) {
        return Err(invalid_response());
    }

    let (current_rate, previous_rate) =
        if let (Some(changes), Some(snapshot)) = (changes_value, snapshot.as_ref()) {
            let expected_assessment = validate_changes(changes, subject, snapshot)?;
            if assessment != expected_assessment {
                return Err(invalid_response());
            }
            (
                subject.trace_health.comparable_rate(),
                snapshot.trace_health.comparable_rate(),
            )
        } else {
            if assessment != "not_determined" {
                return Err(invalid_response());
            }
            (None, None)
        };
    validate_limitations(
        details,
        status,
        subject_deployment.as_ref(),
        current_rate,
        previous_rate,
    )?;
    Ok(ComparisonFacts {
        status,
        subject_deployment: subject_value,
        previous_deployment: previous_value,
        previous_release: snapshot_value,
    })
}

/// Returns whether one availability/reason pair has the exact nullable evidence shape.
fn comparison_shape_matches(status: Availability, reason: &str, shape: u8) -> bool {
    const SUBJECT: u8 = 1;
    const PREVIOUS: u8 = 1 << 1;
    const SNAPSHOT: u8 = 1 << 2;
    const CHANGES: u8 = 1 << 3;
    match (status, reason) {
        (Availability::Available, "deployment_comparison_available") => {
            shape == (SUBJECT | PREVIOUS | SNAPSHOT | CHANGES)
        }
        (Availability::NotFound, "subject_deployment_not_found") => shape == 0,
        (Availability::NotFound, "previous_successful_deployment_not_found") => shape == SUBJECT,
        (Availability::NotFound, "previous_release_telemetry_not_found")
        | (
            Availability::Unavailable,
            "previous_release_read_unavailable"
            | "comparison_evidence_invalid"
            | "comparison_values_out_of_range",
        ) => shape == (SUBJECT | PREVIOUS),
        (Availability::Unavailable, "deployment_read_unavailable") => matches!(shape, 0 | SUBJECT),
        _ => false,
    }
}

/// Validates one exact deployment boundary and its release scope.
fn validate_deployment<'a>(
    deployment: &'a Map<String, Value>,
    subject: &SubjectFacts<'_>,
    is_subject: bool,
) -> Result<DeploymentFacts<'a>, RuntimeError> {
    require_exact_fields(
        deployment,
        &[
            "id",
            "deployment_id",
            "project_id",
            "release",
            "environment",
            "service_name",
            "status",
            "started_at",
            "finished_at",
            "commit_sha",
        ],
    )?;
    if !is_uuid(require_string(deployment, "id")?) {
        return Err(invalid_response());
    }
    let _deployment_id = require_string(deployment, "deployment_id")?;
    require_string_equals(deployment, "project_id", subject.project_id)?;
    require_string_equals(deployment, "environment", subject.environment)?;
    require_string_equals(deployment, "service_name", subject.service_name)?;
    let release = require_string(deployment, "release")?;
    if is_subject && release != subject.release {
        return Err(invalid_response());
    }
    let status = require_string(deployment, "status")?;
    if !matches!(status, "succeeded" | "failed") || !is_subject && status != "succeeded" {
        return Err(invalid_response());
    }
    let started_at = require_timestamp(deployment, "started_at")?;
    let finished_at = require_timestamp(deployment, "finished_at")?;
    let started_millis = timestamp_millis(started_at)?;
    let finished_millis = timestamp_millis(finished_at)?;
    if started_millis > finished_millis {
        return Err(invalid_response());
    }
    if let Some(commit) = optional_string(deployment, "commit_sha")? {
        if !(7..=64).contains(&commit.len())
            || !commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(invalid_response());
        }
    }
    Ok(DeploymentFacts {
        release,
        status,
        started_millis,
        finished_millis,
    })
}

/// Validates the previous release snapshot against the prior successful deployment scope.
fn validate_snapshot(
    snapshot: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
    deployment: &DeploymentFacts<'_>,
) -> Result<SnapshotFacts, RuntimeError> {
    require_exact_fields(
        snapshot,
        &[
            "release",
            "environment",
            "service_name",
            "issue_count",
            "log_count",
            "trace_span_count",
            "action_count",
            "metric_count",
            "first_seen_at",
            "last_seen_at",
            "trace_health_status",
            "trace_health",
        ],
    )?;
    require_string_equals(snapshot, "release", deployment.release)?;
    require_string_equals(snapshot, "environment", subject.environment)?;
    require_string_equals(snapshot, "service_name", subject.service_name)?;
    let first_seen_at = require_timestamp(snapshot, "first_seen_at")?;
    let last_seen_at = require_timestamp(snapshot, "last_seen_at")?;
    if timestamp_millis(first_seen_at)? > timestamp_millis(last_seen_at)? {
        return Err(invalid_response());
    }
    let trace_health = validate_trace_health(
        require_availability(snapshot, "trace_health_status", false)?,
        required_object(snapshot, "trace_health")?,
    )?;
    Ok(SnapshotFacts {
        counts: signal_counts(snapshot)?,
        trace_health,
    })
}

/// Recomputes exact current-minus-previous count/rate changes and assessment.
fn validate_changes(
    changes: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
    previous: &SnapshotFacts,
) -> Result<&'static str, RuntimeError> {
    require_exact_fields(
        changes,
        &[
            "observed_issue_count_delta",
            "observed_log_count_delta",
            "observed_trace_span_count_delta",
            "observed_action_count_delta",
            "observed_metric_count_delta",
            "current_trace_error_rate_basis_points",
            "previous_trace_error_rate_basis_points",
            "trace_error_rate_delta_basis_points",
        ],
    )?;
    for (index, field) in [
        "observed_issue_count_delta",
        "observed_log_count_delta",
        "observed_trace_span_count_delta",
        "observed_action_count_delta",
        "observed_metric_count_delta",
    ]
    .iter()
    .enumerate()
    {
        let expected = i128::from(subject.counts[index]) - i128::from(previous.counts[index]);
        if changes.get(*field).and_then(Value::as_i64).map(i128::from) != Some(expected) {
            return Err(invalid_response());
        }
    }
    let current = nullable_basis_points(changes, "current_trace_error_rate_basis_points")?;
    let previous_rate = nullable_basis_points(changes, "previous_trace_error_rate_basis_points")?;
    let delta = nullable_i64(changes, "trace_error_rate_delta_basis_points")?;
    let expected_current = subject.trace_health.comparable_rate();
    let expected_previous = previous.trace_health.comparable_rate();
    let expected_delta = expected_current
        .zip(expected_previous)
        .map(|(current, previous)| {
            i64::try_from(current).unwrap_or(0) - i64::try_from(previous).unwrap_or(0)
        });
    if current != expected_current || previous_rate != expected_previous || delta != expected_delta
    {
        return Err(invalid_response());
    }
    Ok(match expected_delta {
        Some(value) if value < 0 => "improved",
        Some(0) => "unchanged",
        Some(_) => "regressed",
        None => "not_determined",
    })
}

/// Validates the exact ordered interpretation limits derived by the backend.
fn validate_limitations(
    details: &Map<String, Value>,
    status: Availability,
    subject: Option<&DeploymentFacts<'_>>,
    current_rate: Option<u64>,
    previous_rate: Option<u64>,
) -> Result<(), RuntimeError> {
    let mut expected = BASE_LIMITATIONS.to_vec();
    if matches!(status, Availability::Available) {
        if current_rate.is_none() {
            expected.push("current_trace_population_unavailable");
        }
        if previous_rate.is_none() {
            expected.push("previous_trace_population_unavailable");
        }
    }
    if subject.is_some_and(|deployment| deployment.status != "succeeded") {
        expected.push("subject_deployment_unsuccessful");
    }
    let actual = details
        .get("limitations")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if actual.len() != expected.len()
        || actual
            .iter()
            .zip(expected)
            .any(|(actual, expected)| actual.as_str() != Some(expected))
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates exact release and deployment boundary timeline items.
fn validate_timeline(
    timeline: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
    signals: &Map<String, Value>,
    comparison: &ComparisonFacts<'_>,
) -> Result<bool, RuntimeError> {
    require_exact_fields(timeline, &["items", "truncated"])?;
    let items = timeline
        .get("items")
        .and_then(Value::as_array)
        .filter(|items| items.len() <= TIMELINE_ITEM_LIMIT)
        .ok_or_else(invalid_response)?;
    let truncated = require_bool(timeline, "truncated")?;
    let (expected, expected_truncated) = expected_timeline(subject, signals, comparison)?;
    if truncated != expected_truncated || items.len() != expected.len() {
        return Err(invalid_response());
    }
    for (item, expected) in items.iter().zip(expected) {
        let item = item.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            item,
            &["kind", "occurred_at", "summary", "issue_id", "trace_id"],
        )?;
        if require_string(item, "kind")? != expected.kind
            || require_timestamp(item, "occurred_at")? != expected.occurred_at
            || require_string(item, "summary")? != expected.summary
            || nullable_text(item, "issue_id")? != expected.issue_id.as_deref()
            || nullable_text(item, "trace_id")? != expected.trace_id.as_deref()
        {
            return Err(invalid_response());
        }
    }
    Ok(truncated)
}

/// Rebuilds the backend's bounded, ordered mixed-signal timeline exactly.
fn expected_timeline(
    subject: &SubjectFacts<'_>,
    signals: &Map<String, Value>,
    comparison: &ComparisonFacts<'_>,
) -> Result<(Vec<ExpectedTimelineItem>, bool), RuntimeError> {
    let mut evidence = signal_timeline(signals)?;
    let mut boundaries = vec![
        expected_item(
            "release_first_seen",
            subject.first_seen_at,
            "release telemetry first observed",
            None,
            None,
        )?,
        expected_item(
            "release_last_seen",
            subject.last_seen_at,
            "release telemetry last observed",
            None,
            None,
        )?,
    ];
    if let Some(previous) = comparison.previous_deployment {
        boundaries.push(expected_item(
            "previous_deployment_finished",
            require_timestamp(previous, "finished_at")?,
            "previous successful deployment finished",
            None,
            None,
        )?);
    }
    if let Some(current) = comparison.subject_deployment {
        boundaries.push(expected_item(
            "subject_deployment_started",
            require_timestamp(current, "started_at")?,
            "subject deployment started",
            None,
            None,
        )?);
        boundaries.push(expected_item(
            "subject_deployment_finished",
            require_timestamp(current, "finished_at")?,
            "subject deployment finished",
            None,
            None,
        )?);
    }
    let evidence_limit = TIMELINE_ITEM_LIMIT.saturating_sub(boundaries.len());
    let truncated = evidence.len() > evidence_limit;
    if truncated {
        evidence = evidence.split_off(evidence.len() - evidence_limit);
    }
    evidence.extend(boundaries);
    sort_timeline(evidence.as_mut_slice());
    Ok((evidence, truncated))
}

/// Projects every bounded signal item into the backend's pre-boundary timeline order.
fn signal_timeline(
    signals: &Map<String, Value>,
) -> Result<Vec<ExpectedTimelineItem>, RuntimeError> {
    let mut evidence = Vec::new();
    for item in collection_items(signals, "issues")? {
        let item = item.as_object().ok_or_else(invalid_response)?;
        evidence.push(expected_item(
            "issue",
            require_timestamp(item, "last_seen_at")?,
            require_string(item, "title")?,
            Some(require_string(item, "issue_id")?),
            validate_nullable_trace_id(item, "trace_id")?,
        )?);
    }
    for item in collection_items(signals, "traces")? {
        let item = item.as_object().ok_or_else(invalid_response)?;
        evidence.push(expected_item(
            "trace",
            require_timestamp(item, "started_at")?,
            require_string(item, "root_span_name")?,
            None,
            Some(require_string(item, "trace_id")?),
        )?);
    }
    for item in collection_items(signals, "logs")? {
        let item = item.as_object().ok_or_else(invalid_response)?;
        evidence.push(expected_item(
            "log",
            require_timestamp(item, "occurred_at")?,
            require_string(item, "message")?,
            None,
            validate_nullable_trace_id(item, "trace_id")?,
        )?);
    }
    for item in collection_items(signals, "actions")? {
        let item = item.as_object().ok_or_else(invalid_response)?;
        evidence.push(expected_item(
            "action",
            require_timestamp(item, "last_seen_at")?,
            require_string(item, "name")?,
            None,
            validate_nullable_trace_id(item, "trace_id")?,
        )?);
    }
    for item in collection_items(signals, "metrics")? {
        let item = item.as_object().ok_or_else(invalid_response)?;
        evidence.push(expected_item(
            "metric",
            require_timestamp(item, "latest_at")?,
            require_string(item, "name")?,
            None,
            validate_nullable_trace_id(item, "trace_id")?,
        )?);
    }
    sort_timeline(evidence.as_mut_slice());
    Ok(evidence)
}

/// Builds one owned expected timeline item from already-validated evidence.
fn expected_item(
    kind: &'static str,
    occurred_at: &str,
    summary: &str,
    issue_id: Option<&str>,
    trace_id: Option<&str>,
) -> Result<ExpectedTimelineItem, RuntimeError> {
    Ok(ExpectedTimelineItem {
        kind,
        occurred_at: occurred_at.to_owned(),
        occurred_millis: timestamp_millis(occurred_at)?,
        summary: bounded_summary(summary),
        issue_id: issue_id.map(str::to_owned),
        trace_id: trace_id.map(str::to_owned),
    })
}

/// Orders timeline items using the backend's stable timestamp/kind/summary keys.
fn sort_timeline(items: &mut [ExpectedTimelineItem]) {
    items.sort_by(|left, right| {
        left.occurred_millis
            .cmp(&right.occurred_millis)
            .then_with(|| timeline_kind_rank(left.kind).cmp(&timeline_kind_rank(right.kind)))
            .then_with(|| left.summary.cmp(&right.summary))
    });
}

/// Returns the backend's stable same-timestamp kind rank.
fn timeline_kind_rank(kind: &str) -> u8 {
    match kind {
        "previous_deployment_finished" => 0,
        "subject_deployment_started" => 1,
        "release_first_seen" => 2,
        "action" => 3,
        "trace" => 4,
        "log" => 5,
        "metric" => 6,
        "issue" => 7,
        "subject_deployment_finished" => 8,
        "release_last_seen" => 9,
        _ => u8::MAX,
    }
}

/// Applies the backend's defensive timeline-summary character boundary.
fn bounded_summary(value: &str) -> String {
    let value = value.trim();
    if value.chars().count() <= 256 {
        value.to_owned()
    } else {
        value.chars().take(256).collect()
    }
}

/// Returns one validated signal collection's item array.
fn collection_items<'a>(
    signals: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a [Value], RuntimeError> {
    required_object(signals, name)?
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(invalid_response)
}

/// Returns one exact string-or-null field without interpreting its contents.
fn nullable_text<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.as_str())),
        _ => Err(invalid_response()),
    }
}

/// Recomputes exact captured/missing/redacted/truncated release evidence arrays.
fn validate_evidence(
    evidence: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
    sdk: CollectionFacts,
    signals: &SignalFacts,
    comparison: &ComparisonFacts<'_>,
    timeline_truncated: bool,
) -> Result<(), RuntimeError> {
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
    let (captured, missing) = expected_availability_fields(subject, sdk, signals, comparison)?;
    let redacted = expected_redacted_fields(subject.counts);
    let truncated = expected_truncated_fields(sdk, signals, timeline_truncated);
    require_exact_string_set(evidence, "captured_fields", &captured)?;
    require_exact_string_set(evidence, "missing_fields", &missing)?;
    require_exact_string_set(evidence, "redacted_fields", &redacted)?;
    require_exact_string_set(evidence, "truncated_fields", &truncated)?;
    let expected_status = if missing.is_empty() && redacted.is_empty() && truncated.is_empty() {
        "complete"
    } else {
        "partial"
    };
    require_string_equals(evidence, "status", expected_status)
}

/// Derives exact captured and missing fields from every optional-read state.
fn expected_availability_fields(
    subject: &SubjectFacts<'_>,
    sdk: CollectionFacts,
    signals: &SignalFacts,
    comparison: &ComparisonFacts<'_>,
) -> Result<(BTreeSet<&'static str>, BTreeSet<&'static str>), RuntimeError> {
    let mut captured = BTreeSet::from([
        "release.identity",
        "release.observed_window",
        "release.signal_counts",
        "release.timeline",
    ]);
    let mut missing = BTreeSet::new();
    record_availability(
        subject.trace_health.availability,
        "release.trace_health",
        true,
        &mut captured,
        &mut missing,
    );
    record_availability(
        sdk.status,
        "release.sdk_coverage",
        true,
        &mut captured,
        &mut missing,
    );
    record_availability(
        signals.issues.status,
        "release.issues",
        subject.counts[0] > 0,
        &mut captured,
        &mut missing,
    );
    record_availability(
        signals.traces.status,
        "release.traces",
        subject.counts[2] > 0,
        &mut captured,
        &mut missing,
    );
    record_availability(
        signals.logs.status,
        "release.logs",
        false,
        &mut captured,
        &mut missing,
    );
    record_availability(
        signals.actions.status,
        "release.actions",
        subject.counts[3] > 0,
        &mut captured,
        &mut missing,
    );
    record_availability(
        signals.actions.status,
        "release.actions.subject_coverage",
        subject.counts[3] > 0,
        &mut captured,
        &mut missing,
    );
    record_availability(
        signals.metrics.status,
        "release.metrics",
        subject.counts[4] > 0,
        &mut captured,
        &mut missing,
    );
    record_availability(
        comparison.status,
        "release.deployment_comparison",
        true,
        &mut captured,
        &mut missing,
    );
    let previous_trace_status = comparison
        .previous_release
        .map(|snapshot| require_availability(snapshot, "trace_health_status", false))
        .transpose()?
        .unwrap_or(Availability::NotFound);
    record_availability(
        previous_trace_status,
        "release.deployment_comparison.previous_trace_health",
        true,
        &mut captured,
        &mut missing,
    );
    Ok((captured, missing))
}

/// Derives exact privacy-redaction receipts from nonzero release signal counts.
fn expected_redacted_fields(counts: [u64; 5]) -> BTreeSet<&'static str> {
    let mut redacted = BTreeSet::new();
    if counts[0] > 0 {
        redacted.extend(["release.issues.attributes", "release.issues.stack_trace"]);
    }
    if counts[2] > 0 {
        let _ = redacted.insert("release.traces.attributes");
    }
    if counts[1] > 0 {
        let _ = redacted.insert("release.logs.attributes");
    }
    if counts[3] > 0 {
        redacted.extend([
            "release.actions.distinct_id",
            "release.actions.properties",
            "release.actions.session_id",
        ]);
    }
    if counts[4] > 0 {
        let _ = redacted.insert("release.metrics.attributes");
    }
    redacted
}

/// Derives exact truncation receipts from collection and timeline caps.
fn expected_truncated_fields(
    sdk: CollectionFacts,
    signals: &SignalFacts,
    timeline_truncated: bool,
) -> BTreeSet<&'static str> {
    let mut truncated = BTreeSet::new();
    for (is_truncated, field) in [
        (sdk.truncated, "release.sdk_coverage"),
        (signals.issues.truncated, "release.issues"),
        (signals.traces.truncated, "release.traces"),
        (signals.logs.truncated, "release.logs"),
        (signals.actions.truncated, "release.actions"),
        (signals.metrics.truncated, "release.metrics"),
        (timeline_truncated, "release.timeline"),
    ] {
        if is_truncated {
            let _ = truncated.insert(field);
        }
    }
    truncated
}

/// Mirrors one backend optional-read evidence classification.
fn record_availability<'a>(
    status: Availability,
    field: &'a str,
    expected_when_empty: bool,
    captured: &mut BTreeSet<&'a str>,
    missing: &mut BTreeSet<&'a str>,
) {
    match status {
        Availability::Available => {
            let _ = captured.insert(field);
        }
        Availability::NotFound if !expected_when_empty => {
            let _ = captured.insert(field);
        }
        Availability::NotFound | Availability::Unavailable => {
            let _ = missing.insert(field);
        }
    }
}

/// Requires one response string array to equal an ordered stable set exactly.
fn require_exact_string_set(
    value: &Map<String, Value>,
    name: &str,
    expected: &BTreeSet<&str>,
) -> Result<(), RuntimeError> {
    let actual = value
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if actual.len() != expected.len()
        || actual
            .iter()
            .zip(expected.iter())
            .any(|(actual, expected)| actual.as_str() != Some(*expected))
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates common bounded collection availability and truncation semantics.
fn validate_collection(
    value: &Map<String, Value>,
    limit: usize,
) -> Result<(CollectionFacts, &[Value]), RuntimeError> {
    let status = require_availability(value, "status", true)?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .filter(|items| items.len() <= limit)
        .map(Vec::as_slice)
        .ok_or_else(invalid_response)?;
    let truncated = require_bool(value, "truncated")?;
    let status_matches = match status {
        Availability::Available => !items.is_empty(),
        Availability::NotFound | Availability::Unavailable => items.is_empty() && !truncated,
    };
    if !status_matches || truncated && items.len() != limit {
        return Err(invalid_response());
    }
    Ok((CollectionFacts { status, truncated }, items))
}

/// Parses one availability field, optionally accepting not-found evidence.
fn require_availability(
    value: &Map<String, Value>,
    name: &str,
    allow_not_found: bool,
) -> Result<Availability, RuntimeError> {
    match require_string(value, name)? {
        "available" => Ok(Availability::Available),
        "not_found" if allow_not_found => Ok(Availability::NotFound),
        "unavailable" => Ok(Availability::Unavailable),
        _ => Err(invalid_response()),
    }
}

/// Returns one exact object-or-null field.
fn optional_object<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a Map<String, Value>>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        _ => Err(invalid_response()),
    }
}

/// Validates two required timestamps and their inclusive ordering.
fn validate_ordered_times(
    value: &Map<String, Value>,
    first: &str,
    last: &str,
) -> Result<(), RuntimeError> {
    let first = timestamp_millis(require_timestamp(value, first)?)?;
    let last = timestamp_millis(require_timestamp(value, last)?)?;
    if first <= last {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

/// Converts one exact UTC response timestamp to an ordered millisecond value.
fn timestamp_millis(value: &str) -> Result<i128, RuntimeError> {
    parse_utc_millis(value).ok_or_else(invalid_response)
}

/// Returns one exact nullable basis-point value.
fn nullable_basis_points(
    value: &Map<String, Value>,
    name: &str,
) -> Result<Option<u64>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= 10_000)
            .map(Some)
            .ok_or_else(invalid_response),
        _ => Err(invalid_response()),
    }
}

/// Returns one exact nullable signed integer.
fn nullable_i64(value: &Map<String, Value>, name: &str) -> Result<Option<i64>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(value) => value.as_i64().map(Some).ok_or_else(invalid_response),
        _ => Err(invalid_response()),
    }
}

/// Validates one exact nullable trace identifier.
fn validate_nullable_trace_id<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if is_trace_id(id) => Ok(Some(id.as_str())),
        _ => Err(invalid_response()),
    }
}

/// Validates one exact nullable W3C span identifier.
fn validate_nullable_span_id<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(id))
            if id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            Ok(Some(id.as_str()))
        }
        _ => Err(invalid_response()),
    }
}
