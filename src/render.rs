//! Human-readable API response rendering.

use crate::{Command, ExplainTarget, ReadTarget, RuntimeError, SetTarget};

/// Maximum span names shown in concise human trace summaries.
const SPAN_SUMMARY_LIMIT: usize = 5;

/// Writes successful API output for JSON or human command modes.
pub(crate) fn write_api_success<W: std::io::Write>(
    command: &Command,
    body: &str,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if command.wants_json() {
        writeln!(output, "{body}")?;
        return Ok(());
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        writeln!(output, "{body}")?;
        return Ok(());
    };
    if let Some(summary) = human_summary(command, &value) {
        write!(output, "{summary}")?;
    } else {
        writeln!(output, "{body}")?;
    }
    Ok(())
}

/// Builds a concise human summary for a successful API response.
fn human_summary(command: &Command, value: &serde_json::Value) -> Option<String> {
    match command {
        Command::Read { target, .. } => read_summary(target, value),
        Command::Explain { target, .. } => explain_summary(target, value),
        Command::Set { target, .. } => set_summary(target, value),
        Command::ProjectSetupSeen { .. } => project_setup_seen_summary(value),
        Command::Help { .. }
        | Command::Login { .. }
        | Command::Logout { .. }
        | Command::Setup { .. }
        | Command::Status { .. }
        | Command::Version { .. }
        | Command::Watch { .. } => None,
    }
}

/// Builds a human summary for read responses.
fn read_summary(target: &ReadTarget, value: &serde_json::Value) -> Option<String> {
    match target {
        ReadTarget::Logs => list_summary("Logs", list_items(value, "logs")?, log_line),
        ReadTarget::Issues => list_summary("Issues", list_items(value, "issues")?, issue_line),
        ReadTarget::Actions => list_summary("Actions", list_items(value, "actions")?, action_line),
        ReadTarget::Releases => {
            list_summary("Releases", list_items(value, "releases")?, release_line)
        }
        ReadTarget::Trace(id) => trace_summary(value, id.as_str()),
        ReadTarget::Issue(_) => issue_detail_summary(value),
    }
}

/// Builds a human summary for explain responses.
fn explain_summary(target: &ExplainTarget, value: &serde_json::Value) -> Option<String> {
    match target {
        ExplainTarget::Issue(_) => issue_detail_summary(value),
        ExplainTarget::Trace(id) => trace_summary(value, id.as_str()),
    }
}

/// Builds a human summary for mutation responses.
fn set_summary(target: &SetTarget, value: &serde_json::Value) -> Option<String> {
    match target {
        SetTarget::IssueStatus { .. } => {
            let issue = issue_value(value)?;
            let id = field(issue, "id")?;
            let status = field(issue, "status")?;
            let mut output = format!("Issue {id} marked {status}");
            append_labeled_field(&mut output, "trace", issue, "trace_id");
            output.push_str(release_environment_suffix(issue).as_str());
            output.push_str(".\n");
            if let Some(next) = response_next(value, issue) {
                append_next(&mut output, next);
            }
            Some(output)
        }
    }
}

/// Builds a human summary for backend-owned project setup state.
fn project_setup_seen_summary(value: &serde_json::Value) -> Option<String> {
    let status = field(value, "status")?;
    let mut output = format!("Project setup seen: {status}\n");
    if let Some(last_seen_at) = field(value, "last_seen_at") {
        output.push_str("Last seen: ");
        output.push_str(last_seen_at);
        output.push('\n');
    }
    if let Some(next) = field(value, "next") {
        output.push_str("Next: ");
        output.push_str(next);
        output.push('\n');
    } else {
        output.push_str("Next: send telemetry for this project\n");
    }
    Some(output)
}

/// Returns list items from either real bare API arrays or legacy wrapper objects.
fn list_items<'a>(
    value: &'a serde_json::Value,
    wrapper_key: &str,
) -> Option<&'a [serde_json::Value]> {
    if let Some(items) = value.as_array() {
        Some(items.as_slice())
    } else {
        value.get(wrapper_key)?.as_array().map(Vec::as_slice)
    }
}

/// Builds a titled list summary from an array response.
fn list_summary(
    title: &str,
    items: &[serde_json::Value],
    line_builder: fn(&serde_json::Value) -> Option<String>,
) -> Option<String> {
    let mut output = format!("{title} ({})\n", items.len());
    if items.is_empty() {
        output.push_str(empty_list_message(title).as_str());
        return Some(output);
    }
    for item in items {
        output.push_str("- ");
        output.push_str(line_builder(item)?.as_str());
        output.push('\n');
    }
    Some(output)
}

