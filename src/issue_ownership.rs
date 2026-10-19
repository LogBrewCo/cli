//! Issue owners, activity, ownership rules, and severity rules.
//!
//! `logbrew issues owners|activity|ownership|rule` and `logbrew logs rule` show who owns an
//! issue, what happened to it, and a project's ownership rules and CODEOWNERS file, and set
//! severity rules for issues and logs.

use crate::ids::is_uuid;
use crate::{CliError, Command, ReadOptions, ReadTarget, SetTarget};

/// Recovery for malformed owner, activity, and ownership commands.
pub const OWNERSHIP_NEXT_STEP: &str = "use logbrew issues owners <issue_id>, logbrew issues activity <issue_id> [--limit <1-200>] [--cursor-time <time> --cursor-id <id>], logbrew issues ownership --project <project_id>, or logbrew issues ownership set --project <project_id> [--rules <file>] [--codeowners <file>], each with optional --json";
/// Recovery for malformed severity rule commands.
pub const RULE_NEXT_STEP: &str = "use logbrew issues rule <issue_id> <severity> or logbrew logs rule <log_id> <severity> [--ignore], where severity is source, info, warning, error, or critical, with optional --json";
/// Recovery for malformed inbox commands.
pub const INBOX_NEXT_STEP: &str = "use logbrew inbox [--limit <1-200>] [--cursor-time <time> --cursor-id <id>] or logbrew inbox read, each with optional --json";
/// Largest ownership file, matching the API limit.
const MAX_FILE_BYTES: u64 = 128 * 1024;
/// Severity rule values: `source` restores the captured severity.
const SEVERITIES: [&str; 5] = ["source", "info", "warning", "error", "critical"];

/// Whether `args` after `issues` start an owner, activity, ownership, or rule command.
#[must_use]
pub fn is_command(args: &[String]) -> bool {
    args.iter()
        .find(|arg| *arg != "--json")
        .is_some_and(|action| {
            matches!(
                action.as_str(),
                "owners" | "activity" | "ownership" | "rule"
            )
        })
}

/// Parses `issues owners|activity|ownership|rule ...`.
///
/// # Errors
///
/// Returns [`CliError::InvalidOwnershipCommand`], [`CliError::InvalidSeverityRuleCommand`], or
/// [`CliError::InvalidOwnershipFile`] for any other form or an unusable file.
pub fn parse(args: &[String]) -> Result<Command, CliError> {
    let json = args.iter().filter(|arg| *arg == "--json").count();
    let words = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--json")
        .collect::<Vec<_>>();
    if json > 1 {
        return Err(match words.first() {
            Some(&"rule") => CliError::InvalidSeverityRuleCommand,
            _ => CliError::InvalidOwnershipCommand,
        });
    }
    let json = json == 1;
    match words.as_slice() {
        ["owners", issue] if is_uuid(issue) => {
            Ok(read(ReadTarget::IssueOwners(lower(issue)), json))
        }
        ["activity", issue, flags @ ..] if is_uuid(issue) => activity(issue, flags, json),
        ["ownership", "--project" | "--project-id", project] if is_uuid(project) => {
            Ok(read(ReadTarget::IssueOwnership(lower(project)), json))
        }
        ["ownership", "set", flags @ ..] => set_ownership(flags, json),
        ["rule", issue, severity] if is_uuid(issue) && SEVERITIES.contains(severity) => {
            Ok(Command::Set {
                target: SetTarget::IssueRule {
                    id: lower(issue),
                    severity: (*severity).to_owned(),
                },
                json,
            })
        }
        ["rule", ..] => Err(CliError::InvalidSeverityRuleCommand),
        _ => Err(CliError::InvalidOwnershipCommand),
    }
}

