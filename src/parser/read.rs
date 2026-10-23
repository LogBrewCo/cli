//! Grammar for historical reads, read verbs, and search shortcuts.

use super::explain::parse_issue_investigation_flags;
use super::issue_shortcuts::{has_issue_status_action, parse_issue_first_status_shortcut};
use super::log_shortcuts::log_shortcut_args;
use super::trace_reads::{parse_trace_detail_or_explain, parse_trace_list_read};
use super::{
    has_position_candidate, has_trace_id_candidate, move_leading_json_to_tail,
    take_required_position, unknown_command, unknown_flag, unknown_resource,
};
use crate::flags::{
    FlagScope, is_read_filter_word, is_simple_flag, normalize_log_level, normalize_status,
    parse_flags, validate_min_duration,
};
use crate::ids::{infer_explain_target, is_issue_id, is_trace_id};
use crate::{CliError, Command, ExplainTarget, IssueOccurrenceSelection, ReadOptions, ReadTarget};

/// Valid resources for historical reads.
const READ_RESOURCE_NEXT_STEP: &str =
    "choose one of logs, issues, actions, releases, metrics, traces, trace, issue";
/// Recovery hint for users who type plural trace resources.
const READ_TRACE_ALIAS_NEXT_STEP: &str =
    "use singular trace with an id: logbrew read trace <trace_id>";

/// Help for trace detail reads.
pub(super) const READ_TRACE_NEXT_STEP: &str = "run logbrew read trace --help";
/// Help for issue detail reads.
const READ_ISSUE_NEXT_STEP: &str = "run logbrew read issue --help";
/// Help for log list reads.
const READ_LOGS_NEXT_STEP: &str = "run logbrew read logs --help";
/// Recovery hint for natural log search shortcuts.
const SEARCH_NEXT_STEP: &str = "provide search text or run logbrew logs --help";
/// Help for issue list reads.
const READ_ISSUES_NEXT_STEP: &str = "run logbrew read issues --help";
/// Help for action list reads.
const READ_ACTIONS_NEXT_STEP: &str = "run logbrew read actions --help";
/// Help for release list reads.
const READ_RELEASES_NEXT_STEP: &str = "run logbrew read releases --help";
/// Help for metric sample reads.
const READ_METRICS_NEXT_STEP: &str = "run logbrew read metrics --help";
/// Help for recent trace discovery.
pub(super) const READ_TRACES_NEXT_STEP: &str = "run logbrew read traces --help";

/// Builds an unknown read resource error with common-term recovery guidance.
pub(super) fn unknown_read_resource(resource: &str) -> CliError {
    unknown_resource(resource, read_resource_next_step(resource))
}

/// Returns the next step for unsupported read resources.
fn read_resource_next_step(resource: &str) -> &'static str {
    match resource {
        "trace" | "traces" | "span" | "spans" => READ_TRACE_ALIAS_NEXT_STEP,
        _ => READ_RESOURCE_NEXT_STEP,
    }
}

/// Parses `read`.
pub(super) fn parse_read(args: &[String]) -> Result<Command, CliError> {
    let (resource, rest) = take_required_position(args, "resource", READ_RESOURCE_NEXT_STEP)?;
    let resource = normalize_read_resource(resource.as_str());
    if is_recency_read_verb(resource) {
        return parse_read_verb(resource, rest.as_slice());
    }
    if is_known_issue_status(resource) && has_status_first_issue_resource_candidate(rest.as_slice())
    {
        return parse_status_first_issue_read(resource, rest.as_slice());
    }
    parse_read_resource(resource, rest.as_slice())
}

/// Normalizes safe singular collection words behind `read`.
fn normalize_read_resource(resource: &str) -> &str {
    match resource {
        "log" => "logs",
        "release" => "releases",
        _ => resource,
    }
}

