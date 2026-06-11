//! CLI command grammar.

mod help_topics;
mod issue_shortcuts;
mod log_shortcuts;
mod watch;

use help_topics::{
    command_shaped_help_topic, contains_help_flag, ensure_no_help_positionals, help_command,
    help_topic, is_direct_filter_help_alias, is_help_flag, parse_help, parse_help_alias,
    parse_literal_help, positional_args, validate_help_flags,
};
use issue_shortcuts::{
    has_issue_status_action, is_issue_status_action_alias, parse_bare_issue_status_shortcut,
    parse_issue_first_status_shortcut, parse_issue_status_shortcut,
    parse_status_first_issue_id_shortcut,
};
use log_shortcuts::{literal_log_search_separator_index, log_shortcut_args};
use watch::parse_watch;

use crate::flags::{
    FlagScope, is_read_filter_word, is_simple_flag, normalize_log_level, normalize_status,
    parse_flags,
};
use crate::ids::{infer_explain_target, is_issue_id, is_pasted_detail_id, is_trace_id};
use crate::{
    CliError, Command, ExplainTarget, HelpTopic, ISSUE_STATUS_ARGUMENT_NEXT_STEP, ReadOptions,
    ReadTarget, SetTarget, auth_namespace,
};

/// Standard next step for malformed help invocations.
const HELP_NEXT_STEP: &str = "run logbrew --help";
/// Valid resources for historical reads.
const READ_RESOURCE_NEXT_STEP: &str = "choose one of logs, issues, actions, releases, trace, issue";
/// Recovery hint for users who type plural trace resources.
const READ_TRACE_ALIAS_NEXT_STEP: &str =
    "use singular trace with an id: logbrew read trace <trace_id>";
/// Recovery hint for users who type trace terminology as a top-level command.
const TRACE_COMMAND_NEXT_STEP: &str =
    "use logbrew trace <trace_id> or logbrew explain trace <trace_id>";
/// Help for trace detail reads.
const READ_TRACE_NEXT_STEP: &str = "run logbrew read trace --help";
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
/// Valid resources for live watch.
const WATCH_RESOURCE_NEXT_STEP: &str = "choose logs or actions";
/// Valid resources for explain.
const EXPLAIN_RESOURCE_NEXT_STEP: &str = "choose issue or trace";
/// Valid resources for state mutation.
const SET_RESOURCE_NEXT_STEP: &str = "choose issue";
/// Filters trace detail reads cannot apply.
const TRACE_DETAIL_UNSUPPORTED_FLAGS: &[&str] = &[
    "--name",
    "--since",
    "--user",
    "--distinct-id",
    "--trace",
    "--trace-id",
    "--level",
    "--severity",
    "--search",
    "--status",
    "--limit",
];
/// Filters issue detail reads cannot apply.
const ISSUE_DETAIL_UNSUPPORTED_FLAGS: &[&str] = &[
    "--name",
    "--since",
    "--user",
    "--distinct-id",
    "--trace",
    "--trace-id",
    "--level",
    "--severity",
    "--search",
    "--project",
    "--project-id",
    "--release",
    "--environment",
    "--env",
    "--status",
    "--limit",
];
/// Filters action list reads cannot apply.
const ACTION_LIST_UNSUPPORTED_FLAGS: &[&str] = &[
    "--trace",
    "--trace-id",
    "--level",
    "--severity",
    "--search",
    "--status",
];

/// # Errors
/// Returns [`CliError`] if the command grammar is invalid.
pub fn parse_command<I, S>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let values = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    parse_values(values.as_slice())
}

