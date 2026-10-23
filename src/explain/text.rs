//! Bounded human text helpers shared by explanation renderers.

use serde_json::Value;

use super::issue::render::{append_issue_cause, append_issue_fix};
use super::{NEXT_ACTION_LIMIT, RELATED_PREVIEW_LIMIT};
use crate::http::terminal_safe;

/// Appends strict shared runtime context when available.
pub(super) fn append_runtime_context(output: &mut String, context: Option<&Value>) {
    let Some(context) = context.filter(|value| !value.is_null()) else {
        return;
    };
    if let Some(resource) = context.get("resource").filter(|value| !value.is_null()) {
        let mut runtime = String::from("Runtime:");
        for (label, name) in [
            ("service", "service"),
            ("runtime", "runtime"),
            ("framework", "framework"),
            ("os", "operating_system"),
            ("app", "application"),
        ] {
            if let Some(identity) = resource.get(name).filter(|value| !value.is_null()) {
                append_named_version_label(&mut runtime, label, identity);
            }
        }
        if let Some(deployment) = resource.get("deployment").filter(|value| !value.is_null()) {
            append_labeled_text(&mut runtime, "environment", deployment, "environment", 120);
            append_labeled_text(&mut runtime, "release", deployment, "release", 200);
        }
        if let Some(device) = resource.get("device").filter(|value| !value.is_null()) {
            append_labeled_text(&mut runtime, "device_family", device, "family", 120);
            append_labeled_text(&mut runtime, "device_model", device, "model", 120);
            append_labeled_text(&mut runtime, "architecture", device, "architecture", 80);
        }
        if runtime != "Runtime:" {
            runtime.push('\n');
            output.push_str(runtime.as_str());
        }
    }
    if let Some(trace) = context.get("trace").filter(|value| !value.is_null()) {
        output.push_str("Captured correlation:");
        append_labeled_text(output, "trace", trace, "trace_id", 80);
        append_labeled_text(output, "span", trace, "span_id", 40);
        append_labeled_text(output, "parent", trace, "parent_span_id", 40);
        append_labeled_bool(output, "sampled", trace, "sampled");
        output.push('\n');
    }
    if let Some(session) = context
        .get("session")
        .filter(|value| !value.is_null())
        .filter(|session| {
            session.get("id").and_then(Value::as_str).is_some()
                || session.get("previous_id").and_then(Value::as_str).is_some()
        })
    {
        output.push_str("Session correlation:");
        append_labeled_text(output, "id", session, "id", 160);
        append_labeled_text(output, "previous", session, "previous_id", 160);
        output.push('\n');
    }
    if let Some(subject) = context.get("subject").filter(|value| !value.is_null()) {
        output.push_str("Subject correlation:");
        append_labeled_text(output, "kind", subject, "kind", 40);
        append_labeled_text(output, "id", subject, "id", 160);
        output.push('\n');
    }
    if let Some(tags) = context.get("tags").and_then(Value::as_object) {
        for (key, value) in tags.iter().take(8) {
            let Some(value) = value.as_str() else {
                continue;
            };
            output.push_str("Tag: ");
            output.push_str(display_text(key, 120).as_str());
            output.push('=');
            output.push_str(display_text(value, 200).as_str());
            output.push('\n');
        }
        if tags.len() > 8 {
            output.push_str("Tags omitted from human view: ");
            output.push_str((tags.len() - 8).to_string().as_str());
            output.push_str("; use --json for all retained tags.\n");
        }
    }
}

/// Appends one named/versioned runtime identity to a compact line.
fn append_named_version_label(output: &mut String, label: &str, value: &Value) {
    let Some(name) = field_text(value, "name", 120) else {
        return;
    };
    output.push(' ');
    output.push_str(label);
    output.push('=');
    output.push_str(name.as_str());
    if let Some(version) = field_text(value, "version", 120) {
        output.push('@');
        output.push_str(version.as_str());
    }
}

