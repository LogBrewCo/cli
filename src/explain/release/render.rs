//! Bounded human projection for validated release comparison evidence.

use std::fmt::Write as _;

use serde_json::Value;

use super::super::RELATED_PREVIEW_LIMIT;
use super::super::text::{
    append_actions, append_collection, append_evidence, append_issue_previews,
    append_labeled_approximate_integer, append_labeled_bool, append_labeled_integer,
    append_labeled_text, append_log_previews, append_metric_previews, append_named_text,
    append_string_array, append_timeline, field_text,
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

/// Builds a detailed exact service-release investigation.
pub(in crate::explain) fn render_release(value: &Value) -> Option<String> {
    let subject = value.get("subject")?;
    let analysis = value.get("analysis")?;
    let mut output = String::new();
    output.push_str("Release ");
    output.push_str(field_text(subject, "release", 240)?.as_str());
    append_labeled_text(&mut output, "status", analysis, "status", 64);
    append_labeled_text(&mut output, "causality", analysis, "causality", 64);
    append_labeled_text(&mut output, "environment", subject, "environment", 120);
    append_labeled_text(&mut output, "service", subject, "service_name", 160);
    output.push('\n');
    output.push_str("Signals:");
    append_labeled_integer(&mut output, "issues", subject, "issue_count");
    append_labeled_integer(&mut output, "logs", subject, "log_count");
    append_labeled_integer(&mut output, "spans", subject, "trace_span_count");
    append_labeled_integer(&mut output, "actions", subject, "action_count");
    append_labeled_integer(&mut output, "metrics", subject, "metric_count");
    output.push('\n');
    append_named_text(&mut output, "First seen", subject, "first_seen_at", 64);
    append_named_text(&mut output, "Last seen", subject, "last_seen_at", 64);
    output.push_str("Trace health:");
    append_labeled_text(&mut output, "status", subject, "trace_health_status", 64);
    if let Some(health) = subject.get("trace_health") {
        append_labeled_integer(&mut output, "traces", health, "trace_count");
        append_labeled_integer(&mut output, "error_traces", health, "error_trace_count");
        append_labeled_integer(
            &mut output,
            "error_rate_bps",
            health,
            "error_rate_basis_points",
        );
    }
    output.push('\n');
    if let Some(sdk) = value.get("sdk_coverage") {
        let items = append_collection(&mut output, "SDK coverage", sdk);
        for item in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
            output.push_str("SDK:");
            append_labeled_text(&mut output, "name", item, "name", 120);
            append_labeled_text(&mut output, "version", item, "version", 80);
            append_labeled_text(&mut output, "stream", item, "stream", 80);
            append_labeled_integer(&mut output, "items", item, "item_count");
            append_labeled_text(&mut output, "first", item, "first_seen_at", 64);
            append_labeled_text(&mut output, "last", item, "last_seen_at", 64);
            output.push('\n');
        }
    }
    markers(&mut output, value.get("markers"));
    append_release_signals(&mut output, value.get("signals"));
    append_timeline(&mut output, value.get("timeline"));
    comparison(&mut output, value.get("comparison"));
    append_evidence(&mut output, value.get("evidence"));
    append_actions(&mut output, value.get("next_actions"));
    Some(output)
}

/// Appends bounded high-value evidence from every release signal.
fn append_release_signals(output: &mut String, signals: Option<&Value>) {
    let Some(signals) = signals else {
        return;
    };
    for (label, name) in [
        ("Release issues", "issues"),
        ("Release traces", "traces"),
        ("High-severity logs", "logs"),
        ("Release actions", "actions"),
        ("Release metrics", "metrics"),
    ] {
        let Some(collection) = signals.get(name) else {
            continue;
        };
        let items = append_collection(output, label, collection);
        match name {
            "issues" => append_issue_previews(output, items),
            "traces" => {
                for trace in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
                    output.push_str("Trace:");
                    append_labeled_text(output, "name", trace, "root_span_name", 220);
                    append_labeled_integer(output, "spans", trace, "span_count");
                    append_labeled_integer(output, "errors", trace, "error_span_count");
                    append_labeled_integer(output, "duration_ms", trace, "duration_ms");
                    append_labeled_text(output, "trace", trace, "trace_id", 80);
                    output.push('\n');
                }
            }
            "logs" => append_log_previews(output, items),
            "actions" => append_release_action_previews(output, collection, items),
            "metrics" => append_metric_previews(output, items),
            _ => {}
        }
    }
}

/// Appends versioned release action estimates and exact subject-capture coverage.
fn append_release_action_previews(
    output: &mut String,
    collection: &Value,
    items: Option<&[Value]>,
) {
    if let Some(estimation) = collection.get("estimation") {
        output.push_str("Action cardinality:");
        append_labeled_bool(
            output,
            "unique_counts_approximate",
            estimation,
            "unique_counts_are_approximate",
        );
        output.push('\n');
    }
    for action in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Action:");
        append_labeled_text(output, "name", action, "name", 180);
        append_labeled_integer(output, "events", action, "event_count");
        for (label, name) in [
            ("known_users", "identified_user_count"),
            ("anonymous_subjects", "anonymous_subject_count"),
            ("sessions", "session_count"),
        ] {
            append_labeled_approximate_integer(output, label, action, name);
        }
        append_labeled_text(output, "first", action, "first_seen_at", 64);
        append_labeled_text(output, "last", action, "last_seen_at", 64);
        append_labeled_text(output, "trace", action, "trace_id", 80);
        output.push('\n');
        if let Some(coverage) = action.get("subject_coverage") {
            output.push_str("Action subject coverage:");
            for (label, name) in [
                ("index_version", "index_version"),
                ("typed_user_events", "identified_user_events"),
                ("anonymous_events", "anonymous_subject_events"),
                ("legacy_unknown_events", "legacy_unknown_kind_events"),
                ("missing_events", "missing_subject_events"),
                ("historical_unindexed_events", "historical_unindexed_events"),
            ] {
                append_labeled_integer(output, label, coverage, name);
            }
            output.push('\n');
        }
    }
}