/// Parses natural read-only verbs such as `show logs`.
pub(super) fn parse_read_verb(verb: &str, args: &[String]) -> Result<Command, CliError> {
    let rewritten_args = recency_count_shortcut_args(verb, args);
    let args = rewritten_args.as_deref().unwrap_or(args);
    let (resource, rest) = take_required_position(args, "resource", READ_RESOURCE_NEXT_STEP)?;
    if is_known_issue_status(resource.as_str())
        && has_status_first_issue_resource_candidate(rest.as_slice())
    {
        return parse_status_first_issue_read(resource.as_str(), rest.as_slice());
    }
    let resource = normalize_read_verb_resource(verb, resource.as_str());
    parse_read_resource(resource, rest.as_slice())
}

/// Rewrites `last 10 logs` to `last logs --limit 10`.
fn recency_count_shortcut_args(verb: &str, args: &[String]) -> Option<Vec<String>> {
    if !is_recency_read_verb(verb) {
        return None;
    }
    let normalized = move_leading_json_to_tail(args);
    let (count, tail) = normalized.split_first().filter(|(count, tail)| {
        !tail.is_empty() && count.chars().all(|char| char.is_ascii_digit())
    })?;
    let mut rewritten = Vec::with_capacity(normalized.len() + 2);
    rewritten.push(tail[0].clone());
    let rest = &tail[1..];
    if let Some(separator_index) = rest.iter().position(|arg| arg == "--") {
        rewritten.extend(rest[..separator_index].iter().cloned());
        rewritten.push(String::from("--limit"));
        rewritten.push(count.clone());
        rewritten.extend(rest[separator_index..].iter().cloned());
        return Some(rewritten);
    }
    rewritten.extend(rest.iter().cloned());
    rewritten.push(String::from("--limit"));
    rewritten.push(count.clone());
    Some(rewritten)
}

/// Returns whether a command is a natural read-only verb.
pub(super) fn is_read_verb(value: &str) -> bool {
    matches!(value, "show" | "list" | "get") || is_recency_read_verb(value)
}

/// Returns whether a command is a recency-flavored read alias.
pub(super) fn is_recency_read_verb(value: &str) -> bool {
    matches!(value, "latest" | "recent" | "last" | "newest")
}

/// Normalizes singular collection words behind natural read verbs.
fn normalize_read_verb_resource<'a>(verb: &str, resource: &'a str) -> &'a str {
    match (verb, resource) {
        ("list" | "show" | "get", "log") => "logs",
        (alias, "log") if is_recency_read_verb(alias) => "logs",
        (alias, "issue") if is_recency_read_verb(alias) => "issues",
        ("list", "issue") => "issues",
        ("list" | "show" | "get", "release") => "releases",
        (alias, "release") if is_recency_read_verb(alias) => "releases",
        ("list" | "show" | "get", "metric") => "metrics",
        (alias, "metric") if is_recency_read_verb(alias) => "metrics",
        _ => resource,
    }
}

/// Returns whether a command is a natural log search shortcut.
pub(super) fn is_log_search_shortcut(command: &str) -> bool {
    matches!(command, "search" | "find" | "grep")
}

/// Returns the static argument label for a natural log search shortcut.
pub(super) fn log_search_shortcut_label(command: &str) -> &'static str {
    match command {
        "find" => "find",
        "grep" => "grep",
        _ => "search",
    }
}

/// Parses natural log search shortcuts as `logs --search <text>`.
pub(super) fn parse_search_shortcut(
    label: &'static str,
    args: &[String],
) -> Result<Command, CliError> {
    let (query, tail) = take_search_query(args, label)?;
    let mut rest = Vec::with_capacity(tail.len() + 2);
    if query.starts_with('-') {
        rest.push(format!("--search={query}"));
    } else {
        rest.push(String::from("--search"));
        rest.push(query);
    }
    rest.extend(tail);
    parse_read_resource("logs", rest.as_slice())
}