/// Parses a collected argument slice.
fn parse_values(values: &[String]) -> Result<Command, CliError> {
    let args = values.get(1..).ok_or(CliError::UnknownCommand)?;
    let Some((head, tail)) = args.split_first() else {
        return Ok(Command::Help {
            topic: HelpTopic::Root,
            json: false,
        });
    };
    if is_help_flag(head) {
        validate_help_flags(tail)?;
        ensure_no_help_positionals(positional_args(tail).as_slice())?;
        return Ok(help_command(HelpTopic::Root, tail));
    }
    if head == "--json" {
        return parse_global_json(values, tail);
    }
    if is_version_flag(head) {
        return parse_version(tail);
    }
    if head.starts_with('-') {
        return Err(unknown_flag(head, HELP_NEXT_STEP));
    }
    if head == "help" {
        return parse_help(tail);
    }
    if let Some(command) = parse_literal_help(head, tail)? {
        return Ok(command);
    }
    if contains_help_flag(tail) && !is_log_search_separator_literal(head, tail) {
        validate_help_flags(tail)?;
        if let Some(topic) = command_shaped_help_topic(head, tail) {
            return Ok(help_command(topic, tail));
        }
        return Ok(help_command(help_topic(head, tail)?, tail));
    }
    match head.as_str() {
        "login" => parse_login(tail),
        "logout" => parse_logout(tail),
        alias if is_setup_alias(alias) => parse_setup(tail),
        "status" | "whoami" | "me" | "health" | "ping" | "doctor" => parse_status(tail),
        "version" => parse_version(tail),
        alias if auth_namespace::is_namespace(alias) => auth_namespace::parse(tail),
        alias if auth_namespace::is_help_alias(alias) => parse_help_alias(HelpTopic::Auth, tail),
        "json" | "output" => parse_help_alias(HelpTopic::Json, tail),
        alias if is_direct_filter_help_alias(alias) => parse_help_alias(HelpTopic::Read, tail),
        "read" => parse_read(tail),
        alias if is_read_verb(alias) => parse_read_verb(alias, tail),
        status if is_known_issue_status(status) && has_issue_id_candidate(tail) => {
            parse_status_first_issue_id_shortcut(status, tail)
        }
        status
            if is_known_issue_status(status) && has_status_first_issue_resource_candidate(tail) =>
        {
            parse_status_first_issue_read(status, tail)
        }
        status if is_known_issue_status(status) => parse_bare_issue_status_shortcut(status, tail),
        alias if is_log_search_shortcut(alias) => {
            parse_search_shortcut(log_search_shortcut_label(alias), tail)
        }
        "log" => parse_read_resource("logs", tail),
        "release" => parse_read_resource("releases", tail),
        alias if is_trace_term(alias) && !has_position_candidate(tail) => {
            parse_help_alias(HelpTopic::ReadTrace, tail)
        }
        "logs" | "issues" | "errors" | "error" | "exceptions" | "exception" | "actions"
        | "events" | "event" | "action" | "releases" | "trace" | "issue" => {
            parse_read_resource(head, tail)
        }
        "traces" | "span" | "spans" if has_position_candidate(tail) => {
            parse_read_resource("trace", tail)
        }
        "resolve" | "close" | "ignore" | "reopen" => parse_issue_status_shortcut(head, tail),
        alias if is_watch_command_alias(alias) => parse_watch(tail),
        "explain" => parse_explain(tail),
        "set" => parse_set(tail),
        id if is_pasted_detail_id(id) => parse_pasted_detail_id(id, tail),
        _ => Err(unknown_command(head)),
    }
}

/// Parses a leading global `--json` flag.
fn parse_global_json(values: &[String], tail: &[String]) -> Result<Command, CliError> {
    if global_json_tail_has_duplicate(tail) {
        return Err(CliError::DuplicateFlag {
            flag: "--json",
            next: "use --json once",
        });
    }
    if tail.is_empty() {
        return Ok(Command::Help {
            topic: HelpTopic::Root,
            json: true,
        });
    }

    let mut normalized = Vec::with_capacity(values.len());
    if let Some(program) = values.first() {
        normalized.push(program.clone());
    }
    normalized.extend(tail.iter().cloned());
    normalized.push(String::from("--json"));
    parse_values(normalized.as_slice())
}

/// Returns whether a global JSON command also contains a JSON mode flag.
fn global_json_tail_has_duplicate(tail: &[String]) -> bool {
    if !tail.iter().any(|arg| arg == "--json") {
        return false;
    }
    let Some((command, rest)) = tail.split_first() else {
        return false;
    };
    let Some(separator_index) = literal_log_search_separator_index(command, rest) else {
        return true;
    };
    rest[..separator_index].iter().any(|arg| arg == "--json")
}
/// Builds an unknown-resource error with command-specific recovery guidance.
fn unknown_resource(resource: &str, next: &'static str) -> CliError {
    CliError::UnknownResource {
        resource: resource.to_owned(),
        next,
    }
}