/// Parses `inbox [--limit N] [--cursor-time T --cursor-id ID]` and `inbox read`.
///
/// # Errors
///
/// Returns [`CliError::InvalidInboxCommand`] for any other form.
pub fn parse_inbox(args: &[String]) -> Result<Command, CliError> {
    let json = args.iter().filter(|arg| *arg == "--json").count();
    let words = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--json")
        .collect::<Vec<_>>();
    if json > 1 {
        return Err(CliError::InvalidInboxCommand);
    }
    if words == ["read"] {
        return Ok(Command::Set {
            target: SetTarget::NotificationsRead,
            json: json == 1,
        });
    }
    let options = page_options(&words).ok_or(CliError::InvalidInboxCommand)?;
    Ok(Command::Read {
        target: ReadTarget::Notifications,
        options: Box::new(options),
        json: json == 1,
    })
}

/// Parses `--limit` (1 to 200) and a complete `--cursor-time` and `--cursor-id` pair.
fn page_options(flags: &[&str]) -> Option<ReadOptions> {
    let mut options = ReadOptions::default();
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let value = (*flags.next()?).to_owned();
        let slot = match *flag {
            "--limit" => &mut options.limit,
            "--cursor-time" => &mut options.cursor_time,
            "--cursor-id" => &mut options.cursor_id,
            _ => return None,
        };
        if slot.replace(value).is_some() {
            return None;
        }
    }
    let limit_ok = options.limit.as_deref().is_none_or(|limit| {
        limit
            .parse::<u16>()
            .is_ok_and(|limit| (1..=200).contains(&limit))
    });
    let cursor_ok = match (&options.cursor_time, &options.cursor_id) {
        (None, None) => true,
        (Some(time), Some(id)) => crate::render::is_rfc3339_utc(time) && is_uuid(id),
        _ => false,
    };
    (limit_ok && cursor_ok).then_some(options)
}

/// Parses `logs rule <log_id> <severity> [--ignore] [--json]`.
///
/// # Errors
///
/// Returns [`CliError::InvalidSeverityRuleCommand`] for any other form.
pub fn parse_log_rule(args: &[String]) -> Result<Command, CliError> {
    let args = args
        .iter()
        .filter(|arg| *arg != "rule")
        .cloned()
        .collect::<Vec<_>>();
    let json = args.iter().filter(|arg| *arg == "--json").count();
    let ignored = args.iter().filter(|arg| *arg == "--ignore").count();
    let words = args
        .iter()
        .map(String::as_str)
        .filter(|arg| !matches!(*arg, "--json" | "--ignore"))
        .collect::<Vec<_>>();
    match words.as_slice() {
        [log, severity]
            if is_uuid(log) && SEVERITIES.contains(severity) && json < 2 && ignored < 2 =>
        {
            Ok(Command::Set {
                target: SetTarget::LogRule {
                    id: lower(log),
                    severity: (*severity).to_owned(),
                    ignored: ignored == 1,
                },
                json: json == 1,
            })
        }
        _ => Err(CliError::InvalidSeverityRuleCommand),
    }
}

/// Whether `args` after `logs` are a log rule command rather than a log search.
#[must_use]
pub fn is_log_rule(args: &[String]) -> bool {
    let mut words = args.iter().filter(|arg| *arg != "--json");
    words.next().is_some_and(|word| word == "rule") && words.next().is_some_and(|id| is_uuid(id))
}

/// A read command for `target` with no list filters.
fn read(target: ReadTarget, json: bool) -> Command {
    Command::Read {
        target,
        options: Box::default(),
        json,
    }
}

/// Parses the activity page flags.
fn activity(issue: &str, flags: &[&str], json: bool) -> Result<Command, CliError> {
    let options = page_options(flags).ok_or(CliError::InvalidOwnershipCommand)?;
    Ok(Command::Read {
        target: ReadTarget::IssueActivity(lower(issue)),
        options: Box::new(options),
        json,
    })
}

