//! Shared validation for correlated signals, collections, releases, and deployments.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::contract::{
    is_w3c_id, nullable_w3c_id, optional_object_value, optional_string, require_bool,
    require_exact_fields, require_finite_number, require_safe_positive_u64, require_safe_u64,
    require_string, require_string_equals, require_timestamp, require_timestamp_millis,
    validate_nullable_object, validate_object_array, validate_string_array, validate_strings,
};
use super::invalid_response;
use crate::RuntimeError;
use crate::ids::{is_trace_id, is_uuid};

/// Exact subject scope required by an exact-span trace summary.
pub(super) struct TraceSummaryExpectation<'a> {
    /// Exact containing trace identifier.
    pub(super) trace_id: &'a str,
    /// Service that owns the investigated span.
    pub(super) service_name: &'a str,
    /// Release selected by the investigation.
    pub(super) release: &'a str,
    /// Environment selected by the investigation.
    pub(super) environment: &'a str,
}

/// Validates a bounded trace summary, optionally binding it to one exact-span scope.
pub(super) fn validate_shared_trace_summary(
    value: &Map<String, Value>,
    expected: Option<&TraceSummaryExpectation<'_>>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        value,
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
    let trace_id = require_string(value, "trace_id")?;
    let span_count = require_safe_positive_u64(value, "span_count")?;
    let error_count = require_safe_u64(value, "error_span_count")?;
    let service_count = require_safe_positive_u64(value, "service_count")?;
    let project_count = require_safe_positive_u64(value, "project_count")?;
    if !is_w3c_id(trace_id, 32)
        || span_count > 1_000
        || error_count > span_count
        || expected.is_some_and(|scope| trace_id != scope.trace_id || project_count != 1)
    {
        return Err(invalid_response());
    }
    let _started_at = require_timestamp(value, "started_at")?;
    let _duration_ms = require_safe_u64(value, "duration_ms")?;
    let root = optional_object_value(value.get("root_span"))?;
    if root.is_none() {
        return Err(invalid_response());
    }
    if let Some(root) = root {
        validate_shared_span_summary(root)?;
    }
    if let Some(slowest) = optional_object_value(value.get("slowest_child_span"))? {
        validate_shared_span_summary(slowest)?;
    }
    for (name, errors_only) in [("slowest_path", false), ("error_spans", true)] {
        let spans = value
            .get(name)
            .and_then(Value::as_array)
            .filter(|spans| spans.len() <= 1_000)
            .ok_or_else(invalid_response)?;
        let mut ids = BTreeSet::new();
        for span in spans {
            let span = span.as_object().ok_or_else(invalid_response)?;
            validate_shared_span_summary(span)?;
            if !ids.insert(require_string(span, "span_id")?)
                || errors_only
                    && !optional_string(span, "status")?
                        .is_some_and(|status| status.eq_ignore_ascii_case("error"))
            {
                return Err(invalid_response());
            }
        }
        if errors_only && spans.len() != usize::try_from(error_count.min(5)).unwrap_or(usize::MAX) {
            return Err(invalid_response());
        }
    }
    validate_trace_services(value, expected, span_count, error_count, service_count)
}

/// Validates aggregate service totals and exact release scope for one trace summary.
fn validate_trace_services(
    value: &Map<String, Value>,
    expected: Option<&TraceSummaryExpectation<'_>>,
    span_count: u64,
    error_count: u64,
    service_count: u64,
) -> Result<(), RuntimeError> {
    let services = value
        .get("services")
        .and_then(Value::as_array)
        .filter(|services| services.len() <= 1_000)
        .ok_or_else(invalid_response)?;
    if service_count != u64::try_from(services.len()).map_err(|_error| invalid_response())? {
        return Err(invalid_response());
    }
    let mut totals = (0_u64, 0_u64);
    let mut names = BTreeSet::new();
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
        let spans = require_safe_positive_u64(service, "span_count")?;
        let errors = require_safe_u64(service, "error_span_count")?;
        if names.last().is_some_and(|previous| *previous >= name)
            || !names.insert(name)
            || errors > spans
        {
            return Err(invalid_response());
        }
        let _max_duration_ms = require_safe_u64(service, "max_duration_ms")?;
        totals = (
            totals.0.saturating_add(spans),
            totals.1.saturating_add(errors),
        );
    }
    validate_string_array(value, "releases", 256)?;
    validate_string_array(value, "environments", 256)?;
    if totals != (span_count, error_count)
        || expected.is_some_and(|scope| {
            !names.contains(scope.service_name)
                || value["releases"].as_array().is_none_or(|items| {
                    items.len() != 1 || items[0].as_str() != Some(scope.release)
                })
                || value["environments"].as_array().is_none_or(|items| {
                    items.len() != 1 || items[0].as_str() != Some(scope.environment)
                })
        })
    {
        return Err(invalid_response());
    }
    Ok(())
}