/// Builds an empty-state message for list responses.
fn empty_list_message(title: &str) -> String {
    format!(
        "No {} found.\nNext: widen filters or check --release/--environment.\n",
        title.to_ascii_lowercase()
    )
}

/// Formats one log list item.
fn log_line(value: &serde_json::Value) -> Option<String> {
    let message = field(value, "message")?;
    let mut output = String::new();
    if let Some(severity) = display_severity(value) {
        output.push_str(&severity);
        output.push(' ');
    }
    output.push_str(message);
    append_labeled_field(&mut output, "trace", value, "trace_id");
    output.push_str(release_environment_suffix(value).as_str());
    Some(output)
}

/// Formats one issue list item.
fn issue_line(value: &serde_json::Value) -> Option<String> {
    let id = field(value, "id")?;
    let status = field(value, "status")?;
    let mut output = format!("{id} {status}");
    if let Some(severity) = display_severity(value) {
        output.push(' ');
        output.push_str(&severity);
    }
    if let Some(title) = field(value, "title") {
        output.push(' ');
        output.push_str(title);
    }
    if let Some(occurrences) = int_field(value, "occurrence_count") {
        output.push_str(" occurrences=");
        output.push_str(occurrences.to_string().as_str());
    }
    append_labeled_field(&mut output, "trace", value, "trace_id");
    output.push_str(release_environment_suffix(value).as_str());
    Some(output)
}

/// Formats one action list item.
fn action_line(value: &serde_json::Value) -> Option<String> {
    let name = field(value, "name")?;
    let mut output = name.to_owned();
    if let Some(severity) = display_severity(value) {
        output.push(' ');
        output.push_str(&severity);
    }
    append_labeled_field(&mut output, "user", value, "distinct_id");
    append_labeled_field(&mut output, "trace", value, "trace_id");
    output.push_str(release_environment_suffix(value).as_str());
    Some(output)
}

/// Formats one release list item.
fn release_line(value: &serde_json::Value) -> Option<String> {
    let release = field(value, "release")?;
    let environment = field(value, "environment")?;
    let logs = count_field(value, "log_count");
    let issues = count_field(value, "issue_count");
    let spans = count_field(value, "trace_span_count");
    let actions = count_field(value, "action_count");
    Some(format!(
        "{release} {environment} logs={logs} issues={issues} spans={spans} actions={actions}"
    ))
}

/// Builds a single trace summary from bare API span arrays or wrapper objects.
fn trace_summary(value: &serde_json::Value, fallback_trace_id: &str) -> Option<String> {
    let (trace_id, context, spans) = trace_parts(value, fallback_trace_id)?;
    let mut output = format!(
        "Trace {trace_id} spans={}{}\n",
        spans.len(),
        context.map_or_else(String::new, release_environment_suffix)
    );
    append_span_names(&mut output, spans);
    if spans.is_empty() {
        output.push_str("Next: widen filters or check --release/--environment.\n");
    }
    Some(output)
}