/// Builds an unknown-flag error with command-specific recovery guidance.
fn unknown_flag(flag: &str, next: &'static str) -> CliError {
    CliError::UnknownFlag {
        flag: flag.to_owned(),
        next,
    }
}

/// Builds an unknown read resource error with common-term recovery guidance.
fn unknown_read_resource(resource: &str) -> CliError {
    unknown_resource(resource, read_resource_next_step(resource))
}

/// Returns the next step for unsupported read resources.
fn read_resource_next_step(resource: &str) -> &'static str {
    match resource {
        "trace" | "traces" | "span" | "spans" => READ_TRACE_ALIAS_NEXT_STEP,
        _ => READ_RESOURCE_NEXT_STEP,
    }
}

/// Builds an unknown-command error with typo recovery guidance when available.
fn unknown_command(command: &str) -> CliError {
    CliError::UnknownCommandName {
        command: command.to_owned(),
        next: unknown_command_next_step(command),
    }
}

/// Returns a next step for common command typos.
fn unknown_command_next_step(command: &str) -> &'static str {
    match command {
        "logg" | "lgs" => "did you mean logbrew logs?",
        "action" | "event" | "events" => "did you mean logbrew actions?",
        "releaze" | "rels" => "did you mean logbrew releases?",
        "statuz" | "stats" => "did you mean logbrew status?",
        "error" | "errors" | "exception" | "exceptions" => "did you mean logbrew issues?",
        "trace" | "traces" | "span" | "spans" => TRACE_COMMAND_NEXT_STEP,
        "env" | "environment" | "environments" => {
            "use --environment <environment> with logs, issues, actions, releases, or traces"
        }
        alias if auth_namespace::is_help_alias(alias) => "run logbrew help auth",
        _ => HELP_NEXT_STEP,
    }
}

/// Returns whether a word should land on status/health help.
fn is_status_help_alias(value: &str) -> bool {
    matches!(value, "status" | "health" | "ping" | "doctor")
}

/// Returns whether a word should run the non-mutating setup plan.
fn is_setup_alias(value: &str) -> bool {
    matches!(value, "setup" | "init" | "install" | "configure" | "sdk")
}

/// Returns whether a word should use the live watch placeholder flow.
fn is_watch_command_alias(value: &str) -> bool {
    matches!(value, "watch" | "tail" | "follow" | "stream")
}

/// Returns whether a word names trace/span vocabulary.
fn is_trace_term(value: &str) -> bool {
    matches!(value, "trace" | "traces" | "span" | "spans")
}

/// Returns whether a value is a version flag.
fn is_version_flag(value: &str) -> bool {
    matches!(value, "--version" | "-V")
}

/// Parses `login`.
fn parse_login(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Login)?;
    let json = flags.is_json();
    Ok(Command::Login {
        open_browser: flags.should_open_browser() && !json,
        json,
    })
}

/// Parses `logout`.
fn parse_logout(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Logout)?;
    Ok(Command::Logout {
        json: flags.is_json(),
    })
}

/// Parses `setup`.
fn parse_setup(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Setup)?;
    Ok(Command::Setup {
        auto: flags.is_auto(),
        yes: flags.skip_prompts(),
        json: flags.is_json(),
    })
}

/// Parses `status`.
fn parse_status(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Status)?;
    Ok(Command::Status {
        json: flags.is_json(),
    })
}

/// Parses `version`.
fn parse_version(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Version)?;
    Ok(Command::Version {
        json: flags.is_json(),
    })
}

/// Takes one required positional argument and rejects flags in its place.
fn take_required_arg<'a>(
    args: &'a [String],
    argument: &'static str,
    next: &'static str,
) -> Result<(&'a str, &'a [String]), CliError> {
    let Some((value, rest)) = args.split_first() else {
        return Err(CliError::MissingArgument { argument, next });
    };
    if value.starts_with('-') {
        return Err(CliError::MissingArgument { argument, next });
    }
    Ok((value.as_str(), rest))
}