/// Collects safe scalar leaves from the server-bounded attribute projection.
pub(super) fn collect_scalar_fields(
    value: &Value,
    prefix: &str,
    fields: &mut Vec<(String, String)>,
) {
    if fields.len() >= 8 {
        return;
    }
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (key, child) in entries {
                let path = if prefix.is_empty() {
                    terminal_safe(key)
                } else {
                    format!("{prefix}.{}", terminal_safe(key))
                };
                collect_scalar_fields(child, path.as_str(), fields);
                if fields.len() >= 8 {
                    break;
                }
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                let path = format!("{prefix}[{index}]");
                collect_scalar_fields(child, path.as_str(), fields);
                if fields.len() >= 8 {
                    break;
                }
            }
        }
        Value::Null => fields.push((prefix.to_owned(), String::from("null"))),
        Value::Bool(value) => fields.push((prefix.to_owned(), value.to_string())),
        Value::Number(value) => fields.push((prefix.to_owned(), value.to_string())),
        Value::String(value) => fields.push((prefix.to_owned(), display_text(value, 300))),
    }
}

/// Appends one mixed-signal timeline receipt and representative ordered items.
pub(super) fn append_timeline(output: &mut String, timeline: Option<&Value>) {
    let Some(timeline) = timeline else {
        return;
    };
    let items = timeline.get("items").and_then(Value::as_array);
    output.push_str("Timeline: count=");
    output.push_str(items.map_or(0, Vec::len).to_string().as_str());
    append_labeled_bool(output, "truncated", timeline, "truncated");
    output.push('\n');
    for item in items.into_iter().flatten().take(5) {
        output.push_str("Timeline item:");
        append_labeled_text(output, "at", item, "occurred_at", 64);
        append_labeled_text(output, "kind", item, "kind", 64);
        append_labeled_text(output, "relationship", item, "relationship", 80);
        append_labeled_text(output, "service", item, "service_name", 160);
        append_labeled_text(output, "name", item, "name", 220);
        append_labeled_text(output, "summary", item, "summary", 300);
        append_labeled_text(output, "message", item, "message", 300);
        append_labeled_text(output, "severity", item, "severity", 32);
        append_labeled_text(output, "status", item, "status", 40);
        append_labeled_integer(output, "duration_ms", item, "duration_ms");
        append_labeled_text(output, "issue", item, "issue_id", 80);
        append_labeled_text(output, "trace", item, "trace_id", 80);
        append_labeled_text(output, "span", item, "span_id", 40);
        output.push('\n');
    }
}

/// Appends availability, count, and truncation for one related collection.
pub(super) fn append_collection<'a>(
    output: &mut String,
    label: &str,
    collection: &'a Value,
) -> Option<&'a [Value]> {
    output.push_str(label);
    output.push(':');
    append_labeled_text(output, "status", collection, "status", 40);
    let items = collection.get("items").and_then(Value::as_array);
    output.push_str(" count=");
    output.push_str(items.map_or(0, Vec::len).to_string().as_str());
    append_labeled_integer(output, "total", collection, "total");
    append_labeled_bool(output, "truncated", collection, "truncated");
    output.push('\n');
    items.map(Vec::as_slice)
}

/// Appends named correlation receipts and their type-specific safe previews.
pub(super) fn append_related_collections(
    output: &mut String,
    correlations: &Value,
    collections: &[(&str, &str)],
) {
    for (name, label) in collections {
        let Some(collection) = correlations.get(name) else {
            continue;
        };
        let items = append_collection(output, label, collection);
        match *name {
            "issues" | "related_issues" => append_issue_previews(output, items),
            "logs" | "trace_logs" | "nearby_logs" => append_log_previews(output, items),
            "actions" => append_action_previews(output, items),
            "metrics" => append_metric_previews(output, items),
            _ => {}
        }
    }
}

/// Appends issue evidence previews without raw attributes.
pub(super) fn append_issue_previews(output: &mut String, items: Option<&[Value]>) {
    for issue in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Issue:");
        append_labeled_text(output, "title", issue, "title", 260);
        append_labeled_text(output, "severity", issue, "severity", 32);
        append_labeled_text(output, "status", issue, "status", 32);
        append_labeled_integer(output, "occurrences", issue, "occurrence_count");
        append_labeled_text(output, "issue", issue, "issue_id", 80);
        append_labeled_text(output, "trace", issue, "trace_id", 80);
        append_labeled_text(output, "span", issue, "span_id", 40);
        append_labeled_text(output, "at", issue, "occurred_at", 64);
        output.push('\n');
        append_issue_cause(output, issue.get("cause"));
        append_issue_fix(output, issue.get("fix"));
    }
}