/// Takes leading search text, allowing unquoted multi-word query shortcuts.
fn take_search_query(
    args: &[String],
    argument: &'static str,
) -> Result<(String, Vec<String>), CliError> {
    let normalized = move_leading_json_to_tail(args);
    if normalized.first().is_some_and(|arg| arg == "--") {
        return take_separator_search_query(normalized.as_slice(), argument);
    }
    let query_word_count = normalized
        .iter()
        .take_while(|arg| !arg.starts_with('-'))
        .count();
    if query_word_count == 0 {
        return Err(CliError::MissingArgument {
            argument,
            next: SEARCH_NEXT_STEP,
        });
    }
    let query = normalized[..query_word_count].join(" ");
    let tail = normalized[query_word_count..].to_vec();
    Ok((query, tail))
}

/// Takes search text after `--`, allowing literal flag-looking terms.
fn take_separator_search_query(
    args: &[String],
    argument: &'static str,
) -> Result<(String, Vec<String>), CliError> {
    let words = &args[1..];
    if words.is_empty() {
        return Err(CliError::MissingArgument {
            argument,
            next: SEARCH_NEXT_STEP,
        });
    }
    let has_trailing_json_mode = words.len() > 1 && words.last().is_some_and(|arg| arg == "--json");
    let query_end = if has_trailing_json_mode {
        words.len() - 1
    } else {
        words.len()
    };
    let query = words[..query_end].join(" ");
    let tail = if has_trailing_json_mode {
        vec![String::from("--json")]
    } else {
        Vec::new()
    };
    Ok((query, tail))
}

/// Parses `read` resource arguments or top-level read shortcuts.
pub(super) fn parse_read_resource(resource: &str, rest: &[String]) -> Result<Command, CliError> {
    let (target, flags) = match resource {
        "logs" => parse_log_list_read(rest)?,
        alias if is_issue_collection_alias(alias) && has_issue_id_candidate(rest) => {
            return parse_issue_detail_or_status(rest);
        }
        alias if is_issue_collection_alias(alias) => parse_issue_list_read(rest)?,
        alias if is_action_collection_alias(alias) => parse_action_list_read(rest)?,
        "releases" => parse_list_read(
            ReadTarget::Releases,
            rest,
            "read releases",
            READ_RELEASES_NEXT_STEP,
        )?,
        "metrics" | "metric" => parse_list_read(
            ReadTarget::Metrics,
            rest,
            "read metrics",
            READ_METRICS_NEXT_STEP,
        )?,
        "traces" | "spans" if has_trace_id_candidate(rest) => {
            return parse_trace_detail_or_explain(rest);
        }
        "traces" | "spans" => parse_trace_list_read(rest)?,
        "trace" => return parse_trace_detail_or_explain(rest),
        "span" if has_position_candidate(rest) => {
            return parse_trace_detail_or_explain(rest);
        }
        "issue" if has_issue_status_candidate(rest) => parse_issue_list_read(rest)?,
        "issue" => return parse_issue_detail_or_status(rest),
        other => return Err(unknown_read_resource(other)),
    };
    let json = flags.is_json();
    let options = flags.into_read_options();
    validate_read_filters(&target, &options)?;

    Ok(Command::Read {
        target,
        options: Box::new(options),
        json,
    })
}

/// Returns whether a resource word is an issue list alias.
pub(super) fn is_issue_collection_alias(value: &str) -> bool {
    matches!(
        value,
        "issues" | "errors" | "error" | "exceptions" | "exception"
    )
}

/// Returns whether a resource word can follow a status-first issue shortcut.
pub(super) fn is_status_first_issue_collection_alias(value: &str) -> bool {
    value == "issue" || is_issue_collection_alias(value)
}

/// Returns whether args begin with an issue collection after a status word.
pub(super) fn has_status_first_issue_resource_candidate(args: &[String]) -> bool {
    move_leading_json_to_tail(args)
        .first()
        .is_some_and(|arg| is_status_first_issue_collection_alias(arg))
}

/// Returns whether args begin with an issue status after optional `--json`.
fn has_issue_status_candidate(args: &[String]) -> bool {
    move_leading_json_to_tail(args)
        .first()
        .is_some_and(|arg| is_known_issue_status(arg))
}

