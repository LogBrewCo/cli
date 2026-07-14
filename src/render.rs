//! Human-readable API response rendering.

use crate::{
    Command, ExplainTarget, ReadOptions, ReadTarget, RuntimeError, SetTarget, SupportTarget,
    SupportTicketListOptions,
};

/// Maximum span names shown in concise human trace summaries.
const SPAN_SUMMARY_LIMIT: usize = 5;
/// Maximum support-ticket rows rendered in one human response.
const SUPPORT_HUMAN_ROW_LIMIT: usize = 100;
/// Maximum accepted support timestamp length, including a bounded fraction.
const SUPPORT_TIMESTAMP_LIMIT: usize = 48;

/// One validated optional support field.
enum OptionalSupportField {
    /// Field is absent or null.
    Missing,
    /// Field contains bounded display text.
    Present(String),
}

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
        if matches!(command, Command::Support { .. }) {
            write!(output, "{}", invalid_support_response())?;
        } else if let Some(title) = cursor_response_title(command) {
            write!(output, "{}", invalid_cursor_message(title))?;
        } else {
            writeln!(output, "{body}")?;
        }
        return Ok(());
    };
    if let Some(summary) = human_summary(command, &value) {
        write!(output, "{summary}")?;
    } else {
        writeln!(output, "{body}")?;
    }
    Ok(())
}

/// Returns the human title for a cursor-mode collection read.
fn cursor_response_title(command: &Command) -> Option<&'static str> {
    match command {
        Command::Read {
            target, options, ..
        } if options.pagination.as_deref() == Some("cursor") => match target {
            ReadTarget::Logs => Some("Logs"),
            ReadTarget::Actions => Some("Actions"),
            ReadTarget::Issues => Some("Issues"),
            ReadTarget::Releases
            | ReadTarget::Traces
            | ReadTarget::Trace(_)
            | ReadTarget::Issue(_) => None,
        },
        Command::Support {
            target: SupportTarget::List(options),
            ..
        } if options.pagination.as_deref() == Some("cursor") => Some("Support tickets"),
        Command::Read { .. }
        | Command::Help { .. }
        | Command::Login { .. }
        | Command::Logout { .. }
        | Command::Status { .. }
        | Command::Version { .. }
        | Command::Watch { .. }
        | Command::Explain { .. }
        | Command::Set { .. }
        | Command::ProjectSetupSeen { .. }
        | Command::Support { .. }
        | Command::Setup { .. } => None,
    }
}

/// Returns a bounded support response summary or a value-safe recovery.
fn support_summary(target: &SupportTarget, value: &serde_json::Value) -> String {
    match target {
        SupportTarget::Create(_) => {
            support_created_summary(value).unwrap_or_else(invalid_support_response)
        }
        SupportTarget::List(options) => support_list_summary(options, value),
        SupportTarget::Detail(_) => {
            support_detail_summary(value).unwrap_or_else(invalid_support_response)
        }
    }
}

