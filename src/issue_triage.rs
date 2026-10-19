//! `logbrew issues archive|resolve|reopen|priority|assign|unassign`: change one issue, or up to
//! 100 issues of one project in a single request. `logbrew issues assignees` lists who can be
//! assigned.

use crate::ids::is_uuid;
use crate::{CliError, Command, ReadTarget, SetTarget};

/// Most issues one bulk request may name.
const MAX_ISSUES: usize = 100;

/// Status, priority, or assignee change for several issues of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueBulkOptions {
    /// Project that owns every issue.
    pub project_id: String,
    /// Distinct issue IDs, in the order given.
    pub issue_ids: Vec<String>,
    /// New status: `archived`, `resolved`, or `unresolved`.
    pub status: Option<String>,
    /// New priority: `high`, `medium`, or `low`.
    pub priority: Option<String>,
    /// New assignee: a user ID, `me`, or `none`.
    pub assignee: Option<String>,
}

/// Parses the closed grammar after `logbrew issues`.
///
/// # Errors
///
/// Returns [`CliError::InvalidIssueTriageCommand`] for any other form.
///
/// One issue without `--project` updates that issue. `--project` updates every named issue of
/// that project in one request and is required for more than one issue.
pub fn parse(args: &[String]) -> Result<Command, CliError> {
    let json = args.iter().filter(|arg| *arg == "--json").count();
    let mut words = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--json");
    let (status, priority, assignee) = match words.next() {
        Some("archive") => (Some("archived"), None, None),
        Some("resolve") => (Some("resolved"), None, None),
        Some("reopen") => (Some("unresolved"), None, None),
        Some("priority") => match words.next() {
            Some(level @ ("high" | "medium" | "low")) => (None, Some(level), None),
            _ => return Err(CliError::InvalidIssueTriageCommand),
        },
        Some("assign") => match words.next() {
            Some(user) if user == "me" || crate::ids::is_assignee_id(user) => {
                (None, None, Some(user))
            }
            _ => return Err(CliError::InvalidIssueTriageCommand),
        },
        Some("unassign") => (None, None, Some("none")),
        Some("assignees") => {
            return match (words.next(), words.next(), words.next()) {
                (Some("--project" | "--project-id"), Some(project), None)
                    if json < 2 && is_uuid(project) =>
                {
                    Ok(Command::Read {
                        target: ReadTarget::IssueAssignees(project.to_ascii_lowercase()),
                        options: Box::default(),
                        json: json == 1,
                    })
                }
                _ => Err(CliError::InvalidIssueTriageCommand),
            };
        }
        _ => return Err(CliError::InvalidIssueTriageCommand),
    };
    let mut project_id = None;
    let mut issue_ids: Vec<String> = Vec::new();
    while let Some(word) = words.next() {
        if matches!(word, "--project" | "--project-id") {
            let value = words.next().filter(|value| is_uuid(value));
            if project_id.is_some() || value.is_none() {
                return Err(CliError::InvalidIssueTriageCommand);
            }
            project_id = value.map(str::to_ascii_lowercase);
        } else if word.starts_with('-') || issue_ids.iter().any(|id| id.eq_ignore_ascii_case(word))
        {
            return Err(CliError::InvalidIssueTriageCommand);
        } else {
            issue_ids.push(word.to_owned());
        }
    }
    if json > 1 || issue_ids.is_empty() || issue_ids.len() > MAX_ISSUES {
        return Err(CliError::InvalidIssueTriageCommand);
    }
    let json = json == 1;
    let (status, priority, assignee) = (
        status.map(str::to_owned),
        priority.map(str::to_owned),
        assignee.map(str::to_ascii_lowercase),
    );
    let target = match (project_id, issue_ids.as_slice()) {
        (None, [id]) => {
            let id = id.clone();
            match (status, priority, assignee) {
                (Some(status), ..) => SetTarget::IssueStatus { id, status },
                (_, Some(priority), _) => SetTarget::IssuePriority { id, priority },
                (.., assignee) => SetTarget::IssueAssignee {
                    id,
                    assignee: assignee.unwrap_or_default(),
                },
            }
        }
        (Some(project_id), _) if issue_ids.iter().all(|id| is_uuid(id)) => {
            SetTarget::IssueBulk(IssueBulkOptions {
                project_id,
                issue_ids: issue_ids.iter().map(|id| id.to_ascii_lowercase()).collect(),
                status,
                priority,
                assignee,
            })
        }
        _ => return Err(CliError::InvalidIssueTriageCommand),
    };
    Ok(Command::Set { target, json })
}