/// Validates one trace-summary span without accepting unversioned additions.
pub(super) fn validate_shared_span_summary(value: &Map<String, Value>) -> Result<(), RuntimeError> {
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
    let _parent_span_id = nullable_w3c_id(value, "parent_span_id", 16)?;
    validate_strings(value, &["name", "operation", "service_name"])?;
    let _status = optional_string(value, "status")?;
    let _started_at = require_timestamp(value, "started_at")?;
    let _duration_ms = require_safe_u64(value, "duration_ms")?;
    Ok(())
}

/// Validates one privacy-bounded correlated signal against an exact deployment scope.
pub(super) fn validate_correlated_signal<'a>(
    value: &'a Map<String, Value>,
    project_id: &str,
    environment: &str,
    release: &str,
    kind: &str,
    excluded_issue_id: Option<&str>,
) -> Result<Option<&'a str>, RuntimeError> {
    let fields: &[&str] = match kind {
        "issue" => &[
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
        "action" => &[
            "id",
            "project_id",
            "name",
            "occurred_at",
            "service_name",
            "environment",
            "release",
        ],
        "metric" => &[
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
        _ => return Err(invalid_response()),
    };
    require_exact_fields(value, fields)?;
    if !is_uuid(require_string(value, "id")?)
        || require_string(value, "project_id")? != project_id
        || require_string(value, "environment")? != environment
        || require_string(value, "release")? != release
    {
        return Err(invalid_response());
    }
    let _service = require_string(value, "service_name")?;
    let _occurred_at = require_timestamp(value, "occurred_at")?;
    let issue_id = match kind {
        "issue" => {
            let issue_id = require_string(value, "issue_id")?;
            if !is_uuid(issue_id)
                || excluded_issue_id == Some(issue_id)
                || !matches!(
                    require_string(value, "severity")?,
                    "info" | "warning" | "error" | "critical"
                )
            {
                return Err(invalid_response());
            }
            validate_strings(value, &["title", "message"])?;
            Some(issue_id)
        }
        "action" => {
            let _name = require_string(value, "name")?;
            None
        }
        "metric" => {
            validate_strings(value, &["name", "kind"])?;
            let _value = require_finite_number(value, "value")?;
            let _unit = optional_string(value, "unit")?;
            let _temporality = optional_string(value, "temporality")?;
            None
        }
        _ => return Err(invalid_response()),
    };
    Ok(issue_id)
}

/// Validates one exact-span, exact-trace, or nearby privacy-bounded log.
pub(super) fn validate_correlated_log(
    value: &Map<String, Value>,
    project_id: &str,
    environment: &str,
    release: &str,
    expected_span_id: Option<&str>,
    relationship: Option<(&str, Option<&str>)>,
) -> Result<(), RuntimeError> {
    let fields: &[&str] = if relationship.is_some() {
        &[
            "id",
            "project_id",
            "severity",
            "source",
            "message",
            "occurred_at",
            "service_name",
            "trace_id",
            "span_id",
            "environment",
            "release",
            "relationship",
        ]
    } else {
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
        ]
    };
    require_exact_fields(value, fields)?;
    if !is_uuid(require_string(value, "id")?)
        || require_string(value, "project_id")? != project_id
        || require_string(value, "environment")? != environment
        || require_string(value, "release")? != release
        || !matches!(
            require_string(value, "severity")?,
            "info" | "warning" | "error" | "critical"
        )
    {
        return Err(invalid_response());
    }
    validate_strings(value, &["source", "message", "service_name"])?;
    let _occurred_at = require_timestamp(value, "occurred_at")?;
    let span_id = nullable_w3c_id(value, "span_id", 16)?;
    if expected_span_id.is_some() && span_id != expected_span_id {
        return Err(invalid_response());
    }
    if let Some((expected_relationship, expected_trace_id)) = relationship {
        let trace_id = nullable_w3c_id(value, "trace_id", 32)?;
        require_string_equals(value, "relationship", expected_relationship)?;
        if expected_relationship == "exact_trace" && trace_id != expected_trace_id {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates one optional-evidence availability field.
pub(super) fn validate_availability<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    let status = require_string(value, name)?;
    if matches!(
        status,
        "available" | "not_linked" | "not_found" | "unavailable"
    ) {
        Ok(status)
    } else {
        Err(invalid_response())
    }
}

/// Validates one bounded, ordered correlation collection through a shared item validator.
pub(super) fn validate_correlated_collection(
    value: &Map<String, Value>,
    limit: usize,
    allow_not_linked: bool,
    mut validate_item: impl FnMut(&Map<String, Value>) -> Result<(), RuntimeError>,
) -> Result<(&str, bool, &[Value]), RuntimeError> {
    require_exact_fields(value, &["status", "items", "truncated"])?;
    let status = validate_availability(value, "status")?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    let truncated = require_bool(value, "truncated")?;
    if items.len() > limit
        || (status == "available") == items.is_empty()
        || (status != "available" && truncated)
        || (truncated && items.len() != limit)
        || (!allow_not_linked && status == "not_linked")
    {
        return Err(invalid_response());
    }
    let mut previous = None;
    for item in items {
        let item = item.as_object().ok_or_else(invalid_response)?;
        validate_item(item)?;
        let order = (
            require_timestamp_millis(item, "occurred_at")?,
            require_string(item, "id")?,
        );
        if previous.is_some_and(|previous| previous >= order) {
            return Err(invalid_response());
        }
        previous = Some(order);
    }
    Ok((status, truncated, items))
}

/// Validates one bounded investigation collection.
pub(super) fn validate_items_collection(
    value: &Map<String, Value>,
    status_required: bool,
) -> Result<&[Value], RuntimeError> {
    if status_required {
        let _status = validate_availability(value, "status")?;
    }
    let items = validate_object_array(value, "items", 1_000)?;
    let _truncated = require_bool(value, "truncated")?;
    Ok(items)
}

/// Validates one exact-trace availability and summary object.
pub(super) fn validate_trace_link(
    value: &Map<String, Value>,
    exact_span: bool,
) -> Result<(), RuntimeError> {
    let _status = validate_availability(value, "status")?;
    if optional_string(value, "trace_id")?.is_some_and(|trace| !is_trace_id(trace)) {
        return Err(invalid_response());
    }
    if exact_span {
        let _span_id = optional_string(value, "span_id")?;
        validate_nullable_object(value, "exact_span")?;
    }
    validate_nullable_object(value, "summary")?;
    let _truncated = require_bool(value, "truncated")?;
    Ok(())
}

/// Validates one project-independent release scope.
pub(super) fn validate_release_scope(value: &Map<String, Value>) -> Result<(), RuntimeError> {
    validate_strings(value, &["release", "environment", "service_name"])
}

/// Binds one exact correlated release scope to its selected subject.
pub(super) fn validate_exact_release_scope(
    value: &Map<String, Value>,
    project_id: &str,
    release: &str,
    environment: &str,
    service_name: &str,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        value,
        &["project_id", "release", "environment", "service_name"],
    )?;
    for (name, expected) in [
        ("project_id", project_id),
        ("release", release),
        ("environment", environment),
        ("service_name", service_name),
    ] {
        require_string_equals(value, name, expected)?;
    }
    Ok(())
}

/// Optional exact scope required of one shared deployment boundary.
#[derive(Clone, Copy, Default)]
pub(super) struct DeploymentExpectation<'a> {
    /// Required owning project when internal identity fields are present.
    pub(super) project_id: Option<&'a str>,
    /// Required deployed release when the caller has exact scope.
    pub(super) release: Option<&'a str>,
    /// Required deployment environment when the caller has exact scope.
    pub(super) environment: Option<&'a str>,
    /// Required logical service when the caller has exact scope.
    pub(super) service_name: Option<&'a str>,
    /// Whether the boundary must represent a successful deployment.
    pub(super) succeeded: bool,
}