/// Builds a concise human summary for a successful API response.
fn human_summary(command: &Command, value: &serde_json::Value) -> Option<String> {
    match command {
        Command::Read {
            target, options, ..
        } => read_summary(target, options, value),
        Command::Explain { target, .. } => explain_summary(target, value),
        Command::Set { target, .. } => set_summary(target, value),
        Command::ProjectSetupSeen { .. } => project_setup_seen_summary(value),
        Command::Support { target, .. } => Some(support_summary(target, value)),
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
fn read_summary(
    target: &ReadTarget,
    options: &ReadOptions,
    value: &serde_json::Value,
) -> Option<String> {
    match target {
        ReadTarget::Logs if options.pagination.as_deref() == Some("cursor") => {
            Some(cursor_list_summary("Logs", "log", "logs", value, log_line))
        }
        ReadTarget::Logs => list_summary("Logs", list_items(value, "logs")?, log_line),
        ReadTarget::Issues if options.pagination.as_deref() == Some("cursor") => Some(
            cursor_list_summary("Issues", "issue", "issues", value, issue_cursor_line),
        ),
        ReadTarget::Issues => list_summary("Issues", list_items(value, "issues")?, issue_line),
        ReadTarget::Actions if options.pagination.as_deref() == Some("cursor") => Some(
            cursor_list_summary("Actions", "action", "actions", value, action_line),
        ),
        ReadTarget::Actions => list_summary("Actions", list_items(value, "actions")?, action_line),
        ReadTarget::Releases => {
            list_summary("Releases", list_items(value, "releases")?, release_line)
        }
        ReadTarget::Traces => trace_list_summary(list_items(value, "traces")?),
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

/// Builds the concise result of support-ticket creation.
fn support_created_summary(value: &serde_json::Value) -> Option<String> {
    let ticket_id = field(value, "ticket_id")?;
    let status = required_support_field(value, "status", 64)?;
    let created_at = bounded_support_timestamp(field(value, "created_at")?)?;
    let next = bounded_display_text(field(value, "next")?, 240)?;
    if !is_uuid(ticket_id) {
        return None;
    }
    validate_next_action(value.get("next_action")?)?;
    Some(format!(
        "Support ticket {ticket_id} created ({status})\nCreated: {created_at}\nNext: {next}\n"
    ))
}

/// Builds support-ticket history for legacy and cursor envelopes.
fn support_list_summary(options: &SupportTicketListOptions, value: &serde_json::Value) -> String {
    let Some(tickets) = value.get("tickets").and_then(serde_json::Value::as_array) else {
        return invalid_support_response();
    };
    let Some(next) = value
        .get("next")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| bounded_display_text(value, 240))
    else {
        return invalid_support_response();
    };
    if value
        .get("next_action")
        .and_then(validate_next_action)
        .is_none()
    {
        return invalid_support_response();
    }
    let cursor = if options.pagination.as_deref() == Some("cursor") {
        let Some(cursor) = value.get("next_cursor") else {
            return invalid_support_response();
        };
        if cursor.is_null() {
            Some(None)
        } else {
            let Some(time) = field(cursor, "time").and_then(bounded_support_timestamp) else {
                return invalid_support_response();
            };
            let Some(id) = field(cursor, "id") else {
                return invalid_support_response();
            };
            if !is_uuid(id) {
                return invalid_support_response();
            }
            Some(Some((time, id)))
        }
    } else {
        None
    };

    let mut output = format!("Support tickets ({})\n", tickets.len());
    if tickets.is_empty() {
        output.push_str("No support tickets found.\n");
    } else {
        for ticket in tickets.iter().take(SUPPORT_HUMAN_ROW_LIMIT) {
            let Some(line) = support_ticket_line(ticket) else {
                return invalid_support_response();
            };
            output.push_str("- ");
            output.push_str(line.as_str());
            output.push('\n');
        }
        if tickets.len() > SUPPORT_HUMAN_ROW_LIMIT {
            output.push_str(
                format!(
                    "Showing first {SUPPORT_HUMAN_ROW_LIMIT} of {} tickets. Use filters or a smaller --limit to narrow the page.\n",
                    tickets.len()
                )
                .as_str(),
            );
        }
    }

    match cursor {
        Some(Some((time, id))) => {
            output.push_str("Next page: set --cursor-time ");
            output.push_str(time.as_str());
            output.push_str(" --cursor-id ");
            output.push_str(id);
            output.push_str(
                " on the same command; keep --pagination cursor, --limit, and active filters unchanged.\n",
            );
            output.push_str("Retry: rerun that same command; the rows above remain visible.\n");
        }
        Some(None) => output.push_str("End of support ticket history.\n"),
        None => {}
    }
    output.push_str("Next: ");
    output.push_str(next.as_str());
    output.push('\n');
    output
}

/// Builds one privacy-bounded support-ticket detail summary.
fn support_detail_summary(value: &serde_json::Value) -> Option<String> {
    let ticket_id = field(value, "ticket_id")?;
    let status = required_support_field(value, "status", 64)?;
    let category = required_support_field(value, "category", 64)?;
    let title = bounded_display_text(field(value, "title")?, 120)?;
    let created_at = bounded_support_timestamp(field(value, "created_at")?)?;
    if !is_uuid(ticket_id) {
        return None;
    }
    let mut output = format!(
        "Support ticket {ticket_id} {status}\nCategory: {category}\nTitle: {title}\nCreated: {created_at}\n"
    );
    if let OptionalSupportField::Present(project_id) =
        optional_support_field(value, "project_id", 64)?
    {
        output.push_str("Project: ");
        output.push_str(project_id.as_str());
        output.push('\n');
    }
    if let OptionalSupportField::Present(scope) = support_scope_suffix(value)? {
        output.push_str("Scope: ");
        output.push_str(scope.as_str());
        output.push('\n');
    }
    if let Some(next) = field(value, "next").and_then(|value| bounded_display_text(value, 240)) {
        output.push_str("Next: ");
        output.push_str(next.as_str());
        output.push('\n');
    }
    Some(output)
}

/// Formats one support-ticket list row without description or diagnostics.
fn support_ticket_line(value: &serde_json::Value) -> Option<String> {
    let ticket_id = field(value, "ticket_id")?;
    let status = required_support_field(value, "status", 64)?;
    let category = required_support_field(value, "category", 64)?;
    let title = bounded_display_text(field(value, "title")?, 120)?;
    let created_at = bounded_support_timestamp(field(value, "created_at")?)?;
    if !is_uuid(ticket_id) {
        return None;
    }
    let mut output = format!("{ticket_id} {status} {category} {title} created={created_at}");
    append_optional_support_field(&mut output, "project", value, "project_id", 64)?;
    append_optional_support_field(&mut output, "source", value, "source", 32)?;
    if let OptionalSupportField::Present(scope) = support_scope_suffix(value)? {
        output.push(' ');
        output.push_str(scope.as_str());
    }
    Some(output)
}

/// Builds release/environment scope only when at least one field is present.
fn support_scope_suffix(value: &serde_json::Value) -> Option<OptionalSupportField> {
    let release = optional_support_field(value, "release", 120)?;
    let environment = optional_support_field(value, "environment", 120)?;
    match (release, environment) {
        (OptionalSupportField::Missing, OptionalSupportField::Missing) => {
            Some(OptionalSupportField::Missing)
        }
        (release, environment) => Some(OptionalSupportField::Present(format!(
            "[{} / {}]",
            support_field_or(&release, "unknown release"),
            support_field_or(&environment, "unknown environment")
        ))),
    }
}

/// Returns the present support field or a fixed fallback.
const fn support_field_or<'a>(field: &'a OptionalSupportField, fallback: &'a str) -> &'a str {
    match field {
        OptionalSupportField::Missing => fallback,
        OptionalSupportField::Present(value) => value.as_str(),
    }
}

/// Returns one required bounded, control-safe support field.
fn required_support_field(value: &serde_json::Value, key: &str, limit: usize) -> Option<String> {
    bounded_display_text(field(value, key)?, limit)
}

/// Returns one bounded UTC RFC3339 timestamp for human output.
fn bounded_support_timestamp(value: &str) -> Option<String> {
    if value.chars().count() > SUPPORT_TIMESTAMP_LIMIT || !is_rfc3339_utc(value) {
        None
    } else {
        Some(value.to_owned())
    }
}

/// Returns one optional bounded, control-safe support field.
fn optional_support_field(
    value: &serde_json::Value,
    key: &str,
    limit: usize,
) -> Option<OptionalSupportField> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Some(OptionalSupportField::Missing),
        Some(serde_json::Value::String(value)) => {
            bounded_display_text(value.as_str(), limit).map(OptionalSupportField::Present)
        }
        Some(_) => None,
    }
}

/// Appends one optional bounded support field.
fn append_optional_support_field(
    output: &mut String,
    label: &str,
    value: &serde_json::Value,
    key: &str,
    limit: usize,
) -> Option<()> {
    if let OptionalSupportField::Present(field) = optional_support_field(value, key, limit)? {
        output.push(' ');
        output.push_str(label);
        output.push('=');
        output.push_str(field.as_str());
    }
    Some(())
}

/// Validates one public next-action object without rendering it.
fn validate_next_action(value: &serde_json::Value) -> Option<()> {
    let code = field(value, "code")?;
    let target = field(value, "target")?;
    (!code.is_empty() && !target.is_empty()).then_some(())
}

/// Collapses and bounds one human-facing server string.
fn bounded_display_text(value: &str, limit: usize) -> Option<String> {
    if value.chars().any(char::is_control) {
        return None;
    }
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    let mut chars = normalized.chars();
    let mut output = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        output.push_str("...");
    }
    Some(output)
}