/// Returns whether a resource word is an action list alias.
pub(super) fn is_action_collection_alias(value: &str) -> bool {
    matches!(value, "actions" | "events" | "event" | "action")
}

/// Parses log lists, accepting natural search and positional severity aliases.
fn parse_log_list_read(rest: &[String]) -> Result<(ReadTarget, crate::flags::Flags), CliError> {
    let args = log_shortcut_args(rest);
    parse_list_read(
        ReadTarget::Logs,
        args.as_slice(),
        "read logs",
        READ_LOGS_NEXT_STEP,
    )
}

/// Parses issue/error lists, accepting a first positional status word.
fn parse_issue_list_read(rest: &[String]) -> Result<(ReadTarget, crate::flags::Flags), CliError> {
    let args = issue_status_shortcut_args(rest);
    parse_list_read(
        ReadTarget::Issues,
        args.as_slice(),
        "read issues",
        READ_ISSUES_NEXT_STEP,
    )
}

/// Parses `open issues` as `issues --status unresolved`.
pub(super) fn parse_status_first_issue_read(
    status: &str,
    args: &[String],
) -> Result<Command, CliError> {
    let canonical_status = normalize_status(status)?;
    let (resource, rest) = take_required_position(args, "resource", READ_ISSUES_NEXT_STEP)?;
    let resource = resource.as_str();
    if !is_status_first_issue_collection_alias(resource) {
        return Err(unknown_read_resource(resource));
    }
    let resource = if resource == "issue" {
        "issues"
    } else {
        resource
    };
    let mut rewritten = Vec::with_capacity(rest.len() + 2);
    rewritten.push(String::from("--status"));
    rewritten.push(canonical_status);
    rewritten.extend(rest);
    parse_read_resource(resource, rewritten.as_slice())
}

/// Rewrites `issues open` to `issues --status unresolved`.
fn issue_status_shortcut_args(args: &[String]) -> Vec<String> {
    let normalized = move_leading_json_to_tail(args);
    let Some((status, tail)) = normalized
        .split_first()
        .and_then(|(status, tail)| normalize_status(status).ok().map(|value| (value, tail)))
    else {
        return args.to_vec();
    };
    let mut rewritten = Vec::with_capacity(normalized.len() + 2);
    rewritten.push(String::from("--status"));
    rewritten.push(status);
    rewritten.extend(tail.iter().cloned());
    rewritten
}

/// Parses action/event lists, accepting a first positional as `--name`.
fn parse_action_list_read(rest: &[String]) -> Result<(ReadTarget, crate::flags::Flags), CliError> {
    let args = action_name_shortcut_args(rest);
    parse_list_read(
        ReadTarget::Actions,
        args.as_slice(),
        "read actions",
        READ_ACTIONS_NEXT_STEP,
    )
}

/// Rewrites `events checkout_failed` to `actions --name checkout_failed`.
fn action_name_shortcut_args(args: &[String]) -> Vec<String> {
    let normalized = move_leading_json_to_tail(args);
    let Some((name, tail)) = normalized
        .split_first()
        .filter(|(name, _)| !name.starts_with('-') && !is_read_filter_word(name))
    else {
        return args.to_vec();
    };
    let mut rewritten = Vec::with_capacity(normalized.len() + 2);
    rewritten.push(String::from("--name"));
    rewritten.push(name.clone());
    rewritten.extend(tail.iter().cloned());
    rewritten
}

/// Returns whether args start with an obvious issue id after optional `--json`.
pub(super) fn has_issue_id_candidate(args: &[String]) -> bool {
    move_leading_json_to_tail(args)
        .first()
        .is_some_and(|arg| is_issue_id(arg))
}