/// Parses `ownership set --project <id> [--rules <file>] [--codeowners <file>]` and reads the
/// files.
fn set_ownership(flags: &[&str], json: bool) -> Result<Command, CliError> {
    let (mut project_id, mut rules, mut codeowners) = (None, None, None);
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let value = flags.next().ok_or(CliError::InvalidOwnershipCommand)?;
        let previous = match *flag {
            "--project" | "--project-id" if is_uuid(value) => project_id.replace(lower(value)),
            "--rules" => rules.replace(read_file(value)?),
            "--codeowners" => codeowners.replace(read_file(value)?),
            _ => return Err(CliError::InvalidOwnershipCommand),
        };
        if previous.is_some() {
            return Err(CliError::InvalidOwnershipCommand);
        }
    }
    let project_id = project_id.ok_or(CliError::InvalidOwnershipCommand)?;
    if rules.is_none() && codeowners.is_none() {
        return Err(CliError::InvalidOwnershipCommand);
    }
    Ok(Command::Set {
        target: SetTarget::IssueOwnership {
            project_id,
            rules,
            codeowners,
        },
        json,
    })
}

/// Reads one ownership file of at most 128 KiB of UTF-8 text.
fn read_file(path: &str) -> Result<String, CliError> {
    let file = std::fs::File::open(path).map_err(|_| CliError::InvalidOwnershipFile)?;
    let mut text = String::new();
    let read = std::io::Read::read_to_string(
        &mut std::io::Read::take(file, MAX_FILE_BYTES + 1),
        &mut text,
    )
    .map_err(|_| CliError::InvalidOwnershipFile)?;
    if u64::try_from(read).map_or(true, |read| read > MAX_FILE_BYTES) {
        return Err(CliError::InvalidOwnershipFile);
    }
    Ok(text)
}

/// Lowercase copy of an ID.
fn lower(id: &str) -> String {
    id.to_ascii_lowercase()
}

/// Request body of an ownership or severity rule update; `None` for other targets.
pub(crate) fn request_body(target: &SetTarget) -> Option<serde_json::Value> {
    match target {
        SetTarget::IssueOwnership {
            project_id,
            rules,
            codeowners,
        } => {
            let mut body = serde_json::json!({ "project_id": project_id });
            for (name, value) in [("rules", rules), ("codeowners", codeowners)] {
                if let (Some(value), Some(object)) = (value, body.as_object_mut()) {
                    let _previous =
                        object.insert(name.to_owned(), serde_json::Value::from(value.as_str()));
                }
            }
            Some(body)
        }
        SetTarget::IssueRule { severity, .. } => Some(serde_json::json!({ "severity": severity })),
        SetTarget::LogRule {
            severity, ignored, ..
        } => Some(serde_json::json!({ "severity": severity, "ignored": ignored })),
        SetTarget::NotificationsRead => Some(serde_json::json!({})),
        SetTarget::IssueStatus { .. }
        | SetTarget::IssuePriority { .. }
        | SetTarget::IssueAssignee { .. }
        | SetTarget::IssueBulk(_) => None,
    }
}

/// Human list of an issue's suggested owners, or `None` when the shape is not the contract.
#[must_use]
pub fn owners_summary(value: &serde_json::Value) -> Option<String> {
    let issue = id(value, "issue_id")?;
    let owners = value.get("owners")?.as_array()?;
    let mut output = format!("Owners of issue {issue} ({})\n", owners.len());
    if let Some(suggested) = value
        .get("suggested_assignee")
        .filter(|value| !value.is_null())
    {
        let suggested = crate::http::terminal_safe(&who(suggested)?);
        output.push_str(format!("Suggested assignee: {suggested}\n").as_str());
    }
    for owner in owners {
        let reason = match text(owner, "source")? {
            "ownership_rule" => "ownership rule",
            "codeowners" => "CODEOWNERS",
            "recent_activity" => "recent activity",
            _ => return None,
        };
        let mut line = format!("- {}: {reason}", who(owner)?);
        if let (Some(rule), Some(number)) = (text(owner, "rule"), owner.get("line")?.as_u64()) {
            line.push_str(format!(" line {number} `{rule}`").as_str());
        }
        if let Some(matched) = text(owner, "matched") {
            line.push_str(format!(", matched {matched}").as_str());
        }
        output.push_str(&crate::http::terminal_safe(&line));
        output.push('\n');
    }
    let inputs = value.get("inputs")?;
    let count = |name: &str| inputs.get(name)?.as_u64();
    let route = if inputs.get("route_template")?.as_bool()? {
        "yes"
    } else {
        "no"
    };
    output.push_str(
        format!(
            "Checked: frame paths {}, modules {}, route template {route}, tags {}, ownership \
             rules {}, CODEOWNERS lines {}.\n",
            count("frame_paths")?,
            count("frame_modules")?,
            count("context_tags")?,
            count("ownership_rules")?,
            count("codeowners_rules")?,
        )
        .as_str(),
    );
    let project = id(value, "project_id")?;
    let next = match owners.first() {
        Some(owner) => format!("run logbrew issues assign {} {issue}", assignee_ref(owner)?),
        None => format!(
            "add rules with logbrew issues ownership set --project {project} --rules <file>"
        ),
    };
    output.push_str(format!("Next: {next}\n").as_str());
    Some(output)
}