/// Extracts trace identity, display context, and span rows from supported shapes.
fn trace_parts<'a>(
    value: &'a serde_json::Value,
    fallback_trace_id: &'a str,
) -> Option<(
    &'a str,
    Option<&'a serde_json::Value>,
    &'a [serde_json::Value],
)> {
    if let Some(trace) = value.get("trace") {
        let spans = trace
            .get("spans")
            .and_then(serde_json::Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let trace_id = field(trace, "trace_id")
            .or_else(|| spans.first().and_then(|span| field(span, "trace_id")))
            .unwrap_or(fallback_trace_id);
        let context = if release_environment_suffix(trace).is_empty() {
            spans.first()
        } else {
            Some(trace)
        };
        return Some((trace_id, context, spans));
    }

    let spans = value.as_array()?;
    let trace_id = spans
        .first()
        .and_then(|span| field(span, "trace_id"))
        .unwrap_or(fallback_trace_id);
    Some((trace_id, spans.first(), spans.as_slice()))
}

/// Appends the first span names to a trace summary.
fn append_span_names(output: &mut String, spans: &[serde_json::Value]) {
    let names = spans
        .iter()
        .filter_map(|span| field(span, "name"))
        .collect::<Vec<_>>();
    for name in names.iter().take(SPAN_SUMMARY_LIMIT) {
        output.push_str("- ");
        output.push_str(name);
        output.push('\n');
    }
    if names.len() > SPAN_SUMMARY_LIMIT {
        output.push_str("- ... ");
        output.push_str((names.len() - SPAN_SUMMARY_LIMIT).to_string().as_str());
        output.push_str(" more spans\n");
    }
}

/// Builds a single issue summary.
fn issue_detail_summary(value: &serde_json::Value) -> Option<String> {
    let issue = issue_value(value)?;
    let id = field(issue, "id")?;
    let status = field(issue, "status")?;
    let mut output = format!("Issue {id} {status}");
    if let Some(severity) = display_severity(issue) {
        output.push(' ');
        output.push_str(&severity);
    }
    append_labeled_field(&mut output, "trace", issue, "trace_id");
    output.push_str(release_environment_suffix(issue).as_str());
    output.push('\n');
    if let Some(title) = field(issue, "title") {
        output.push_str("Title: ");
        output.push_str(title);
        output.push('\n');
    }
    if let Some(message) = field(issue, "message") {
        output.push_str("Message: ");
        output.push_str(message);
        output.push('\n');
    }
    if let Some(occurrences) = int_field(issue, "occurrence_count") {
        output.push_str("Occurrences: ");
        output.push_str(occurrences.to_string().as_str());
        output.push('\n');
    }
    if let Some(first_seen) = field(issue, "first_seen_at") {
        output.push_str("First seen: ");
        output.push_str(first_seen);
        output.push('\n');
    }
    if let Some(last_seen) = field(issue, "last_seen_at") {
        output.push_str("Last seen: ");
        output.push_str(last_seen);
        output.push('\n');
    }
    if let Some(next) = response_next(value, issue) {
        append_next(&mut output, next);
    } else {
        append_next(&mut output, issue_next_step(id, status).as_str());
    }
    Some(output)
}

/// Returns an issue object from either real bare API objects or legacy wrappers.
fn issue_value(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value
        .get("issue")
        .or_else(|| value.as_object().map(|_| value))
}

/// Returns backend-provided next-step copy from a response wrapper or item.
fn response_next<'a>(
    value: &'a serde_json::Value,
    nested: &'a serde_json::Value,
) -> Option<&'a str> {
    field(value, "next").or_else(|| field(nested, "next"))
}

/// Appends a next-step line to a human summary.
fn append_next(output: &mut String, next: &str) {
    output.push_str("Next: ");
    output.push_str(next);
    output.push('\n');
}

/// Returns a string field value.
fn field<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    value.get(name)?.as_str()
}

/// Returns the user-facing severity label, preferring canonical backend severity.
fn display_severity(value: &serde_json::Value) -> Option<std::borrow::Cow<'_, str>> {
    field(value, "severity")
        .or_else(|| field(value, "level"))
        .map(canonical_severity_label)
}

/// Maps SDK/runtime aliases to the public severity vocabulary for human output.
fn canonical_severity_label(value: &str) -> std::borrow::Cow<'_, str> {
    match value.to_ascii_lowercase().as_str() {
        "trace" | "debug" | "info" | "information" => std::borrow::Cow::Borrowed("info"),
        "warn" | "warning" => std::borrow::Cow::Borrowed("warning"),
        "error" | "err" => std::borrow::Cow::Borrowed("error"),
        "fatal" | "critical" => std::borrow::Cow::Borrowed("critical"),
        _ => std::borrow::Cow::Borrowed(value),
    }
}

/// Appends a compact labeled string field to an existing line.
fn append_labeled_field(
    output: &mut String,
    label: &str,
    value: &serde_json::Value,
    field_name: &str,
) {
    if let Some(field_value) = field(value, field_name) {
        output.push(' ');
        output.push_str(label);
        output.push('=');
        output.push_str(field_value);
    }
}

/// Returns a signed integer field value.
fn int_field(value: &serde_json::Value, name: &str) -> Option<i64> {
    value.get(name)?.as_i64()
}

/// Returns a numeric count field as display text.
fn count_field(value: &serde_json::Value, name: &str) -> String {
    value
        .get(name)
        .and_then(serde_json::Value::as_u64)
        .map_or_else(|| String::from("0"), |count| count.to_string())
}

/// Returns the next issue action for the current status.
fn issue_next_step(id: &str, status: &str) -> String {
    match status {
        "resolved" | "ignored" => format!("logbrew reopen {id}"),
        _ => format!("logbrew resolve {id} or logbrew ignore {id}"),
    }
}

/// Builds an optional release/environment suffix.
fn release_environment_suffix(value: &serde_json::Value) -> String {
    let release = field(value, "release");
    let environment = field(value, "environment");
    match (release, environment) {
        (Some(release), Some(environment)) => format!(" [{release} / {environment}]"),
        (Some(release), None) => format!(" [{release}]"),
        (None, Some(environment)) => format!(" [{environment}]"),
        (None, None) => String::new(),
    }
}