/// Moves a leading JSON flag behind required positional arguments.
fn move_leading_json_to_tail(args: &[String]) -> Vec<String> {
    if args.first().is_some_and(|arg| arg == "--json") {
        let mut normalized = Vec::with_capacity(args.len());
        normalized.extend(args[1..].iter().cloned());
        normalized.push(String::from("--json"));
        normalized
    } else {
        args.to_vec()
    }
}

/// Returns whether a command has a required positional candidate after `--json`.
fn has_position_candidate(args: &[String]) -> bool {
    move_leading_json_to_tail(args)
        .first()
        .is_some_and(|arg| !arg.starts_with('-'))
}

/// Takes a required positional argument after tolerating a leading JSON flag.
fn take_required_position(
    args: &[String],
    argument: &'static str,
    next: &'static str,
) -> Result<(String, Vec<String>), CliError> {
    let normalized = move_leading_json_to_tail(args);
    let (value, rest) = take_required_arg(normalized.as_slice(), argument, next)?;
    Ok((value.to_owned(), rest.to_vec()))
}

/// Parses `read`.
fn parse_read(args: &[String]) -> Result<Command, CliError> {
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
fn parse_read_verb(verb: &str, args: &[String]) -> Result<Command, CliError> {
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
fn is_read_verb(value: &str) -> bool {
    matches!(value, "show" | "list" | "get") || is_recency_read_verb(value)
}

/// Returns whether a command is a recency-flavored read alias.
fn is_recency_read_verb(value: &str) -> bool {
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
        _ => resource,
    }
}

/// Returns whether a command is a natural log search shortcut.
fn is_log_search_shortcut(command: &str) -> bool {
    matches!(command, "search" | "find" | "grep")
}

/// Returns whether a log search form uses `--` to search help-looking text.
fn is_log_search_separator_literal(command: &str, args: &[String]) -> bool {
    literal_log_search_separator_index(command, args).is_some()
}

/// Returns the static argument label for a natural log search shortcut.
fn log_search_shortcut_label(command: &str) -> &'static str {
    match command {
        "find" => "find",
        "grep" => "grep",
        _ => "search",
    }
}

