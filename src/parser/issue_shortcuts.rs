//! Issue status shortcut parsing.

use super::{
    ISSUE_STATUS_ARGUMENT_NEXT_STEP, move_leading_json_to_tail, take_required_position,
    unknown_command,
};
use crate::flags::{FlagScope, normalize_status, parse_flags};
use crate::{CliError, Command, SetTarget};

/// Parses top-level issue status shortcuts.
pub(super) fn parse_issue_status_shortcut(
    action: &str,
    args: &[String],
) -> Result<Command, CliError> {
    let (id, tail) =
        take_required_position(args, "issue_id", issue_status_missing_id_next_step(action))?;
    let Some((status, scope)) = issue_status_action(action) else {
        return Err(unknown_command(action));
    };
    let flags = parse_flags(tail.as_slice(), scope)?;

    Ok(Command::Set {
        target: SetTarget::IssueStatus {
            id,
            status: status.to_owned(),
        },
        json: flags.is_json(),
    })
}

/// Parses `resolved <issue_id>` and other status-first issue-id mutations.
pub(super) fn parse_status_first_issue_id_shortcut(
    status: &str,
    args: &[String],
) -> Result<Command, CliError> {
    let (id, tail) = take_required_position(args, "issue_id", "provide an issue id")?;
    let scope = status_first_issue_scope(status);
    let status = normalize_status(status)?;
    let flags = parse_flags(tail.as_slice(), scope)?;

    Ok(Command::Set {
        target: SetTarget::IssueStatus { id, status },
        json: flags.is_json(),
    })
}

/// Parses bare issue status words as recoverable missing-target errors.
pub(super) fn parse_bare_issue_status_shortcut(
    status: &str,
    args: &[String],
) -> Result<Command, CliError> {
    let scope = status_first_issue_scope(status);
    let _flags = parse_flags(args, scope)?;

    Err(CliError::MissingArgument {
        argument: "issue_id_or_issues",
        next: bare_issue_status_next_step(status),
    })
}

/// Returns whether an issue detail tail starts with a status mutation alias.
pub(super) fn has_issue_status_action(args: &[String]) -> bool {
    move_leading_json_to_tail(args)
        .first()
        .is_some_and(|action| issue_status_mutation_alias(action).is_some())
}

/// Parses `issue <issue_id> resolve|ignore|reopen|<status>`.
pub(super) fn parse_issue_first_status_shortcut(
    id: String,
    args: &[String],
) -> Result<Command, CliError> {
    let (action, tail) = take_required_position(args, "status", ISSUE_STATUS_ARGUMENT_NEXT_STEP)?;
    let Some((status, scope)) = issue_status_mutation(action.as_str()) else {
        return Err(unknown_command(action.as_str()));
    };
    let flags = parse_flags(tail.as_slice(), scope)?;

    Ok(Command::Set {
        target: SetTarget::IssueStatus { id, status },
        json: flags.is_json(),
    })
}

/// Returns whether a word is an issue-id status mutation alias.
pub(super) fn is_issue_status_action_alias(action: &str) -> bool {
    issue_status_mutation_alias(action).is_some()
}

/// Maps issue action shortcuts to canonical statuses and flag scopes.
fn issue_status_action(action: &str) -> Option<(&'static str, FlagScope)> {
    let value = match action {
        "resolve" => ("resolved", FlagScope::Resolve),
        "close" => ("resolved", FlagScope::Close),
        "ignore" => ("ignored", FlagScope::Ignore),
        "reopen" => ("unresolved", FlagScope::Reopen),
        _ => return None,
    };
    Some(value)
}

/// Returns recovery for top-level issue mutation shortcuts without ids.
fn issue_status_missing_id_next_step(action: &str) -> &'static str {
    match action {
        "resolve" => "use logbrew resolve <issue_id>",
        "close" => "use logbrew close <issue_id>",
        "ignore" => "use logbrew ignore <issue_id>",
        "reopen" => "use logbrew reopen <issue_id>",
        _ => "provide an issue id",
    }
}

/// Maps status-first issue shortcut words to their flag recovery scope.
fn status_first_issue_scope(status: &str) -> FlagScope {
    match status.to_ascii_lowercase().as_str() {
        "resolved" => FlagScope::StatusResolved,
        "closed" => FlagScope::StatusClosed,
        "ignored" => FlagScope::StatusIgnored,
        "open" => FlagScope::StatusOpen,
        "unresolved" => FlagScope::StatusUnresolved,
        _ => FlagScope::Set,
    }
}

/// Returns recovery for bare status words such as `open`.
fn bare_issue_status_next_step(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "resolved" => "use logbrew resolved <issue_id> or logbrew resolved issues",
        "closed" => "use logbrew closed <issue_id> or logbrew closed issues",
        "ignored" => "use logbrew ignored <issue_id> or logbrew ignored issues",
        "open" => "use logbrew open <issue_id> or logbrew open issues",
        "unresolved" => "use logbrew unresolved <issue_id> or logbrew unresolved issues",
        _ => "use logbrew <status> <issue_id> or logbrew <status> issues",
    }
}

/// Maps issue-id mutation aliases to canonical statuses and flag scopes.
fn issue_status_mutation(action: &str) -> Option<(String, FlagScope)> {
    issue_status_action(action)
        .map(|(status, scope)| (status.to_owned(), scope))
        .or_else(|| {
            normalize_status(action)
                .ok()
                .map(|status| (status, status_first_issue_scope(action)))
        })
}

/// Maps issue-id mutation aliases to canonical statuses.
fn issue_status_mutation_alias(action: &str) -> Option<String> {
    issue_status_mutation(action).map(|(status, _scope)| status)
}
