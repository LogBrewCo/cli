//! `logbrew issues archive|resolve|reopen|priority`: change one issue, or up to 100 issues of
//! one project in a single request.

use crate::ids::is_uuid;
use crate::{CliError, Command, SetTarget};

/// Most issues one bulk request may name.
const MAX_ISSUES: usize = 100;

/// Status or priority change for several issues of one project.
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
    let (status, priority) = match words.next() {
        Some("archive") => (Some("archived"), None),
        Some("resolve") => (Some("resolved"), None),
        Some("reopen") => (Some("unresolved"), None),
        Some("priority") => match words.next() {
            Some(level @ ("high" | "medium" | "low")) => (None, Some(level)),
            _ => return Err(CliError::InvalidIssueTriageCommand),
        },
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
    let (status, priority) = (status.map(str::to_owned), priority.map(str::to_owned));
    let target = match (project_id, issue_ids.as_slice()) {
        (None, [id]) => priority.map_or_else(
            || SetTarget::IssueStatus {
                id: id.clone(),
                status: status.unwrap_or_default(),
            },
            |priority| SetTarget::IssuePriority {
                id: id.clone(),
                priority,
            },
        ),
        (Some(project_id), _) if issue_ids.iter().all(|id| is_uuid(id)) => {
            SetTarget::IssueBulk(IssueBulkOptions {
                project_id,
                issue_ids: issue_ids.iter().map(|id| id.to_ascii_lowercase()).collect(),
                status,
                priority,
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
    });
    for (name, value) in [("status", &options.status), ("priority", &options.priority)] {
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
            lines.push_str(format!(": {status}, priority {priority}").as_str());
        }
        lines.push('\n');
    }
    let [updated, unchanged, not_found] = counts;
    Some(format!(
        "Project {project}: {updated} updated, {unchanged} unchanged, {not_found} not found.\n{lines}"
    ))
}
