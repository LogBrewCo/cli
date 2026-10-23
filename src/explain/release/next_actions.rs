//! Exact next-action validation for release investigations.

use serde_json::{Map, Value};

use crate::RuntimeError;
use crate::explain::{
    NEXT_ACTION_LIMIT, invalid_response, require_exact_fields, require_string, required_object,
    required_uuid_text,
};
use crate::ids::{is_trace_id, is_uuid};

/// Code, target, reason, issue, and trace for one deterministic release follow-up.
type ReleaseNextActionExpectation<'a> = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'a str>,
    Option<&'a str>,
);

/// Validates the version-3 priority-by-order release action contract and source bindings.
pub(in crate::explain) fn validate_release_next_actions(
    response: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    let expected = expected_release_next_actions(response)?;
    let actions = response
        .get("next_actions")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response)?;
    if actions.len() != expected.len() || actions.len() > NEXT_ACTION_LIMIT {
        return Err(invalid_response());
    }
    for (action, (code, target, reason, issue_id, trace_id)) in actions.iter().zip(expected) {
        let action = action.as_object().ok_or_else(invalid_response)?;
        require_exact_fields(
            action,
            &["code", "target", "reason", "issue_id", "trace_id"],
        )?;
        if require_string(action, "code")? != code
            || require_string(action, "target")? != target
            || require_string(action, "reason")? != reason
            || nullable_uuid(action, "issue_id")? != issue_id
            || nullable_trace_id(action, "trace_id")? != trace_id
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Recomputes the backend's deterministic release follow-up order from returned evidence.
fn expected_release_next_actions(
    response: &Map<String, Value>,
) -> Result<Vec<ReleaseNextActionExpectation<'_>>, RuntimeError> {
    let signals = required_object(response, "signals")?;
    let mut expected = Vec::new();
    if let Some(issue) = release_signal_items(signals, "issues")?.first() {
        let issue = issue.as_object().ok_or_else(invalid_response)?;
        expected.push((
            "inspect_release_issue",
            "issue_investigation",
            "issue_observed",
            Some(required_uuid_text(issue, "issue_id")?),
            nullable_trace_id(issue, "trace_id")?,
        ));
    }
    if let Some(trace) = release_signal_items(signals, "traces")?.first() {
        let trace = trace.as_object().ok_or_else(invalid_response)?;
        expected.push((
            "inspect_release_trace",
            "trace_investigation",
            "trace_observed",
            None,
            Some(required_trace_id(trace, "trace_id")?),
        ));
    }
    for (signal, code, target, reason) in [
        (
            "logs",
            "review_release_logs",
            "telemetry_logs",
            "high_severity_logs_observed",
        ),
        (
            "actions",
            "review_release_actions",
            "telemetry_actions",
            "product_usage_observed",
        ),
        (
            "metrics",
            "review_release_metrics",
            "telemetry_metrics",
            "metric_evidence_observed",
        ),
    ] {
        if !release_signal_items(signals, signal)?.is_empty() {
            expected.push((code, target, reason, None, None));
        }
    }
    let signal_unavailable =
        require_string(required_object(response, "subject")?, "trace_health_status")?
            == "unavailable"
            || require_string(required_object(response, "sdk_coverage")?, "status")?
                == "unavailable"
            || require_string(required_object(response, "markers")?, "status")? == "unavailable"
            || ["issues", "traces", "logs", "actions", "metrics"]
                .iter()
                .any(|name| {
                    required_object(signals, name)
                        .and_then(|signal| require_string(signal, "status"))
                        .is_ok_and(|status| status == "unavailable")
                });
    let comparison = required_object(response, "comparison")?;
    let comparison_status = require_string(comparison, "status")?;
    let comparison_reason = require_string(comparison, "reason")?;
    let previous_trace_unavailable = comparison
        .get("details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("previous_release"))
        .and_then(Value::as_object)
        .and_then(|previous| previous.get("trace_health_status"))
        .and_then(Value::as_str)
        == Some("unavailable");
    if signal_unavailable || comparison_status == "unavailable" || previous_trace_unavailable {
        expected.push((
            "retry_unavailable_evidence",
            "release_investigation",
            "related_evidence_unavailable",
            None,
            None,
        ));
    }
    if matches!(
        comparison_reason,
        "deployment_boundary_not_captured"
            | "subject_deployment_not_found"
            | "previous_successful_deployment_not_found"
    ) {
        expected.push((
            "capture_deployment_boundary",
            "release_instrumentation",
            "comparison_unavailable",
            None,
            None,
        ));
    }
    Ok(expected)
}

/// Returns one already-bounded release signal collection.
fn release_signal_items<'a>(
    signals: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a [Value], RuntimeError> {
    required_object(signals, name)?
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(invalid_response)
}

/// Returns one exact nullable UUID field.
fn nullable_uuid<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if is_uuid(id) => Ok(Some(id.as_str())),
        _ => Err(invalid_response()),
    }
}

/// Returns one required distributed-trace identifier.
fn required_trace_id<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    let id = require_string(value, name)?;
    if is_trace_id(id) {
        Ok(id)
    } else {
        Err(invalid_response())
    }
}

/// Returns one exact nullable distributed-trace identifier.
fn nullable_trace_id<'a>(
    value: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, RuntimeError> {
    match value.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if is_trace_id(id) => Ok(Some(id.as_str())),
        _ => Err(invalid_response()),
    }
}