/// Parses issue detail reads and issue-first mutation shortcuts.
fn parse_issue_detail_or_status(args: &[String]) -> Result<Command, CliError> {
    let (id, tail) = take_required_position(args, "issue_id", "provide an issue id")?;
    if has_issue_status_action(tail.as_slice()) {
        return parse_issue_first_status_shortcut(id, tail.as_slice());
    }
    if let Some(command) = parse_detail_explain_suffix(
        ExplainTarget::Issue {
            id: id.clone(),
            occurrence: IssueOccurrenceSelection::Recommended,
        },
        tail.as_slice(),
    )? {
        return Ok(command);
    }
    let target = ReadTarget::Issue(id);
    let flags =
        parse_detail_read_flags(&target, tail.as_slice(), "read issue", READ_ISSUE_NEXT_STEP)?;
    let json = flags.is_json();
    let options = flags.into_read_options();
    validate_read_filters(&target, &options)?;

    Ok(Command::Read {
        target,
        options: Box::new(options),
        json,
    })
}

/// Parses a list read after rejecting filters the target cannot apply.
fn parse_list_read(
    target: ReadTarget,
    args: &[String],
    command: &'static str,
    next: &'static str,
) -> Result<(ReadTarget, crate::flags::Flags), CliError> {
    reject_unsupported_read_flags(&target, args, command, next)?;
    Ok((target, parse_flags(args, FlagScope::Read)?))
}

/// Rejects target-inapplicable read filters before parsing values.
pub(super) fn reject_unsupported_read_flags(
    target: &ReadTarget,
    args: &[String],
    command: &'static str,
    next: &'static str,
) -> Result<(), CliError> {
    let mut index = 0;
    let mut seen = Vec::new();
    while let Some(arg) = args.get(index) {
        let (flag, inline_value) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
        if !is_read_value_flag(flag) {
            if flag == "--json" && inline_value.is_none() {
                if seen.contains(&"--json") {
                    return Ok(());
                }
                seen.push("--json");
                index += 1;
                continue;
            }
            if inline_value.is_some() && is_simple_flag(flag) {
                return Err(CliError::UnsupportedFlag {
                    flag: arg.to_owned(),
                    command,
                    next,
                });
            }
            if arg.starts_with('-') {
                return Err(unknown_flag(arg, next));
            }
            return Ok(());
        }
        if !target.supports_filter(flag) {
            return Err(CliError::UnsupportedFlag {
                flag: user_facing_read_flag(flag).to_owned(),
                command,
                next,
            });
        }
        if let Some(canonical) = read_value_canonical_flag(flag) {
            if seen.contains(&canonical) {
                return Ok(());
            }
            seen.push(canonical);
        }
        if inline_value.is_some_and(str::is_empty) {
            return Ok(());
        }
        if inline_value.is_some_and(|value| has_invalid_supported_read_value(flag, value)) {
            return Ok(());
        }
        if inline_value.is_none() {
            let Some(value) = args.get(index + 1) else {
                return Ok(());
            };
            if value.starts_with('-') {
                return Ok(());
            }
            if has_invalid_supported_read_value(flag, value) {
                return Ok(());
            }
            index += 1;
        }
        index += 1;
    }
    Ok(())
}

/// Returns the duplicate-tracking key for a read value flag.
fn read_value_canonical_flag(flag: &str) -> Option<&'static str> {
    let canonical = match flag {
        "--name" => "--name",
        "--service" | "--service-name" => "--service",
        "--since" => "--since",
        "--until" => "--until",
        "--priority" => "--priority",
        "--assignee" => "--assignee",
        "--user" | "--distinct-id" => "--user",
        "--trace" | "--trace-id" => "--trace",
        "--level" | "--severity" => "--severity",
        "--search" => "--search",
        "--project" | "--project-id" => "--project",
        "--release" => "--release",
        "--environment" | "--env" => "--environment",
        "--status" => "--status",
        "--limit" => "--limit",
        "--min-duration-ms" => "--min-duration-ms",
        "--pagination" => "--pagination",
        "--cursor-time" => "--cursor-time",
        "--cursor-id" => "--cursor-id",
        _ => return None,
    };
    Some(canonical)
}