/// Human view of a project's ownership rules and CODEOWNERS text.
#[must_use]
pub fn ownership_summary(value: &serde_json::Value) -> Option<String> {
    let project = id(value, "project_id")?;
    let mut output = format!("Ownership for project {project}\n");
    let updated = value.get("updated_at")?;
    if !updated.is_null() {
        let updated = updated
            .as_str()
            .filter(|time| crate::render::is_rfc3339_utc(time))?;
        output.push_str(format!("Updated: {updated}\n").as_str());
    }
    for (name, title) in [("rules", "Ownership rules"), ("codeowners", "CODEOWNERS")] {
        let body = value.get(name)?.as_str()?;
        let lines = body.lines().filter(|line| !line.trim().is_empty()).count();
        output.push_str(format!("{title} ({lines} lines)\n").as_str());
        for line in body.lines().filter(|line| !line.trim().is_empty()) {
            output.push_str(format!("  {}\n", crate::http::terminal_safe(line)).as_str());
        }
    }
    let unresolved = value.get("unresolved_owners")?.as_array()?;
    if !unresolved.is_empty() {
        let names = unresolved
            .iter()
            .map(|owner| owner.as_str())
            .collect::<Option<Vec<_>>>()?;
        let names = crate::http::terminal_safe(&names.join(", "));
        output.push_str(format!("Owners without project access: {names}\n").as_str());
    }
    Some(output)
}

/// Human list of an issue's activity, or `None` when the shape is not the contract.
#[must_use]
pub fn activity_summary(value: &serde_json::Value) -> Option<String> {
    let issue = id(value, "issue_id")?;
    let entries = value.get("activity")?.as_array()?;
    let mut output = format!("Activity of issue {issue} ({})\n", entries.len());
    for entry in entries {
        let at = text(entry, "at")?;
        let what = match (text(entry, "type")?, text(entry, "value")) {
            ("first_seen", _) => String::from("first seen"),
            ("status", Some(value)) => format!("status set to {value}"),
            ("priority", Some(value)) => format!("priority set to {value}"),
            ("severity_rule", Some(value)) => format!("severity rule set to {value}"),
            ("assigned", _) => match entry.get("team").filter(|team| !team.is_null()) {
                Some(team) => format!("assigned to {}", who(team)?),
                None => format!("assigned to {}", person(entry, "assignee")?),
            },
            ("unassigned", _) => String::from("unassigned"),
            _ => return None,
        };
        let by = if entry.get("actor_id")?.is_null() {
            String::new()
        } else {
            format!(" by {}", person(entry, "actor")?)
        };
        output.push_str(&crate::http::terminal_safe(&format!("- {at} {what}{by}")));
        output.push('\n');
    }
    if let Some(cursor) = value.get("next_cursor").filter(|cursor| !cursor.is_null()) {
        output.push_str(
            format!(
                "More: run logbrew issues activity {issue} --cursor-time {} --cursor-id {}\n",
                text(cursor, "time").filter(|time| crate::render::is_rfc3339_utc(time))?,
                id(cursor, "id")?
            )
            .as_str(),
        );
    }
    Some(output)
}

