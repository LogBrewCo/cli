//! Bounded human projection for validated release comparison evidence.

use std::fmt::Write as _;

use serde_json::Value;

/// Appends comparison availability, boundaries, prior snapshot, exact deltas, and caveats.
pub(super) fn render_comparison(output: &mut String, comparison: Option<&Value>) {
    let Some(comparison) = comparison.and_then(Value::as_object) else {
        return;
    };
    let Some(status) = comparison.get("status").and_then(Value::as_str) else {
        return;
    };
    let Some(reason) = comparison.get("reason").and_then(Value::as_str) else {
        return;
    };
    let details = comparison.get("details").and_then(Value::as_object);
    let assessment = details
        .and_then(|details| details.get("assessment"))
        .and_then(Value::as_str)
        .unwrap_or("not_determined");
    let _ = writeln!(
        output,
        "Comparison: status={status} reason={reason} assessment={assessment}"
    );
    let Some(details) = details else {
        return;
    };
    if let Some(deployment) = details.get("subject_deployment").and_then(Value::as_object) {
        append_subject_deployment(output, deployment);
    }
    if let Some(deployment) = details
        .get("previous_deployment")
        .and_then(Value::as_object)
    {
        append_previous_deployment(output, deployment);
    }
    if let Some(previous) = details.get("previous_release").and_then(Value::as_object) {
        append_previous_release(output, previous);
    }
    if let Some(changes) = details.get("changes").and_then(Value::as_object) {
        append_changes(output, changes, assessment);
    }
    if let Some(limitations) = details.get("limitations").and_then(Value::as_array) {
        output.push_str("Comparison limits:");
        for limitation in limitations.iter().filter_map(Value::as_str) {
            output.push(' ');
            output.push_str(limitation);
        }
        output.push('\n');
    }
    output.push_str(
        "Comparison interpretation: counts are retained raw observations; deployment alignment is correlation evidence, not proof of causation.\n",
    );
}

/// Appends the investigated release deployment boundary.
fn append_subject_deployment(output: &mut String, deployment: &serde_json::Map<String, Value>) {
    let _ = write!(
        output,
        "Subject deployment: id={} status={} started={} finished={}",
        text(deployment, "deployment_id"),
        text(deployment, "status"),
        text(deployment, "started_at"),
        text(deployment, "finished_at")
    );
    if let Some(commit) = deployment.get("commit_sha").and_then(Value::as_str) {
        let _ = write!(output, " commit={commit}");
    }
    output.push('\n');
}

/// Appends the prior successful deployment boundary.
fn append_previous_deployment(output: &mut String, deployment: &serde_json::Map<String, Value>) {
    let _ = write!(
        output,
        "Previous deployment: id={} release={} status={} started={} finished={}",
        text(deployment, "deployment_id"),
        text(deployment, "release"),
        text(deployment, "status"),
        text(deployment, "started_at"),
        text(deployment, "finished_at")
    );
    if let Some(commit) = deployment.get("commit_sha").and_then(Value::as_str) {
        let _ = write!(output, " commit={commit}");
    }
    output.push('\n');
}

/// Appends the exact prior-release aggregate and observation window.
fn append_previous_release(output: &mut String, previous: &serde_json::Map<String, Value>) {
    let _ = writeln!(
        output,
        "Previous release: {} issues={} logs={} spans={} actions={} metrics={}",
        text(previous, "release"),
        integer(previous, "issue_count"),
        integer(previous, "log_count"),
        integer(previous, "trace_span_count"),
        integer(previous, "action_count"),
        integer(previous, "metric_count")
    );
    let _ = writeln!(
        output,
        "Previous observed window: first={} last={}",
        text(previous, "first_seen_at"),
        text(previous, "last_seen_at")
    );
    if let Some(health) = previous.get("trace_health").and_then(Value::as_object) {
        let _ = writeln!(
            output,
            "Previous trace health: status={} traces={} error_traces={} error_rate_bps={}",
            text(previous, "trace_health_status"),
            integer(health, "trace_count"),
            integer(health, "error_trace_count"),
            integer(health, "error_rate_basis_points")
        );
    }
}

/// Appends exact signed count and trace-error-rate changes.
fn append_changes(output: &mut String, changes: &serde_json::Map<String, Value>, assessment: &str) {
    let _ = writeln!(
        output,
        "Observed count change (current - previous): issues={} logs={} spans={} actions={} metrics={}",
        signed(changes, "observed_issue_count_delta"),
        signed(changes, "observed_log_count_delta"),
        signed(changes, "observed_trace_span_count_delta"),
        signed(changes, "observed_action_count_delta"),
        signed(changes, "observed_metric_count_delta")
    );
    let current = optional_integer(changes, "current_trace_error_rate_basis_points");
    let previous = optional_integer(changes, "previous_trace_error_rate_basis_points");
    let delta = optional_signed(changes, "trace_error_rate_delta_basis_points");
    let _ = writeln!(
        output,
        "Trace error-rate change: current_bps={current} previous_bps={previous} delta_bps={delta} assessment={assessment}"
    );
}

/// Returns one already-validated response string.
fn text<'a>(value: &'a serde_json::Map<String, Value>, name: &str) -> &'a str {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("unavailable")
}

/// Returns one already-validated unsigned integer.
fn integer(value: &serde_json::Map<String, Value>, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(0)
}

/// Formats one already-validated signed integer with an explicit positive sign.
fn signed(value: &serde_json::Map<String, Value>, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_i64)
        .map_or_else(|| String::from("unavailable"), |value| format!("{value:+}"))
}

/// Formats one nullable unsigned integer.
fn optional_integer(value: &serde_json::Map<String, Value>, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_u64)
        .map_or_else(|| String::from("unavailable"), |value| value.to_string())
}

/// Formats one nullable signed integer with an explicit positive sign.
fn optional_signed(value: &serde_json::Map<String, Value>, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_i64)
        .map_or_else(|| String::from("unavailable"), |value| format!("{value:+}"))
}