/// Parses natural log search shortcuts as `logs --search <text>`.
fn parse_search_shortcut(label: &'static str, args: &[String]) -> Result<Command, CliError> {
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
fn parse_read_resource(resource: &str, rest: &[String]) -> Result<Command, CliError> {
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
            &[
                "--name",
                "--since",
                "--user",
                "--distinct-id",
                "--trace",
                "--trace-id",
                "--level",
                "--severity",
                "--search",
                "--status",
            ],
        )?,
        "trace" => return parse_trace_detail_or_explain(rest),
        "traces" | "span" | "spans" if has_position_candidate(rest) => {
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
fn is_issue_collection_alias(value: &str) -> bool {
    matches!(
        value,
        "issues" | "errors" | "error" | "exceptions" | "exception"
    )
}

/// Returns whether a resource word can follow a status-first issue shortcut.
fn is_status_first_issue_collection_alias(value: &str) -> bool {
    value == "issue" || is_issue_collection_alias(value)
}

/// Returns whether args begin with an issue collection after a status word.
fn has_status_first_issue_resource_candidate(args: &[String]) -> bool {
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
fn is_action_collection_alias(value: &str) -> bool {
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
        &["--name", "--user", "--distinct-id", "--status"],
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
        &[
            "--name",
            "--since",
            "--user",
            "--distinct-id",
            "--trace",
            "--trace-id",
            "--level",
            "--severity",
            "--search",
        ],
    )
}

/// Parses `open issues` as `issues --status unresolved`.
fn parse_status_first_issue_read(status: &str, args: &[String]) -> Result<Command, CliError> {
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
        ACTION_LIST_UNSUPPORTED_FLAGS,
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
fn has_issue_id_candidate(args: &[String]) -> bool {
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
    if let Some(command) =
        parse_detail_explain_suffix(ExplainTarget::Issue(id.clone()), tail.as_slice())?
    {
        return Ok(command);
    }
    let target = ReadTarget::Issue(id);
    let flags = parse_detail_read_flags(
        tail.as_slice(),
        "read issue",
        READ_ISSUE_NEXT_STEP,
        ISSUE_DETAIL_UNSUPPORTED_FLAGS,
    )?;
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
    unsupported_flags: &[&str],
) -> Result<(ReadTarget, crate::flags::Flags), CliError> {
    reject_unsupported_read_flags(args, command, next, unsupported_flags)?;
    Ok((target, parse_flags(args, FlagScope::Read)?))
}

/// Rejects target-inapplicable read filters before parsing values.
fn reject_unsupported_read_flags(
    args: &[String],
    command: &'static str,
    next: &'static str,
    unsupported_flags: &[&str],
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
        if unsupported_flags.contains(&flag) {
            return Err(CliError::UnsupportedFlag {
                flag: flag.to_owned(),
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
        "--since" => "--since",
        "--user" | "--distinct-id" => "--user",
        "--trace" | "--trace-id" => "--trace",
        "--level" | "--severity" => "--level",
        "--search" => "--search",
        "--project" | "--project-id" => "--project",
        "--release" => "--release",
        "--environment" | "--env" => "--environment",
        "--status" => "--status",
        "--limit" => "--limit",
        _ => return None,
    };
    Some(canonical)
}

/// Returns whether a supported read flag has a value that should be reported first.
fn has_invalid_supported_read_value(flag: &str, value: &str) -> bool {
    match flag {
        "--level" | "--severity" => !is_known_log_level(value),
        "--status" => !is_known_issue_status(value),
        "--limit" => value.parse::<u32>().map_or(true, |limit| limit == 0),
        _ => false,
    }
}

/// Returns whether a value is in the log-level vocabulary.
fn is_known_log_level(value: &str) -> bool {
    normalize_log_level(value).is_ok()
}

/// Returns whether a positional log search word should stay a recoverable error.
fn is_ambiguous_log_search_word(value: &str) -> bool {
    is_read_filter_word(value) || value.contains('@') || is_trace_id(value)
}

/// Returns whether a value is in the issue-status vocabulary.
fn is_known_issue_status(value: &str) -> bool {
    normalize_status(value).is_ok()
}

/// Returns whether a flag is a value-taking read filter.
fn is_read_value_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--name"
            | "--since"
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
    )
}

/// Parses one trace detail read or trace explain suffix.
fn parse_trace_detail_or_explain(rest: &[String]) -> Result<Command, CliError> {
    let (id, tail) = take_required_position(rest, "trace_id", "provide a trace id")?;
    if let Some(command) =
        parse_detail_explain_suffix(ExplainTarget::Trace(id.clone()), tail.as_slice())?
    {
        return Ok(command);
    }
    let target = ReadTarget::Trace(id);
    let flags = parse_detail_read_flags(
        tail.as_slice(),
        "read trace",
        READ_TRACE_NEXT_STEP,
        TRACE_DETAIL_UNSUPPORTED_FLAGS,
    )?;
    let json = flags.is_json();
    let options = flags.into_read_options();
    validate_read_filters(&target, &options)?;

    Ok(Command::Read {
        target,
        options: Box::new(options),
        json,
    })
}

/// Parses a trailing `explain` action after an issue or trace detail id.
fn parse_detail_explain_suffix(
    target: ExplainTarget,
    args: &[String],
) -> Result<Option<Command>, CliError> {
    let normalized = move_leading_json_to_tail(args);
    if normalized.first().is_none_or(|arg| arg != "explain") {
        return Ok(None);
    }
    Ok(Some(Command::Explain {
        target,
        json: parse_flags(&normalized[1..], FlagScope::Explain)?.is_json(),
    }))
}

/// Parses detail read filters after rejecting list-only filters.
fn parse_detail_read_flags(
    args: &[String],
    command: &'static str,
    next: &'static str,
    unsupported_flags: &[&str],
) -> Result<crate::flags::Flags, CliError> {
    reject_unsupported_read_flags(args, command, next, unsupported_flags)?;
    parse_flags(args, FlagScope::Read)
}

/// Parses an obvious pasted issue or trace id as a detail read shortcut.
fn parse_pasted_detail_id(id: &str, args: &[String]) -> Result<Command, CliError> {
    if is_issue_id(id) && has_issue_status_action(args) {
        return parse_issue_first_status_shortcut(id.to_owned(), args);
    }
    let explain_args = move_leading_json_to_tail(args);
    if explain_args.first().is_some_and(|arg| arg == "explain") {
        let target = infer_explain_target(id).ok_or_else(|| unknown_command(id))?;
        return Ok(Command::Explain {
            target,
            json: parse_flags(&explain_args[1..], FlagScope::Explain)?.is_json(),
        });
    }
    let (target, flags) = if is_trace_id(id) {
        (
            ReadTarget::Trace(id.to_owned()),
            parse_detail_read_flags(
                args,
                "read trace",
                READ_TRACE_NEXT_STEP,
                TRACE_DETAIL_UNSUPPORTED_FLAGS,
            )?,
        )
    } else if is_issue_id(id) {
        (
            ReadTarget::Issue(id.to_owned()),
            parse_detail_read_flags(
                args,
                "read issue",
                READ_ISSUE_NEXT_STEP,
                ISSUE_DETAIL_UNSUPPORTED_FLAGS,
            )?,
        )
    } else {
        return Err(unknown_command(id));
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

/// Rejects filters that a read endpoint would otherwise ignore.
fn validate_read_filters(target: &ReadTarget, filters: &ReadOptions) -> Result<(), CliError> {
    let unsupported = match target {
        ReadTarget::Logs => filters
            .first_log_unsupported_flag()
            .map(|flag| (flag, "read logs", READ_LOGS_NEXT_STEP)),
        ReadTarget::Issues => filters
            .first_issue_list_unsupported_flag()
            .map(|flag| (flag, "read issues", READ_ISSUES_NEXT_STEP)),
        ReadTarget::Actions => filters
            .first_action_unsupported_flag()
            .map(|flag| (flag, "read actions", READ_ACTIONS_NEXT_STEP)),
        ReadTarget::Releases => filters
            .first_release_unsupported_flag()
            .map(|flag| (flag, "read releases", READ_RELEASES_NEXT_STEP)),
        ReadTarget::Trace(_) => filters
            .first_trace_detail_unsupported_flag()
            .map(|flag| (flag, "read trace", READ_TRACE_NEXT_STEP)),
        ReadTarget::Issue(_) => filters
            .first_issue_detail_unsupported_flag()
            .map(|flag| (flag, "read issue", READ_ISSUE_NEXT_STEP)),
    };

    if let Some((flag, command, next)) = unsupported {
        return Err(CliError::UnsupportedFlag {
            flag: flag.to_owned(),
            command,
            next,
        });
    }
    Ok(())
}

/// Parses `explain`.
fn parse_explain(args: &[String]) -> Result<Command, CliError> {
    let (resource, rest) = take_required_position(args, "resource", EXPLAIN_RESOURCE_NEXT_STEP)?;
    let (target, tail) = match resource.as_str() {
        "issue" => {
            let (id, tail) =
                take_required_position(rest.as_slice(), "issue_id", "provide an issue id")?;
            (ExplainTarget::Issue(id), tail)
        }
        "trace" => {
            let (id, tail) =
                take_required_position(rest.as_slice(), "trace_id", "provide a trace id")?;
            (ExplainTarget::Trace(id), tail)
        }
        other => {
            if let Some(target) = infer_explain_target(other) {
                (target, rest)
            } else {
                return Err(unknown_resource(other, EXPLAIN_RESOURCE_NEXT_STEP));
            }
        }
    };
    let flags = parse_flags(tail.as_slice(), FlagScope::Explain)?;
    Ok(Command::Explain {
        target,
        json: flags.is_json(),
    })
}

/// Parses `set`.
fn parse_set(args: &[String]) -> Result<Command, CliError> {
    let (resource, rest) = take_required_position(args, "resource", SET_RESOURCE_NEXT_STEP)?;
    if resource != "issue" {
        return Err(unknown_resource(resource.as_str(), SET_RESOURCE_NEXT_STEP));
    }
    let (id, rest) = take_required_position(rest.as_slice(), "issue_id", "provide an issue id")?;
    let (status, tail) =
        take_required_position(rest.as_slice(), "status", ISSUE_STATUS_ARGUMENT_NEXT_STEP)?;
    let status = normalize_status(status.as_str())?;
    let flags = parse_flags(tail.as_slice(), FlagScope::Set)?;

    Ok(Command::Set {
        target: SetTarget::IssueStatus { id, status },
        json: flags.is_json(),
    })
}
