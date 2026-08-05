//! Validation of selected-branch topology and retained peer baselines.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value};

use super::{
    SubjectFacts, availability, invalid_response, is_error_status, is_w3c_id,
    nullable_nonnegative_number, nullable_string, nullable_w3c_id, require_exact_fields,
    require_safe_u64, require_string, require_timestamp, required_array, required_nullable_object,
};
use crate::RuntimeError;

/// Maximum returned direct child spans.
const CHILD_LIMIT: usize = 50;
/// Maximum returned known ancestors.
const ANCESTOR_LIMIT: usize = 64;
/// Maximum returned cross-service edges.
const EDGE_LIMIT: usize = 50;

/// Validated topology facts needed by analysis, evidence, and next actions.
#[derive(Debug, Clone, Copy)]
pub(super) struct TopologyFacts<'a> {
    /// Optional-evidence status.
    pub(super) status: &'a str,
    /// Direct retained parent identity.
    pub(super) parent_span_id: Option<&'a str>,
    /// First direct child that reported an error.
    pub(super) error_child_span_id: Option<&'a str>,
    /// Whether the topology response was clipped.
    pub(super) truncated: bool,
}

/// Validated peer-baseline facts needed by analysis and next actions.
#[derive(Debug, Clone, Copy)]
pub(super) struct BaselineFacts<'a> {
    /// Optional-evidence status.
    pub(super) status: &'a str,
    /// Whether subject duration meets the returned approximate p95.
    pub(super) at_or_above_p95: bool,
    /// Whether subject duration meets the returned approximate p99.
    pub(super) at_or_above_p99: bool,
}

/// Minimal validated span-summary identity used for relationship checks.
#[derive(Debug, Clone, Copy)]
struct SpanSummaryFacts<'a> {
    /// Span identity.
    span_id: &'a str,
    /// Captured parent identity.
    parent_span_id: Option<&'a str>,
    /// Reported status.
    status: Option<&'a str>,
    /// Logical service.
    service_name: &'a str,
}

/// Validates bounded parent, child, descendant, and service-boundary topology.
pub(super) fn validate_topology<'a>(
    topology: &'a Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<TopologyFacts<'a>, RuntimeError> {
    require_exact_fields(
        topology,
        &[
            "status",
            "root",
            "parent",
            "ancestors",
            "children",
            "descendant_count",
            "cross_service_edges",
            "parent_chain_status",
            "truncated",
        ],
    )?;
    let status = availability(topology, "status")?;
    let root = required_nullable_object(topology.get("root"))?;
    let parent = required_nullable_object(topology.get("parent"))?;
    let ancestor_values = required_array(topology, "ancestors", ANCESTOR_LIMIT)?;
    let child_values = required_array(topology, "children", CHILD_LIMIT)?;
    let descendant_count = require_safe_u64(topology, "descendant_count")?;
    let edge_values = required_array(topology, "cross_service_edges", EDGE_LIMIT)?;
    let chain_status = require_string(topology, "parent_chain_status")?;
    if !matches!(
        chain_status,
        "root" | "complete" | "missing" | "cycle" | "truncated" | "unavailable"
    ) {
        return Err(invalid_response());
    }
    let truncated = super::require_bool(topology, "truncated")?;
    let root_facts = root.map(validate_span_summary).transpose()?;
    let parent_facts = parent.map(validate_span_summary).transpose()?;
    let ancestors = ancestor_values
        .iter()
        .map(validate_span_summary_value)
        .collect::<Result<Vec<_>, _>>()?;
    let children = child_values
        .iter()
        .map(validate_span_summary_value)
        .collect::<Result<Vec<_>, _>>()?;
    validate_unique_topology_ids(&ancestors, &children, subject.span_id)?;
    if status == "available" {
        validate_ancestor_chain(&ancestors, parent_facts, subject.parent_span_id)?;
    }
    if children
        .iter()
        .any(|child| child.parent_span_id != Some(subject.span_id))
        || descendant_count < u64::try_from(children.len()).map_err(|_error| invalid_response())?
    {
        return Err(invalid_response());
    }
    validate_service_edges(edge_values, subject, &ancestors, &children)?;
    validate_topology_state(
        status,
        root,
        root_facts,
        parent,
        parent_facts,
        &ancestors,
        &children,
        descendant_count,
        edge_values,
        chain_status,
        truncated,
        subject,
    )?;
    Ok(TopologyFacts {
        status,
        parent_span_id: parent_facts.map(|parent| parent.span_id),
        error_child_span_id: children
            .iter()
            .find(|child| is_error_status(child.status))
            .map(|child| child.span_id),
        truncated,
    })
}