/// Returns the canonical flag name to show in read-filter recovery output.
fn user_facing_read_flag(flag: &str) -> &str {
    match flag {
        "--level" => "--severity",
        "--service-name" => "--service",
        other => other,
    }
}

/// Returns whether a supported read flag has a value that should be reported first.
fn has_invalid_supported_read_value(flag: &str, value: &str) -> bool {
    match flag {
        "--level" | "--severity" => !is_known_log_level(value),
        "--status" => !is_known_issue_status(value),
        "--limit" => value.parse::<u32>().map_or(true, |limit| limit == 0),
        "--min-duration-ms" => validate_min_duration(value).is_err(),
        "--pagination" => value != "cursor",
        _ => false,
    }
}

/// Returns whether a value is in the log-level vocabulary.
pub(super) fn is_known_log_level(value: &str) -> bool {
    normalize_log_level(value).is_ok()
}

/// Returns whether a positional log search word should stay a recoverable error.
pub(super) fn is_ambiguous_log_search_word(value: &str) -> bool {
    is_read_filter_word(value) || value.contains('@') || is_trace_id(value)
}

/// Returns whether a value is in the issue-status vocabulary.
pub(super) fn is_known_issue_status(value: &str) -> bool {
    normalize_status(value).is_ok()
}

/// Returns whether a flag is a value-taking read filter.
fn is_read_value_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--name"
            | "--service"
            | "--service-name"
            | "--since"
            | "--until"
            | "--priority"
            | "--assignee"
            | "--user"
            | "--distinct-id"
            | "--trace"
            | "--trace-id"
            | "--level"
            | "--severity"
            | "--search"
            | "--project"
            | "--project-id"
            | "--release"
            | "--environment"
            | "--env"
            | "--status"
            | "--limit"
            | "--min-duration-ms"
            | "--pagination"
            | "--cursor-time"
            | "--cursor-id"
    )
}

/// Parses a trailing `explain` action after an issue or trace detail id.
pub(super) fn parse_detail_explain_suffix(
    target: ExplainTarget,
    args: &[String],
) -> Result<Option<Command>, CliError> {
    let normalized = move_leading_json_to_tail(args);
    if normalized.first().is_none_or(|arg| arg != "explain") {
        return Ok(None);
    }
    Ok(Some(parse_explain_target_flags(target, &normalized[1..])?))
}

/// Parses target-specific explanation flags while keeping trace/log behavior unchanged.
pub(super) fn parse_explain_target_flags(
    target: ExplainTarget,
    args: &[String],
) -> Result<Command, CliError> {
    if let ExplainTarget::Issue { id, .. } = target {
        let (occurrence, json) = parse_issue_investigation_flags(args)?;
        return Ok(Command::Explain {
            target: ExplainTarget::Issue { id, occurrence },
            json,
        });
    }
    Ok(Command::Explain {
        target,
        json: parse_flags(args, FlagScope::Explain)?.is_json(),
    })
}

/// Parses detail read filters after rejecting list-only filters.
pub(super) fn parse_detail_read_flags(
    target: &ReadTarget,
    args: &[String],
    command: &'static str,
    next: &'static str,
) -> Result<crate::flags::Flags, CliError> {
    reject_unsupported_read_flags(target, args, command, next)?;
    parse_flags(args, FlagScope::Read)
}

/// Parses an obvious pasted issue or trace id as a detail read shortcut.
pub(super) fn parse_pasted_detail_id(id: &str, args: &[String]) -> Result<Command, CliError> {
    if is_issue_id(id) && has_issue_status_action(args) {
        return parse_issue_first_status_shortcut(id.to_owned(), args);
    }
    let explain_args = move_leading_json_to_tail(args);
    if explain_args.first().is_some_and(|arg| arg == "explain") {
        let target = infer_explain_target(id).ok_or_else(|| unknown_command(id))?;
        return parse_explain_target_flags(target, &explain_args[1..]);
    }
    let (target, command, next) = if is_trace_id(id) {
        (
            ReadTarget::Trace(id.to_owned()),
            "read trace",
            READ_TRACE_NEXT_STEP,
        )
    } else if is_issue_id(id) {
        (
            ReadTarget::Issue(id.to_owned()),
            "read issue",
            READ_ISSUE_NEXT_STEP,
        )
    } else {
        return Err(unknown_command(id));
    };
    let flags = parse_detail_read_flags(&target, args, command, next)?;
    let json = flags.is_json();
    let options = flags.into_read_options();
    validate_read_filters(&target, &options)?;

    Ok(Command::Read {
        target,
        options: Box::new(options),
        json,
    })
}