/// Appends structured-log evidence previews without arbitrary attributes.
pub(super) fn append_log_previews(output: &mut String, items: Option<&[Value]>) {
    for log in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Log:");
        append_labeled_text(output, "message", log, "message", 300);
        append_labeled_text(output, "severity", log, "severity", 32);
        append_labeled_text(output, "level", log, "level", 32);
        append_labeled_text(output, "source", log, "source", 100);
        append_labeled_text(output, "service", log, "service_name", 160);
        append_labeled_text(output, "span", log, "span_id", 40);
        append_labeled_text(output, "at", log, "occurred_at", 64);
        output.push('\n');
    }
}

/// Appends product-action evidence previews.
fn append_action_previews(output: &mut String, items: Option<&[Value]>) {
    for action in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Action:");
        append_labeled_text(output, "name", action, "name", 180);
        append_labeled_text(output, "service", action, "service_name", 160);
        append_labeled_integer(output, "events", action, "event_count");
        append_labeled_integer(output, "users", action, "identified_user_count");
        append_labeled_integer(output, "sessions", action, "session_count");
        append_labeled_text(output, "span", action, "span_id", 40);
        append_labeled_text(output, "at", action, "occurred_at", 64);
        output.push('\n');
    }
}

/// Appends metric exemplar or release metric evidence without anomaly claims.
pub(super) fn append_metric_previews(output: &mut String, items: Option<&[Value]>) {
    for metric in items.into_iter().flatten().take(RELATED_PREVIEW_LIMIT) {
        output.push_str("Metric:");
        append_labeled_text(output, "name", metric, "name", 200);
        append_labeled_text(output, "kind", metric, "kind", 80);
        append_labeled_text(output, "temporality", metric, "temporality", 40);
        append_labeled_number(output, "value", metric, "value");
        append_labeled_number(output, "latest", metric, "latest_value");
        append_labeled_number(output, "min", metric, "minimum_value");
        append_labeled_number(output, "max", metric, "maximum_value");
        append_labeled_number(output, "average", metric, "average_value");
        append_labeled_integer(output, "events", metric, "event_count");
        append_labeled_text(output, "unit", metric, "unit", 80);
        append_labeled_text(output, "service", metric, "service_name", 160);
        append_labeled_text(output, "at", metric, "occurred_at", 64);
        append_labeled_text(output, "latest_at", metric, "latest_at", 64);
        append_labeled_text(output, "trace", metric, "trace_id", 80);
        append_labeled_text(output, "span", metric, "span_id", 40);
        output.push('\n');
    }
}

/// Appends explicit evidence completeness and omission identifiers.
pub(super) fn append_evidence(output: &mut String, evidence: Option<&Value>) {
    let Some(evidence) = evidence else {
        return;
    };
    output.push_str("Evidence:");
    append_labeled_text(output, "status", evidence, "status", 32);
    for (label, name) in [
        ("captured", "captured_fields"),
        ("missing", "missing_fields"),
        ("redacted", "redacted_fields"),
        ("truncated", "truncated_fields"),
    ] {
        if let Some(fields) = evidence.get(name).and_then(Value::as_array) {
            output.push(' ');
            output.push_str(label);
            output.push('=');
            output.push_str(fields.len().to_string().as_str());
        }
    }
    output.push('\n');
    append_string_array(output, "Missing", evidence.get("missing_fields"), 8);
    append_string_array(output, "Redacted", evidence.get("redacted_fields"), 8);
    append_string_array(output, "Truncated", evidence.get("truncated_fields"), 8);
}

/// Appends prioritized backend-generated next actions and reasons.
pub(super) fn append_actions(output: &mut String, actions: Option<&Value>) {
    let Some(actions) = actions.and_then(Value::as_array) else {
        return;
    };
    for (index, action) in actions.iter().take(NEXT_ACTION_LIMIT).enumerate() {
        output.push_str("Next");
        output.push(' ');
        output.push_str(
            integer_text(action, "priority")
                .unwrap_or_else(|| index.saturating_add(1).to_string())
                .as_str(),
        );
        output.push(':');
        append_labeled_text(output, "code", action, "code", 100);
        append_labeled_text(output, "target", action, "target", 100);
        append_labeled_text(output, "reason", action, "reason", 500);
        output.push('\n');
    }
}