/// Returns value-safe recovery for malformed support success responses.
fn invalid_support_response() -> String {
    String::from(
        "Support response could not be rendered safely.\nNext: retry the same command with --json and inspect the public response shape.\n",
    )
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

/// Builds cursor-paginated output without clearing prior pages.
fn cursor_list_summary(
    title: &str,
    history_name: &str,
    wrapper_key: &str,
    value: &serde_json::Value,
    line_builder: fn(&serde_json::Value) -> Option<String>,
) -> String {
    let Some(items) = value.get(wrapper_key).and_then(serde_json::Value::as_array) else {
        return invalid_cursor_message(title);
    };
    let Some(next_cursor) = value.get("next_cursor") else {
        return invalid_cursor_message(title);
    };
    let cursor = if next_cursor.is_null() {
        None
    } else {
        let Some(time) = field(next_cursor, "time") else {
            return invalid_cursor_message(title);
        };
        let Some(id) = field(next_cursor, "id") else {
            return invalid_cursor_message(title);
        };
        if !is_rfc3339_utc(time) || !is_uuid(id) {
            return invalid_cursor_message(title);
        }
        Some((time, id))
    };

    let mut output = format!("{title} ({})\n", items.len());
    if items.is_empty() {
        output
            .push_str(format!("No {} found on this page.\n", title.to_ascii_lowercase()).as_str());
    } else {
        for item in items {
            let Some(line) = line_builder(item) else {
                return invalid_cursor_message(title);
            };
            output.push_str("- ");
            output.push_str(line.as_str());
            output.push('\n');
        }
    }

    let Some((time, id)) = cursor else {
        output.push_str(format!("End of {history_name} history.\n").as_str());
        return output;
    };
    output.push_str("Next page: set --cursor-time ");
    output.push_str(time);
    output.push_str(" --cursor-id ");
    output.push_str(id);
    output.push_str(
        " on the same command; keep --pagination cursor, --limit, and active filters unchanged.\n",
    );
    output.push_str("Retry: rerun that same command; the rows above remain visible.\n");
    output
}

/// Builds a value-safe recovery when a cursor response violates its public shape.
fn invalid_cursor_message(title: &str) -> String {
    format!(
        "{title} response could not be rendered safely.\nNext: retry the same command with --json and inspect next_cursor.\n",
    )
}

/// Checks the UTC RFC3339 shape returned by cursor endpoints.
fn is_rfc3339_utc(value: &str) -> bool {
    let Some(without_zone) = value
        .strip_suffix('Z')
        .or_else(|| value.strip_suffix("+00:00"))
    else {
        return false;
    };
    let (seconds, fraction) = match without_zone.split_once('.') {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (without_zone, None),
    };
    if fraction.is_some_and(|digits| {
        digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return false;
    }

    let bytes = seconds.as_bytes();
    if bytes.len() != 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return false;
    }
    for index in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !bytes[index].is_ascii_digit() {
            return false;
        }
    }

    let Some(year) = ascii_number(&bytes[0..4]) else {
        return false;
    };
    let Some(month) = ascii_number(&bytes[5..7]) else {
        return false;
    };
    let Some(day) = ascii_number(&bytes[8..10]) else {
        return false;
    };
    let Some(hour) = ascii_number(&bytes[11..13]) else {
        return false;
    };
    let Some(minute) = ascii_number(&bytes[14..16]) else {
        return false;
    };
    let Some(second) = ascii_number(&bytes[17..19]) else {
        return false;
    };

    (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        && second <= 60
}

/// Parses an ASCII decimal field without accepting signs or whitespace.
fn ascii_number(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(*byte - b'0'))
    })
}