/// Validates deterministic baseline dimensions, arithmetic, availability, and limitations.
#[expect(
    clippy::too_many_lines,
    reason = "the baseline validator keeps the serialized dimensions, arithmetic, method, and availability state machine together"
)]
pub(super) fn validate_baseline<'a>(
    baseline: &'a Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<BaselineFacts<'a>, RuntimeError> {
    require_exact_fields(
        baseline,
        &[
            "status",
            "since",
            "until",
            "dimensions",
            "retained_peer_count",
            "error_peer_count",
            "error_rate_basis_points",
            "p50_duration_ms",
            "p95_duration_ms",
            "p99_duration_ms",
            "subject_percentile_basis_points",
            "method",
            "limitations",
        ],
    )?;
    let status = availability(baseline, "status")?;
    let since = require_timestamp(baseline, "since")?;
    let until = require_timestamp(baseline, "until")?;
    if until != subject.started_at || since >= until {
        return Err(invalid_response());
    }
    let dimensions = super::required_object(baseline, "dimensions")?;
    require_exact_fields(
        dimensions,
        &[
            "project_id",
            "environment",
            "release",
            "service_name",
            "operation",
            "name",
        ],
    )?;
    for (field, expected) in [
        ("project_id", subject.project_id),
        ("environment", subject.environment),
        ("release", subject.release),
        ("service_name", subject.service_name),
        ("operation", subject.operation),
        ("name", subject.name),
    ] {
        if require_string(dimensions, field)? != expected {
            return Err(invalid_response());
        }
    }
    let retained = require_safe_u64(baseline, "retained_peer_count")?;
    let errors = require_safe_u64(baseline, "error_peer_count")?;
    let error_rate = require_safe_u64(baseline, "error_rate_basis_points")?;
    let expected_rate = if retained == 0 {
        0
    } else {
        errors.saturating_mul(10_000) / retained
    };
    if errors > retained || error_rate > 10_000 || error_rate != expected_rate {
        return Err(invalid_response());
    }
    let p50 = nullable_nonnegative_number(baseline, "p50_duration_ms")?;
    let p95 = nullable_nonnegative_number(baseline, "p95_duration_ms")?;
    let p99 = nullable_nonnegative_number(baseline, "p99_duration_ms")?;
    let percentile = super::nullable_safe_u64(baseline, "subject_percentile_basis_points")?;
    if percentile.is_some_and(|value| value > 10_000)
        || matches!((p50, p95, p99), (Some(p50), Some(p95), Some(p99)) if p50 > p95 || p95 > p99)
    {
        return Err(invalid_response());
    }
    if require_string(baseline, "method")? != "approximate_t_digest" {
        return Err(invalid_response());
    }
    let limitations = baseline
        .get("limitations")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    let expected_limitations = [
        "retained_telemetry_only",
        "sampling_may_apply",
        "approximate_percentiles",
        "same_release_only",
    ];
    if limitations.len() != expected_limitations.len()
        || limitations
            .iter()
            .zip(expected_limitations)
            .any(|(actual, expected)| actual.as_str() != Some(expected))
    {
        return Err(invalid_response());
    }
    let values_present = p50.is_some() && p95.is_some() && p99.is_some() && percentile.is_some();
    match status {
        "available" if retained > 0 && values_present => {}
        "not_found" | "unavailable"
            if retained == 0
                && errors == 0
                && error_rate == 0
                && p50.is_none()
                && p95.is_none()
                && p99.is_none()
                && percentile.is_none() => {}
        _ => return Err(invalid_response()),
    }
    let duration = safe_u64_as_f64(subject.duration_ms)?;
    Ok(BaselineFacts {
        status,
        at_or_above_p95: p95.is_some_and(|p95| duration >= p95),
        at_or_above_p99: p99.is_some_and(|p99| duration >= p99),
    })
}