/// Appends one bounded string array on a named line.
pub(super) fn append_string_array(
    output: &mut String,
    label: &str,
    value: Option<&Value>,
    limit: usize,
) {
    let Some(values) = value
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
    else {
        return;
    };
    let values = values
        .iter()
        .filter_map(Value::as_str)
        .take(limit)
        .map(|value| display_text(value, 120))
        .collect::<Vec<_>>();
    output.push_str(label);
    output.push_str(": ");
    output.push_str(values.join(", ").as_str());
    if value
        .and_then(Value::as_array)
        .is_some_and(|all| all.len() > limit)
    {
        output.push_str(", ...");
    }
    output.push('\n');
}

/// Appends one named string field on its own line.
pub(super) fn append_named_text(
    output: &mut String,
    label: &str,
    value: &Value,
    name: &str,
    limit: usize,
) {
    append_named(output, label, field_text(value, name, limit));
}

/// Appends one named integer field on its own line.
pub(super) fn append_named_integer(output: &mut String, label: &str, value: &Value, name: &str) {
    append_named(output, label, integer_text(value, name));
}

/// Appends a name/version pair using one separator.
pub(super) fn append_named_pair(
    output: &mut String,
    label: &str,
    value: &Value,
    left: &str,
    right: &str,
    separator: &str,
) {
    let (Some(left), Some(right)) = (field_text(value, left, 160), field_text(value, right, 120))
    else {
        return;
    };
    output.push_str(label);
    output.push_str(": ");
    output.push_str(left.as_str());
    output.push_str(separator);
    output.push_str(right.as_str());
    output.push('\n');
}

/// Appends one compact bounded string field.
pub(super) fn append_labeled_text(
    output: &mut String,
    label: &str,
    value: &Value,
    name: &str,
    limit: usize,
) {
    append_labeled(output, label, field_text(value, name, limit), false);
}

/// Appends one compact integer field.
pub(super) fn append_labeled_integer(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(output, label, integer_text(value, name), false);
}

/// Appends one compact approximate integer field with an explicit approximation marker.
pub(super) fn append_labeled_approximate_integer(
    output: &mut String,
    label: &str,
    value: &Value,
    name: &str,
) {
    append_labeled(output, label, integer_text(value, name), true);
}

/// Appends one compact basis-point field as a two-decimal percentage.
pub(super) fn append_labeled_basis_points(
    output: &mut String,
    label: &str,
    value: &Value,
    name: &str,
) {
    append_labeled(
        output,
        label,
        value
            .get(name)
            .and_then(Value::as_u64)
            .map(|value| format!("{}.{:02}%", value / 100, value % 100)),
        false,
    );
}

/// Appends one compact finite numeric field.
pub(super) fn append_labeled_number(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(
        output,
        label,
        value
            .get(name)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .map(|value| value.to_string()),
        false,
    );
}

/// Appends one compact boolean field.
pub(super) fn append_labeled_bool(output: &mut String, label: &str, value: &Value, name: &str) {
    append_labeled(
        output,
        label,
        value
            .get(name)
            .and_then(Value::as_bool)
            .map(|value| value.to_string()),
        false,
    );
}

/// Appends one optional value on its own named line.
fn append_named(output: &mut String, label: &str, value: Option<String>) {
    let Some(value) = value else { return };
    output.push_str(label);
    output.push_str(": ");
    output.push_str(value.as_str());
    output.push('\n');
}

/// Appends one optional compact value with an exact or approximate marker.
fn append_labeled(output: &mut String, label: &str, value: Option<String>, approximate: bool) {
    let Some(value) = value else { return };
    output.push(' ');
    output.push_str(label);
    output.push_str(if approximate { "=~" } else { "=" });
    output.push_str(value.as_str());
}

/// Returns a terminal-safe bounded string field.
pub(super) fn field_text(value: &Value, name: &str, limit: usize) -> Option<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(|value| display_text(value, limit))
}

/// Returns a signed or unsigned integer field as text.
pub(super) fn integer_text(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(|value| {
        value
            .as_i64()
            .map(|value| value.to_string())
            .or_else(|| value.as_u64().map(|value| value.to_string()))
    })
}

/// Escapes terminal controls and truncates one human-facing value.
pub(super) fn display_text(value: &str, limit: usize) -> String {
    let safe = terminal_safe(value);
    let mut characters = safe.chars();
    let mut output = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        output.push_str("...");
    }
    output
}