/// Strict facts shared by issue, metric, and release deployment evidence.
pub(super) struct DeploymentBoundary<'a> {
    /// Caller-owned external deployment identity.
    pub(super) id: &'a str,
    /// Exact deployed release.
    pub(super) release: &'a str,
    /// Terminal deployment result.
    pub(super) status: &'a str,
    /// Parsed deployment start.
    pub(super) started_millis: i128,
    /// Parsed deployment finish.
    pub(super) finished_millis: i128,
}

/// Validates one canonical completed deployment boundary and optional exact scope.
pub(super) fn validate_deployment_boundary<'a>(
    value: &'a Map<String, Value>,
    expected: DeploymentExpectation<'_>,
) -> Result<DeploymentBoundary<'a>, RuntimeError> {
    let mut fields = vec![
        "deployment_id",
        "release",
        "environment",
        "service_name",
        "status",
        "started_at",
        "finished_at",
        "commit_sha",
    ];
    if expected.project_id.is_some() {
        drop(fields.splice(0..0, ["id", "project_id"]));
    }
    require_exact_fields(value, fields.as_slice())?;
    if expected.project_id.is_some()
        && (!is_uuid(require_string(value, "id")?)
            || optional_string(value, "project_id")? != expected.project_id)
    {
        return Err(invalid_response());
    }
    let id = require_string(value, "deployment_id")?;
    let release = require_string(value, "release")?;
    let environment = require_string(value, "environment")?;
    let service = require_string(value, "service_name")?;
    let status = require_string(value, "status")?;
    let started_millis = require_timestamp_millis(value, "started_at")?;
    let finished_millis = require_timestamp_millis(value, "finished_at")?;
    let commit = optional_string(value, "commit_sha")?;
    let valid = id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
        && matches!(status, "succeeded" | "failed")
        && (!expected.succeeded || status == "succeeded")
        && started_millis <= finished_millis
        && expected.release.is_none_or(|expected| release == expected)
        && expected
            .environment
            .is_none_or(|expected| environment == expected)
        && expected
            .service_name
            .is_none_or(|expected| service == expected)
        && commit.is_none_or(|value| {
            (7..=64).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        });
    valid
        .then_some(DeploymentBoundary {
            id,
            release,
            status,
            started_millis,
            finished_millis,
        })
        .ok_or_else(invalid_response)
}