/// Validates one exact shared span summary.
fn validate_span_summary(value: &Map<String, Value>) -> Result<SpanSummaryFacts<'_>, RuntimeError> {
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
    let span_id = require_string(value, "span_id")?;
    if !is_w3c_id(span_id, 16) {
        return Err(invalid_response());
    }
    let parent_span_id = nullable_w3c_id(value, "parent_span_id", 16)?;
    let _name = require_string(value, "name")?;
    let _operation = require_string(value, "operation")?;
    let status = nullable_string(value, "status")?;
    let _started_at = require_timestamp(value, "started_at")?;
    let _duration = require_safe_u64(value, "duration_ms")?;
    Ok(SpanSummaryFacts {
        span_id,
        parent_span_id,
        status,
        service_name: require_string(value, "service_name")?,
    })
}

/// Validates a span summary represented as one array item.
fn validate_span_summary_value(value: &Value) -> Result<SpanSummaryFacts<'_>, RuntimeError> {
    value
        .as_object()
        .ok_or_else(invalid_response)
        .and_then(validate_span_summary)
}

/// Rejects duplicate span identities and any accidental subject duplication.
fn validate_unique_topology_ids(
    ancestors: &[SpanSummaryFacts<'_>],
    children: &[SpanSummaryFacts<'_>],
    subject_span_id: &str,
) -> Result<(), RuntimeError> {
    let mut seen = BTreeSet::new();
    for span in ancestors.iter().chain(children) {
        if span.span_id == subject_span_id || !seen.insert(span.span_id) {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates root-to-parent chain continuity and exact direct-parent identity.
fn validate_ancestor_chain(
    ancestors: &[SpanSummaryFacts<'_>],
    parent: Option<SpanSummaryFacts<'_>>,
    expected_parent: Option<&str>,
) -> Result<(), RuntimeError> {
    if parent.map(|parent| parent.span_id) != expected_parent
        || ancestors.last().map(|ancestor| ancestor.span_id) != parent.map(|parent| parent.span_id)
    {
        return Err(invalid_response());
    }
    if ancestors
        .windows(2)
        .any(|pair| pair[1].parent_span_id != Some(pair[0].span_id))
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates availability-dependent topology emptiness and parent-chain semantics.
#[expect(
    clippy::too_many_arguments,
    reason = "the strict topology state machine compares each independently serialized receipt"
)]
fn validate_topology_state(
    status: &str,
    root: Option<&Map<String, Value>>,
    root_facts: Option<SpanSummaryFacts<'_>>,
    parent: Option<&Map<String, Value>>,
    parent_facts: Option<SpanSummaryFacts<'_>>,
    ancestors: &[SpanSummaryFacts<'_>],
    children: &[SpanSummaryFacts<'_>],
    descendant_count: u64,
    edges: &[Value],
    chain_status: &str,
    truncated: bool,
    subject: &SubjectFacts<'_>,
) -> Result<(), RuntimeError> {
    match status {
        "unavailable" => {
            if root.is_some()
                || parent.is_some()
                || !ancestors.is_empty()
                || !children.is_empty()
                || descendant_count != 0
                || !edges.is_empty()
                || chain_status != "unavailable"
                || truncated
            {
                return Err(invalid_response());
            }
        }
        "not_found" => {
            let expected_chain = if subject.parent_span_id.is_some() {
                "missing"
            } else {
                "root"
            };
            if root.is_some()
                || parent.is_some()
                || !ancestors.is_empty()
                || !children.is_empty()
                || descendant_count != 0
                || !edges.is_empty()
                || chain_status != expected_chain
                || truncated
            {
                return Err(invalid_response());
            }
        }
        "available" => match chain_status {
            "root" => {
                if subject.parent_span_id.is_some()
                    || parent.is_some()
                    || !ancestors.is_empty()
                    || root.is_none()
                    || !root_is_subject(root.ok_or_else(invalid_response)?, subject)?
                {
                    return Err(invalid_response());
                }
            }
            "complete" => {
                if subject.parent_span_id.is_none()
                    || ancestors.is_empty()
                    || ancestors
                        .first()
                        .is_some_and(|span| span.parent_span_id.is_some())
                    || root != ancestor_object(ancestors.first(), root)
                    || parent_facts.is_none()
                {
                    return Err(invalid_response());
                }
            }
            "missing" | "truncated" => {
                if subject.parent_span_id.is_none()
                    || ancestors.is_empty()
                    || root_facts.map(|root| root.span_id)
                        != ancestors.first().map(|ancestor| ancestor.span_id)
                    || parent_facts.is_none()
                {
                    return Err(invalid_response());
                }
            }
            "cycle" if root.is_none() && truncated => {}
            _ => return Err(invalid_response()),
        },
        _ => return Err(invalid_response()),
    }
    if matches!(chain_status, "cycle" | "truncated") && !truncated {
        return Err(invalid_response());
    }
    Ok(())
}

/// Returns the root object when it matches the first validated ancestor identity.
fn ancestor_object<'a>(
    ancestor: Option<&SpanSummaryFacts<'_>>,
    root: Option<&'a Map<String, Value>>,
) -> Option<&'a Map<String, Value>> {
    root.filter(|root| {
        root.get("span_id").and_then(Value::as_str) == ancestor.map(|ancestor| ancestor.span_id)
    })
}

/// Validates that the retained root summary is exactly the selected root span.
fn root_is_subject(
    root: &Map<String, Value>,
    subject: &SubjectFacts<'_>,
) -> Result<bool, RuntimeError> {
    Ok(require_string(root, "span_id")? == subject.span_id
        && nullable_string(root, "parent_span_id")?.is_none()
        && require_string(root, "name")? == subject.name
        && require_string(root, "operation")? == subject.operation
        && nullable_string(root, "status")? == subject.status
        && require_timestamp(root, "started_at")? == subject.started_at
        && require_safe_u64(root, "duration_ms")? == subject.duration_ms
        && require_string(root, "service_name")? == subject.service_name)
}

/// Validates sorted unique cross-service edges and known-endpoint service bindings.
fn validate_service_edges(
    values: &[Value],
    subject: &SubjectFacts<'_>,
    ancestors: &[SpanSummaryFacts<'_>],
    children: &[SpanSummaryFacts<'_>],
) -> Result<(), RuntimeError> {
    let services = ancestors
        .iter()
        .chain(children)
        .map(|span| (span.span_id, span.service_name))
        .chain(std::iter::once((subject.span_id, subject.service_name)))
        .collect::<HashMap<_, _>>();
    let mut previous: Option<(&str, &str, &str, &str)> = None;
    for value in values {
        let edge = value.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            edge,
            &["from_span_id", "to_span_id", "from_service", "to_service"],
        )?;
        let tuple = (
            require_string(edge, "from_span_id")?,
            require_string(edge, "to_span_id")?,
            require_string(edge, "from_service")?,
            require_string(edge, "to_service")?,
        );
        if !is_w3c_id(tuple.0, 16)
            || !is_w3c_id(tuple.1, 16)
            || tuple.0 == tuple.1
            || tuple.2 == tuple.3
            || previous.is_some_and(|previous| previous >= tuple)
            || services
                .get(tuple.0)
                .is_some_and(|service| *service != tuple.2)
            || services
                .get(tuple.1)
                .is_some_and(|service| *service != tuple.3)
        {
            return Err(invalid_response());
        }
        previous = Some(tuple);
    }
    Ok(())
}

/// Converts one safe JSON integer without introducing an unchecked lossy cast.
fn safe_u64_as_f64(value: u64) -> Result<f64, RuntimeError> {
    value
        .to_string()
        .parse::<f64>()
        .map_err(|_error| invalid_response())
}
