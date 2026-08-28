//! Bounded human projection for validated release comparison evidence.

use std::fmt::Write as _;

use serde_json::Value;

use super::super::{
    RELATED_PREVIEW_LIMIT, append_collection, append_labeled_bool, append_labeled_integer,
    append_labeled_text, append_string_array,
};

/// Appends comparison availability, boundaries, prior snapshot, exact deltas, and caveats.
pub(in crate::explain) fn comparison(output: &mut String, comparison: Option<&Value>) {
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
    if let Some(deployment) = details.get("subject_deployment") {
        append_deployment(output, "Subject deployment", deployment, false);
    }
    if let Some(deployment) = details.get("previous_deployment") {
        append_deployment(output, "Previous deployment", deployment, true);
    }
    if let Some(previous) = details.get("previous_release") {
        append_previous_release(output, previous);
    }
    if let Some(changes) = details.get("changes") {
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

/// Appends bounded retained SDK release-marker evidence.
pub(in crate::explain) fn markers(output: &mut String, markers: Option<&Value>) {
    let Some(markers) = markers else {
        return;
    };
    let items = append_collection(output, "Release markers", markers);
    append_string_array(
        output,
        "Release marker limits",
        markers.get("limitations"),
        2,
    );
    for marker in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Release marker:");
        append_labeled_text(output, "occurred", marker, "occurred_at", 64);
        append_labeled_text(output, "ingested", marker, "ingested_at", 64);
        append_labeled_text(output, "sdk", marker, "sdk_name", 120);
        append_labeled_text(output, "version", marker, "sdk_version", 80);
        append_labeled_bool(output, "untrusted_telemetry", marker, "untrusted_telemetry");
        for (name, limit) in [("commit", 256), ("notes", 512)] {
            if let Some(field) = marker.get(name) {
                append_labeled_text(
                    output,
                    format!("{name}_status").as_str(),
                    field,
                    "status",
                    32,
                );
                append_labeled_text(output, name, field, "value", limit);
            }
        }
        output.push('\n');
    }
}

/// Appends one current or previous deployment boundary.
fn append_deployment(output: &mut String, label: &str, deployment: &Value, include_release: bool) {
    output.push_str(label);
    output.push(':');
    append_labeled_text(output, "id", deployment, "deployment_id", 128);
    if include_release {
        append_labeled_text(output, "release", deployment, "release", 256);
    }
    append_labeled_text(output, "status", deployment, "status", 32);
    append_labeled_text(output, "started", deployment, "started_at", 64);
    append_labeled_text(output, "finished", deployment, "finished_at", 64);
    append_labeled_text(output, "commit", deployment, "commit_sha", 64);
    output.push('\n');
}

/// Appends the exact prior-release aggregate and observation window.
fn append_previous_release(output: &mut String, previous: &Value) {
    output.push_str("Previous release: ");
    output.push_str(previous["release"].as_str().unwrap_or("unavailable"));
    for (label, name) in [
        ("issues", "issue_count"),
        ("logs", "log_count"),
        ("spans", "trace_span_count"),
        ("actions", "action_count"),
        ("metrics", "metric_count"),
    ] {
        append_labeled_integer(output, label, previous, name);
    }
    output.push('\n');
    output.push_str("Previous observed window:");
    append_labeled_text(output, "first", previous, "first_seen_at", 64);
    append_labeled_text(output, "last", previous, "last_seen_at", 64);
    output.push('\n');
    if let Some(health) = previous.get("trace_health") {
        output.push_str("Previous trace health:");
        append_labeled_text(output, "status", previous, "trace_health_status", 32);
        append_labeled_integer(output, "traces", health, "trace_count");
        append_labeled_integer(output, "error_traces", health, "error_trace_count");
        append_labeled_integer(output, "error_rate_bps", health, "error_rate_basis_points");
        output.push('\n');
    }
}

/// Appends exact signed count and trace-error-rate changes.
fn append_changes(output: &mut String, changes: &Value, assessment: &str) {
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
    let delta = signed(changes, "trace_error_rate_delta_basis_points");
    let _ = writeln!(
        output,
        "Trace error-rate change: current_bps={current} previous_bps={previous} delta_bps={delta} assessment={assessment}"
    );
}

/// Formats one already-validated signed integer with an explicit positive sign.
fn signed(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_i64)
        .map_or_else(|| String::from("unavailable"), |value| format!("{value:+}"))
}

/// Formats one nullable unsigned integer.
fn optional_integer(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_u64)
        .map_or_else(|| String::from("unavailable"), |value| value.to_string())
}