/// Returns the number of calendar days in one month.
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    }
}

/// Reports whether a Gregorian year has a leap day.
const fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Checks the canonical hyphenated UUID shape returned by cursor endpoints.
fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
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
    append_labeled_field(&mut output, "service", value, "service_name");
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
    append_labeled_field(&mut output, "last_seen", value, "last_seen_at");
    append_labeled_field(&mut output, "service", value, "service_name");
    append_labeled_field(&mut output, "trace", value, "trace_id");
    output.push_str(release_environment_suffix(value).as_str());
    Some(output)
}

/// Formats a cursor issue only when every bounded list field has the public type.
fn issue_cursor_line(value: &serde_json::Value) -> Option<String> {
    let id = field(value, "id")?;
    let status = field(value, "status")?;
    let (_severity, _title, _occurrences) = (
        field(value, "severity")?,
        field(value, "title")?,
        value.get("occurrence_count")?.as_u64()?,
    );
    let last_seen = field(value, "last_seen_at")?;
    if !is_uuid(id)
        || !matches!(status, "unresolved" | "resolved" | "ignored")
        || !is_rfc3339_utc(last_seen)
    {
        return None;
    }
    issue_line(value)
}

/// Formats one action list item.
fn action_line(value: &serde_json::Value) -> Option<String> {
    let name = field(value, "name")?;
    let mut output = name.to_owned();
    if let Some(severity) = display_severity(value) {
        output.push(' ');
        output.push_str(&severity);
    }
    append_labeled_field(&mut output, "service", value, "service_name");
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
    let mut output = format!("{release} {environment}");
    append_labeled_field(&mut output, "service", value, "service_name");
    output
        .push_str(format!(" logs={logs} issues={issues} spans={spans} actions={actions}").as_str());
    Some(output)
}