/// Rejects filters that a read endpoint would otherwise ignore.
pub(super) fn validate_read_filters(
    target: &ReadTarget,
    filters: &ReadOptions,
) -> Result<(), CliError> {
    let (command, next) = match target {
        ReadTarget::Logs => ("read logs", READ_LOGS_NEXT_STEP),
        ReadTarget::Issues => ("read issues", READ_ISSUES_NEXT_STEP),
        ReadTarget::Actions => ("read actions", READ_ACTIONS_NEXT_STEP),
        ReadTarget::Releases => ("read releases", READ_RELEASES_NEXT_STEP),
        ReadTarget::Metrics => ("read metrics", READ_METRICS_NEXT_STEP),
        ReadTarget::Traces => ("read traces", READ_TRACES_NEXT_STEP),
        ReadTarget::Trace(_) => ("read trace", READ_TRACE_NEXT_STEP),
        ReadTarget::Issue(_) => ("read issue", READ_ISSUE_NEXT_STEP),
        ReadTarget::IssueAssignees(_) => ("issues assignees", READ_ISSUES_NEXT_STEP),
        ReadTarget::IssueOwners(_) => ("issues owners", READ_ISSUES_NEXT_STEP),
        ReadTarget::IssueActivity(_) => ("issues activity", READ_ISSUES_NEXT_STEP),
        ReadTarget::IssueOwnership(_) => ("issues ownership", READ_ISSUES_NEXT_STEP),
        ReadTarget::Notifications => ("inbox", READ_ISSUES_NEXT_STEP),
    };
    if let Some(flag) = filters.first_unsupported_flag(target) {
        return Err(CliError::UnsupportedFlag {
            flag: flag.to_owned(),
            command,
            next,
        });
    }
    match target {
        ReadTarget::Logs => validate_read_cursor(filters, CliError::InvalidLogCursor)?,
        ReadTarget::Actions => validate_read_cursor(filters, CliError::InvalidActionCursor)?,
        ReadTarget::Issues => validate_read_cursor(filters, CliError::InvalidIssueCursor)?,
        ReadTarget::Metrics => validate_read_cursor(filters, CliError::InvalidMetricCursor)?,
        ReadTarget::Traces => validate_read_cursor(filters, CliError::InvalidTraceCursor)?,
        ReadTarget::Releases
        | ReadTarget::Trace(_)
        | ReadTarget::Issue(_)
        | ReadTarget::IssueAssignees(_)
        | ReadTarget::IssueOwners(_)
        | ReadTarget::IssueActivity(_)
        | ReadTarget::IssueOwnership(_)
        | ReadTarget::Notifications => {}
    }
    Ok(())
}

/// Validates an explicit first-page or continuation cursor shape.
fn validate_read_cursor(
    filters: &ReadOptions,
    invalid_cursor: fn(String) -> CliError,
) -> Result<(), CliError> {
    match (
        filters.pagination.as_deref(),
        filters.cursor_time.as_ref(),
        filters.cursor_id.as_ref(),
    ) {
        (None | Some("cursor"), None, None) | (Some("cursor"), Some(_), Some(_)) => Ok(()),
        (None, _, _) => Err(invalid_cursor(String::from(
            "cursor fields require --pagination cursor",
        ))),
        (Some("cursor"), _, _) => Err(invalid_cursor(String::from(
            "--cursor-time and --cursor-id must be used together",
        ))),
        (Some(_), _, _) => Err(CliError::UnknownPagination),
    }
}