/// Request body for a bulk update.
#[must_use]
pub fn request_body(options: &IssueBulkOptions) -> serde_json::Value {
    let mut body = serde_json::json!({
        "project_id": options.project_id,
        "issue_ids": options.issue_ids,
        "response_version": 2,
    });
    for (name, value) in [
        ("status", &options.status),
        ("priority", &options.priority),
        ("assignee", &options.assignee),
    ] {
        if let (Some(value), Some(object)) = (value, body.as_object_mut()) {
            let _previous = object.insert(name.to_owned(), serde_json::Value::from(value.as_str()));
        }
    }
    body
}

/// Human summary of a bulk update response, or `None` when its shape is not the contract.
#[must_use]
pub fn summary(value: &serde_json::Value) -> Option<String> {
    let project = value.get("project_id")?.as_str()?;
    let results = value.get("results")?.as_array()?;
    let mut counts = [0_usize; 3];
    let mut lines = String::new();
    for result in results {
        let id = result.get("issue_id")?.as_str()?;
        let outcome = result.get("outcome")?.as_str()?;
        let slot = ["updated", "unchanged", "not_found"]
            .iter()
            .position(|name| *name == outcome)?;
        counts[slot] += 1;
        lines.push_str(format!("  {id} {}", outcome.replace('_', " ")).as_str());
        if let Some(issue) = result.get("issue").filter(|issue| !issue.is_null()) {
            let status = issue.get("status")?.as_str()?;
            let priority = issue.get("priority")?.as_str()?;
            let assignee = assignee_label(issue)?;
            lines
                .push_str(format!(": {status}, priority {priority}, assignee {assignee}").as_str());
        }
        lines.push('\n');
    }
    let [updated, unchanged, not_found] = counts;
    Some(format!(
        "Project {project}: {updated} updated, {unchanged} unchanged, {not_found} not found.\n{lines}"
    ))
}

/// Human list of accounts that can be assigned, or `None` when its shape is not the contract.
#[must_use]
pub fn assignees_summary(value: &serde_json::Value) -> Option<String> {
    let project = value.get("project_id")?.as_str()?;
    let assignees = value.get("assignees")?.as_array()?;
    let mut output = format!("Assignees for project {project} ({})\n", assignees.len());
    for assignee in assignees {
        let (user_id, name, email) = account(assignee)?;
        let name = name.map(|name| format!("{name} ")).unwrap_or_default();
        let line = crate::http::terminal_safe(format!("{user_id} {name}<{email}>").as_str());
        output.push_str(format!("- {line}\n").as_str());
    }
    let teams = value.get("teams")?.as_array()?;
    output.push_str(format!("Teams ({})\n", teams.len()).as_str());
    for team in teams {
        let (team_id, slug, name) = team_parts(team)?;
        let line = crate::http::terminal_safe(format!("team:{team_id} #{slug} {name}").as_str());
        output.push_str(format!("- {line}\n").as_str());
    }
    Some(output)
}

/// Display label of an issue's assignee: name, else email, else user ID, or `unassigned`.
/// `None` when the issue has no `assignee` field or it is not the contract shape.
#[must_use]
pub fn assignee_label(issue: &serde_json::Value) -> Option<String> {
    let assignee = issue.get("assignee")?;
    if assignee.is_null() {
        return Some(String::from("unassigned"));
    }
    if assignee.get("type").and_then(serde_json::Value::as_str) == Some("team") {
        let (_team_id, slug, _name) = team_parts(assignee)?;
        return Some(crate::http::terminal_safe(format!("team #{slug}").as_str()));
    }
    let (user_id, name, email) = account(assignee)?;
    let label = [name.unwrap_or_default(), email]
        .into_iter()
        .find(|text| !text.trim().is_empty())
        .unwrap_or(user_id);
    Some(crate::http::terminal_safe(label))
}

/// Team ID, slug, and name of one team object.
pub(crate) fn team_parts(value: &serde_json::Value) -> Option<(&str, &str, &str)> {
    let team_id = value.get("team_id")?.as_str().filter(|id| is_uuid(id))?;
    Some((
        team_id,
        value.get("slug")?.as_str()?,
        value.get("name")?.as_str()?,
    ))
}

/// User ID, optional name, and email of one account object.
fn account(value: &serde_json::Value) -> Option<(&str, Option<&str>, &str)> {
    let user_id = value.get("user_id")?.as_str().filter(|id| is_uuid(id))?;
    let name = value.get("name")?;
    let name = if name.is_null() {
        None
    } else {
        Some(name.as_str()?)
    };
    Some((user_id, name, value.get("email")?.as_str()?))
}