/// Builds a concise recent-trace list with a detail-read next step.
fn trace_list_summary(items: &[serde_json::Value]) -> Option<String> {
    let mut output = format!("Traces ({})\n", items.len());
    if items.is_empty() {
        output.push_str(
            "No traces found.\nNext: widen --project/--service/--release/--environment/--status/\
             --since/--min-duration-ms filters.\n",
        );
        return Some(output);
    }
    for item in items {
        output.push_str("- ");
        output.push_str(trace_list_line(item)?.as_str());
        output.push('\n');
    }
    output.push_str("Next: logbrew trace <trace_id> or logbrew explain trace <trace_id>\n");
    Some(output)
}

/// Formats one recent trace summary.
fn trace_list_line(value: &serde_json::Value) -> Option<String> {
    let trace_id = field(value, "trace_id")?;
    let errors = count_field(value, "error_span_count");
    let status = if value
        .get("error_span_count")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|count| count > 0)
    {
        "error"
    } else {
        "ok"
    };
    let mut output = format!("{trace_id} {status}");
    if let Some(name) = field(value, "root_span_name") {
        output.push(' ');
        output.push_str(name);
    }
    append_labeled_field(&mut output, "service", value, "root_service_name");
    append_labeled_field(&mut output, "operation", value, "root_operation");
    output.push_str(" spans=");
    output.push_str(count_field(value, "span_count").as_str());
    output.push_str(" errors=");
    output.push_str(errors.as_str());
    output.push_str(" services=");
    output.push_str(count_field(value, "service_count").as_str());
    output.push_str(" duration=");
    output.push_str(count_field(value, "duration_ms").as_str());
    output.push_str("ms");
    append_labeled_field(&mut output, "started", value, "started_at");
    Some(output)
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
    output.push_str("Next: ");
    output.push_str(issue_next_step(id, status).as_str());
    output.push('\n');
    Some(output)
}

/// Returns an issue object from either real bare API objects or legacy wrappers.
fn issue_value(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value
        .get("issue")
        .or_else(|| value.as_object().map(|_| value))
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