/// Human summary of an issue or log severity rule response.
#[must_use]
pub fn rule_summary(kind: &str, value: &serde_json::Value) -> Option<String> {
    let signal = id(value, "id")?;
    let triage = value.get("triage")?;
    let effective = text(triage, "effective_severity")?;
    let captured = text(value, "severity")?;
    let mut output = format!("{kind} {signal} severity {effective} (captured {captured})");
    if triage.get("ignored")?.as_bool()? {
        output.push_str(", ignored");
    }
    output.push('.');
    Some(format!("{}\n", crate::http::terminal_safe(&output)))
}

/// The display label of the account in `field`, or its account ID when it lost access.
fn person(entry: &serde_json::Value, field: &str) -> Option<String> {
    let profile = entry.get(field)?;
    if profile.is_null() {
        let id = text(entry, &format!("{field}_id"))?;
        return Some(format!("{id} (no access)"));
    }
    account(profile)
}

/// A team as `team #slug`, or an account as [`account`] shows it.
fn who(value: &serde_json::Value) -> Option<String> {
    match crate::issue_triage::team_parts(value) {
        Some((_team_id, slug, _name)) => Some(format!("team #{slug}")),
        None => account(value),
    }
}

/// The value `logbrew issues assign` takes for an owner: `team:<team_id>` or a user ID.
fn assignee_ref(owner: &serde_json::Value) -> Option<String> {
    match crate::issue_triage::team_parts(owner) {
        Some((team_id, _slug, _name)) => Some(format!("team:{team_id}")),
        None => id(owner, "user_id").map(str::to_owned),
    }
}

/// `name <email>`, or `<email>` without a name, after checking the user ID.
fn account(value: &serde_json::Value) -> Option<String> {
    let _user_id = id(value, "user_id")?;
    let email = text(value, "email")?;
    let name = value.get("name")?;
    Some(if name.is_null() {
        format!("<{email}>")
    } else {
        format!("{} <{email}>", name.as_str()?)
    })
}

/// One UUID field.
fn id<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    text(value, name).filter(|id| is_uuid(id))
}

/// One string field.
fn text<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    value.get(name)?.as_str()
}

/// Human list of the caller's notifications, or `None` when the shape is not the contract.
#[must_use]
pub fn inbox_summary(value: &serde_json::Value) -> Option<String> {
    let notifications = value.get("notifications")?.as_array()?;
    let unread = value.get("unread")?.as_u64()?;
    let mut output = format!("Inbox ({}, {unread} unread)\n", notifications.len());
    for notification in notifications {
        let at = text(notification, "at").filter(|time| crate::render::is_rfc3339_utc(time))?;
        let issue = id(notification, "issue_id")?;
        let title = notification.get("title")?;
        let title = if title.is_null() {
            "(issue removed)"
        } else {
            title.as_str()?
        };
        let assignee = notification.get("assignee")?;
        let to = match crate::issue_triage::team_parts(assignee) {
            Some((_team_id, slug, _name)) => format!("team #{slug}"),
            None => String::from("you"),
        };
        let by = person(notification, "actor")?;
        let new = if notification.get("read")?.as_bool()? {
            ""
        } else {
            " [new]"
        };
        let line = format!("- {at}{new} {title} ({issue}) assigned to {to} by {by}");
        output.push_str(&crate::http::terminal_safe(&line));
        output.push('\n');
    }
    if let Some(cursor) = value.get("next_cursor").filter(|cursor| !cursor.is_null()) {
        output.push_str(
            format!(
                "More: run logbrew inbox --cursor-time {} --cursor-id {}\n",
                text(cursor, "time").filter(|time| crate::render::is_rfc3339_utc(time))?,
                id(cursor, "id")?
            )
            .as_str(),
        );
    }
    if unread > 0 {
        output.push_str("Next: run logbrew inbox read\n");
    }
    Some(output)
}

/// Human summary of marking notifications read.
#[must_use]
pub fn inbox_read_summary(value: &serde_json::Value) -> Option<String> {
    let read_at = text(value, "read_at").filter(|time| crate::render::is_rfc3339_utc(time))?;
    let unread = value.get("unread")?.as_u64()?;
    Some(format!("Inbox read up to {read_at}; {unread} unread.\n"))
}
