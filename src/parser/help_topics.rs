//! Help-topic parsing for the CLI grammar.

use super::{
    EXPLAIN_RESOURCE_NEXT_STEP, HELP_NEXT_STEP, SET_RESOURCE_NEXT_STEP, WATCH_RESOURCE_NEXT_STEP,
    is_action_collection_alias, is_ambiguous_log_search_word, is_examples_help_alias,
    is_issue_collection_alias, is_issue_status_action_alias, is_known_issue_status,
    is_known_log_level, is_log_search_shortcut, is_project_help_alias, is_read_verb,
    is_recency_read_verb, is_setup_alias, is_status_first_issue_collection_alias,
    is_status_help_alias, is_watch_command_alias, unknown_command, unknown_flag,
    unknown_read_resource, unknown_resource,
};
use crate::ids::{infer_explain_target, is_issue_id, is_pasted_detail_id, is_trace_id};
use crate::{CliError, Command, HelpTopic, auth_namespace};

/// Parses `help`.
pub(super) fn parse_help(args: &[String]) -> Result<Command, CliError> {
    validate_help_flags(args)?;
    let positionals = positional_args(args);
    let topic = explicit_help_topic(positionals.as_slice())?;
    Ok(help_command(topic, args))
}

/// Parses `logbrew read help logs` and `logbrew logs help` style help.
pub(super) fn parse_literal_help(head: &str, args: &[String]) -> Result<Option<Command>, CliError> {
    let Some(help_index) = args
        .iter()
        .position(|arg| {
            arg == "help"
                || arg.starts_with('-') && !matches!(arg.as_str(), "--json" | "--help" | "-h")
        })
        .filter(|index| args[*index] == "help")
    else {
        return Ok(None);
    };
    validate_help_flags(args)?;
    let mut tail = args.to_vec();
    drop(tail.remove(help_index));
    if help_index > 0
        && let Some(topic) = command_shaped_help_topic(head, tail.as_slice())
    {
        return Ok(Some(help_command(topic, args)));
    }
    Ok(Some(help_command(help_topic(head, tail.as_slice())?, args)))
}

/// Builds a help command.
pub(super) fn help_command(topic: HelpTopic, args: &[String]) -> Command {
    Command::Help {
        topic,
        json: contains_json_flag(args),
    }
}

/// Parses a topic alias such as `logbrew auth`.
pub(super) fn parse_help_alias(topic: HelpTopic, args: &[String]) -> Result<Command, CliError> {
    validate_help_flags(args)?;
    ensure_no_help_positionals(positional_args(args).as_slice())?;
    Ok(help_command(topic, args))
}

/// Resolves the help topic for a command tail.
pub(super) fn help_topic(head: &str, tail: &[String]) -> Result<HelpTopic, CliError> {
    let positionals = positional_args(tail);
    match head {
        "login" => help_topic_without_positionals(HelpTopic::Login, positionals.as_slice()),
        "logout" => help_topic_without_positionals(HelpTopic::Logout, positionals.as_slice()),
        alias if is_setup_alias(alias) => {
            help_topic_without_positionals(HelpTopic::Setup, positionals.as_slice())
        }
        alias if is_status_help_alias(alias) => {
            help_topic_without_positionals(HelpTopic::Status, positionals.as_slice())
        }
        "whoami" | "me" => {
            help_topic_without_positionals(HelpTopic::Status, positionals.as_slice())
        }
        "version" => help_topic_without_positionals(HelpTopic::Version, positionals.as_slice()),
        "account" if positionals.first().is_some_and(|arg| *arg == "usage") => {
            help_topic_without_positionals(HelpTopic::Usage, &positionals[1..])
        }
        alias if auth_namespace::is_namespace(alias) => {
            auth_namespace::help_topic(positionals.as_slice())
        }
        alias if auth_namespace::is_help_alias(alias) => {
            help_topic_without_positionals(HelpTopic::Auth, positionals.as_slice())
        }
        "json" | "output" => {
            help_topic_without_positionals(HelpTopic::Json, positionals.as_slice())
        }
        alias if is_examples_help_alias(alias) => {
            help_topic_without_positionals(HelpTopic::Examples, positionals.as_slice())
        }
        alias if is_project_help_alias(alias) => Ok(HelpTopic::Projects),
        "usage" => Ok(HelpTopic::Usage),
        "support" => help_topic_without_positionals(HelpTopic::Support, positionals.as_slice()),
        "list" if positionals.first().is_some_and(|arg| *arg == "issue") => {
            help_topic_without_positionals(HelpTopic::ReadIssues, &positionals[1..])
        }
        "read" => read_help_topic(positionals.as_slice()),
        alias if is_read_verb(alias) => read_verb_help_topic(alias, positionals.as_slice()),
        status if is_known_issue_status(status) => {
            status_first_issue_help_topic(positionals.as_slice())
        }
        "logs" => log_list_help_topic(positionals.as_slice()),
        alias if is_log_search_shortcut(alias) => log_search_help_topic(positionals.as_slice()),
        "issues" => issue_alias_help_topic(positionals.as_slice()),
        "errors" | "error" | "exceptions" | "exception" => {
            issue_alias_help_topic(positionals.as_slice())
        }
        "actions" | "action" | "events" | "event" => {
            action_alias_help_topic(positionals.as_slice())
        }
        "releases" => {
            help_topic_without_positionals(HelpTopic::ReadReleases, positionals.as_slice())
        }
        "trace" | "span" => trace_help_topic(positionals.as_slice()),
        "traces" | "spans" => trace_collection_help_topic(positionals.as_slice()),
        "issue" => singular_issue_help_topic(positionals.as_slice()),
        "resolve" | "close" | "ignore" | "reopen" => {
            issue_mutation_help_topic(positionals.as_slice())
        }
        alias if is_read_filter_help_alias(alias) => {
            help_topic_without_positionals(HelpTopic::Read, positionals.as_slice())
        }
        alias if is_watch_command_alias(alias) => subresource_help_topic(
            HelpTopic::Watch,
            positionals.as_slice(),
            &["logs", "actions", "action", "events", "event"],
            WATCH_RESOURCE_NEXT_STEP,
        ),
        "explain" => subresource_help_topic(
            HelpTopic::Explain,
            positionals.as_slice(),
            &["issue", "trace"],
            EXPLAIN_RESOURCE_NEXT_STEP,
        ),
        "set" => subresource_help_topic(
            HelpTopic::Set,
            positionals.as_slice(),
            &["issue"],
            SET_RESOURCE_NEXT_STEP,
        ),
        _ => Err(unknown_command(head)),
    }
}

/// Resolves help for fully typed command shapes such as `issue <id> --help`.
pub(super) fn command_shaped_help_topic(head: &str, tail: &[String]) -> Option<HelpTopic> {
    let positionals = positional_args(tail);
    match head {
        "read" => read_command_shaped_help_topic(positionals.as_slice()),
        alias if is_read_verb(alias) => natural_read_command_shaped_help_topic(
            recency_count_help_args(alias, positionals.as_slice()),
        ),
        alias if is_log_search_shortcut(alias) => {
            log_search_command_shaped_help_topic(positionals.as_slice())
        }
        "logs" => log_list_command_shaped_help_topic(positionals.as_slice()),
        status if is_known_issue_status(status) => {
            status_first_issue_command_shaped_help_topic(positionals.as_slice())
        }
        "trace" | "span" => trace_command_shaped_help_topic(positionals.as_slice()),
        "traces" | "spans" => trace_collection_command_shaped_help_topic(positionals.as_slice()),
        "issue" => issue_command_shaped_help_topic(positionals.as_slice()),
        alias if is_issue_collection_alias(alias) => {
            issue_alias_command_shaped_help_topic(positionals.as_slice())
        }
        alias if is_action_collection_alias(alias) => {
            action_alias_command_shaped_help_topic(positionals.as_slice())
        }
        "explain" => explain_command_shaped_help_topic(positionals.as_slice()),
        "set" => set_command_shaped_help_topic(positionals.as_slice()),
        "support" => Some(HelpTopic::Support),
        "resolve" | "close" | "ignore" | "reopen" => {
            single_id_help_topic(positionals.as_slice(), HelpTopic::Set)
        }
        id if is_pasted_detail_id(id) => match positionals.as_slice() {
            [] => Some(if is_trace_id(id) {
                HelpTopic::ReadTrace
            } else {
                HelpTopic::ReadIssue
            }),
            ["explain"] => Some(HelpTopic::Explain),
            [action] if is_issue_id(id) && is_issue_status_action_alias(action) => {
                Some(HelpTopic::Set)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Resolves help for command-shaped historical reads.
fn read_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        [verb, tail @ ..] if is_recency_read_verb(verb) => {
            natural_read_command_shaped_help_topic(recency_count_help_args(verb, tail))
        }
        [status, tail @ ..] if is_known_issue_status(status) => {
            status_first_issue_command_shaped_help_topic(tail)
        }
        ["trace" | "traces" | "span" | "spans", id, "explain"] if is_trace_id(id) => {
            Some(HelpTopic::Explain)
        }
        ["trace" | "span", _] => Some(HelpTopic::ReadTrace),
        ["traces" | "spans", id] if is_trace_id(id) => Some(HelpTopic::ReadTrace),
        ["traces" | "spans", _] => Some(HelpTopic::ReadTraces),
        ["issue", status] if is_known_issue_status(status) => Some(HelpTopic::ReadIssues),
        ["issue", _] => Some(HelpTopic::ReadIssue),
        ["logs" | "log", tail @ ..] => log_list_command_shaped_help_topic(tail),
        [resource, tail @ ..] if is_issue_collection_alias(resource) => {
            issue_alias_command_shaped_help_topic(tail)
        }
        [resource, tail @ ..] if is_action_collection_alias(resource) => {
            action_alias_command_shaped_help_topic(tail)
        }
        _ => None,
    }
}

/// Resolves help for natural read commands that include detail IDs.
fn natural_read_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        ["trace" | "traces" | "span" | "spans", id, "explain"] if is_trace_id(id) => {
            Some(HelpTopic::Explain)
        }
        ["trace" | "span", _] => Some(HelpTopic::ReadTrace),
        ["traces" | "spans", id] if is_trace_id(id) => Some(HelpTopic::ReadTrace),
        ["traces" | "spans", _] => Some(HelpTopic::ReadTraces),
        ["issue", status] if is_known_issue_status(status) => Some(HelpTopic::ReadIssues),
        ["issue", _] => Some(HelpTopic::ReadIssue),
        [resource, tail @ ..] if is_issue_collection_alias(resource) => {
            issue_alias_command_shaped_help_topic(tail)
        }
        _ => None,
    }
}

/// Resolves help for a command followed by exactly one ID.
fn single_id_help_topic(positionals: &[&str], topic: HelpTopic) -> Option<HelpTopic> {
    match positionals {
        [_] => Some(topic),
        _ => None,
    }
}

/// Resolves trace detail help plus explain suffixes.
fn trace_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    match args {
        [] => Ok(HelpTopic::ReadTrace),
        [id] if is_trace_id(id) => Ok(HelpTopic::ReadTrace),
        [id, "explain"] if is_trace_id(id) => Ok(HelpTopic::Explain),
        [id, "explain", extra, ..] if is_trace_id(id) => Err(unexpected_help_argument(extra)),
        [id, extra, ..] if is_trace_id(id) => Err(unexpected_help_argument(extra)),
        [extra, ..] => Err(unexpected_help_argument(extra)),
    }
}

/// Resolves recent trace discovery help while preserving copied trace IDs.
fn trace_collection_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    match args {
        [] => Ok(HelpTopic::ReadTraces),
        [id] if is_trace_id(id) => Ok(HelpTopic::ReadTrace),
        [id, "explain"] if is_trace_id(id) => Ok(HelpTopic::Explain),
        [id, "explain", extra, ..] if is_trace_id(id) => Err(unexpected_help_argument(extra)),
        [id, extra, ..] if is_trace_id(id) => Err(unexpected_help_argument(extra)),
        [extra, ..] => Err(unexpected_help_argument(extra)),
    }
}

/// Resolves command-shaped trace detail help plus explain suffixes.
fn trace_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        [id, "explain"] if is_trace_id(id) => Some(HelpTopic::Explain),
        [_] => Some(HelpTopic::ReadTrace),
        _ => None,
    }
}

/// Resolves command-shaped recent trace discovery help.
fn trace_collection_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        [id, "explain"] if is_trace_id(id) => Some(HelpTopic::Explain),
        [id] if is_trace_id(id) => Some(HelpTopic::ReadTrace),
        [] | [_] => Some(HelpTopic::ReadTraces),
        _ => None,
    }
}

/// Resolves help for `issue <id>` and issue-first mutation aliases.
fn issue_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        [status] if is_known_issue_status(status) => Some(HelpTopic::ReadIssues),
        [_] => Some(HelpTopic::ReadIssue),
        [id, "explain"] if is_issue_id(id) => Some(HelpTopic::Explain),
        [_, _] => Some(HelpTopic::Set),
        _ => None,
    }
}

/// Resolves help for issue list aliases that include obvious issue ids.
fn issue_alias_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        [] => Some(HelpTopic::ReadIssues),
        [status] if is_known_issue_status(status) => Some(HelpTopic::ReadIssues),
        [id] if is_issue_id(id) => Some(HelpTopic::ReadIssue),
        [id, "explain"] if is_issue_id(id) => Some(HelpTopic::Explain),
        [id, action] if is_issue_id(id) && is_issue_status_action_alias(action) => {
            Some(HelpTopic::Set)
        }
        _ => None,
    }
}

/// Resolves issue/error help, allowing one status shortcut word.
fn issue_alias_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    issue_help_topic(args, HelpTopic::ReadIssues)
}

/// Resolves singular issue help while treating status words as list shortcuts.
fn singular_issue_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    issue_help_topic(args, HelpTopic::ReadIssue)
}

/// Resolves issue help, detail explain suffixes, and issue-first mutation aliases.
fn issue_help_topic(args: &[&str], empty_topic: HelpTopic) -> Result<HelpTopic, CliError> {
    match args {
        [] => Ok(empty_topic),
        [status] if is_known_issue_status(status) => Ok(HelpTopic::ReadIssues),
        [id] if is_issue_id(id) => Ok(HelpTopic::ReadIssue),
        [id, "explain"] if is_issue_id(id) => Ok(HelpTopic::Explain),
        [id, action] if is_issue_id(id) && is_issue_status_action_alias(action) => {
            Ok(HelpTopic::Set)
        }
        [id, "explain", extra, ..] if is_issue_id(id) => Err(unexpected_help_argument(extra)),
        [id, action, extra, ..] if is_issue_id(id) && is_issue_status_action_alias(action) => {
            Err(unexpected_help_argument(extra))
        }
        [id, extra, ..] if is_issue_id(id) => Err(unexpected_help_argument(extra)),
        [extra, ..] => Err(unexpected_help_argument(extra)),
    }
}

/// Resolves help for status-first issue shortcuts such as `open issues --help`.
fn status_first_issue_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    match args {
        [] => Ok(HelpTopic::Set),
        [id] if is_issue_id(id) => Ok(HelpTopic::Set),
        [id, extra, ..] if is_issue_id(id) => Err(unexpected_help_argument(extra)),
        [resource] if is_status_first_issue_collection_alias(resource) => Ok(HelpTopic::ReadIssues),
        [resource, extra, ..] if is_status_first_issue_collection_alias(resource) => {
            Err(unexpected_help_argument(extra))
        }
        [resource, ..] => Err(unknown_read_resource(resource)),
    }
}

/// Resolves command-shaped help for status-first issue shortcuts.
fn status_first_issue_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        [] => Some(HelpTopic::Set),
        [id] if is_issue_id(id) => Some(HelpTopic::Set),
        [resource] if is_status_first_issue_collection_alias(resource) => {
            Some(HelpTopic::ReadIssues)
        }
        _ => None,
    }
}

/// Resolves help for action list aliases that include one positional name.
fn action_alias_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    single_optional_positional_command_help_topic(positionals, HelpTopic::ReadActions)
}

/// Resolves log help, allowing executable natural log level/search positionals.
fn log_list_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    if let Some(argument) = invalid_log_list_help_positional(args) {
        return Err(unexpected_help_argument(argument));
    }
    Ok(HelpTopic::ReadLogs)
}

/// Resolves command-shaped log help for copied natural log commands.
fn log_list_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    invalid_log_list_help_positional(positionals)
        .is_none()
        .then_some(HelpTopic::ReadLogs)
}

/// Returns the first positional that is not executable in a natural log command.
fn invalid_log_list_help_positional<'a>(args: &'a [&'a str]) -> Option<&'a str> {
    let (first, tail) = args.split_first()?;
    if is_known_log_level(first) {
        if first.eq_ignore_ascii_case("trace") && !tail.is_empty() {
            return Some(first);
        }
        return tail
            .iter()
            .copied()
            .find(|arg| is_ambiguous_log_search_word(arg));
    }
    if is_ambiguous_log_search_word(first) || args.len() == 1 {
        return Some(first);
    }
    args.iter()
        .copied()
        .find(|arg| is_ambiguous_log_search_word(arg))
}

/// Resolves action/event help, allowing one positional action name.
fn action_alias_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    single_optional_positional_help_topic(args, HelpTopic::ReadActions)
}

/// Resolves search shortcut help, allowing one copied search term.
fn log_search_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    single_optional_positional_help_topic(args, HelpTopic::ReadLogs)
}

/// Resolves command-shaped search shortcut help with a copied search term.
fn log_search_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    single_optional_positional_command_help_topic(positionals, HelpTopic::ReadLogs)
}

/// Resolves help when a command may include one copied positional argument.
fn single_optional_positional_help_topic(
    args: &[&str],
    topic: HelpTopic,
) -> Result<HelpTopic, CliError> {
    match args {
        [] | [_] => Ok(topic),
        [_, extra, ..] => Err(unexpected_help_argument(extra)),
    }
}

/// Resolves command-shaped help when one copied positional argument is allowed.
fn single_optional_positional_command_help_topic(
    positionals: &[&str],
    topic: HelpTopic,
) -> Option<HelpTopic> {
    match positionals {
        [] | [_] => Some(topic),
        _ => None,
    }
}

/// Resolves help for explain commands that include an ID.
fn explain_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        ["issue" | "trace", _] => Some(HelpTopic::Explain),
        [id] if infer_explain_target(id).is_some() => Some(HelpTopic::Explain),
        _ => None,
    }
}

/// Resolves help for `set issue <id> <status>`.
fn set_command_shaped_help_topic(positionals: &[&str]) -> Option<HelpTopic> {
    match positionals {
        ["issue", _, _] => Some(HelpTopic::Set),
        _ => None,
    }
}

/// Returns whether a word should land on read/filter help.
pub(super) fn is_read_filter_help_alias(value: &str) -> bool {
    matches!(
        value,
        "env"
            | "environment"
            | "environments"
            | "filter"
            | "filters"
            | "project-id"
            | "service"
            | "service-name"
    )
}

/// Returns whether a bare word is safe to treat as filter help.
pub(super) fn is_direct_filter_help_alias(value: &str) -> bool {
    matches!(
        value,
        "env"
            | "environment"
            | "environments"
            | "filter"
            | "filters"
            | "project-id"
            | "service"
            | "service-name"
    )
}

/// Returns whether args request help.
pub(super) fn contains_help_flag(args: &[String]) -> bool {
    args.iter().any(|arg| is_help_flag(arg))
}

/// Returns whether a value is a help flag.
pub(super) fn is_help_flag(value: &str) -> bool {
    matches!(value, "--help" | "-h")
}

/// Resolves the topic for an explicit `help` command.
fn explicit_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    match args {
        [] => Ok(HelpTopic::Root),
        ["list", "issue"] => Ok(HelpTopic::ReadIssues),
        ["read", tail @ ..] => read_or_detail_help_topic(tail),
        [verb, tail @ ..] if is_read_verb(verb) => read_verb_help_topic(verb, tail),
        [status, tail @ ..] if is_known_issue_status(status) => status_first_issue_help_topic(tail),
        ["login", tail @ ..] => help_topic_without_positionals(HelpTopic::Login, tail),
        ["logout", tail @ ..] => help_topic_without_positionals(HelpTopic::Logout, tail),
        [topic, tail @ ..] if is_setup_alias(topic) => {
            help_topic_without_positionals(HelpTopic::Setup, tail)
        }
        [topic, tail @ ..] if is_status_help_alias(topic) => {
            help_topic_without_positionals(HelpTopic::Status, tail)
        }
        ["whoami" | "me", tail @ ..] => help_topic_without_positionals(HelpTopic::Status, tail),
        ["version", tail @ ..] => help_topic_without_positionals(HelpTopic::Version, tail),
        ["account", "usage", tail @ ..] => help_topic_without_positionals(HelpTopic::Usage, tail),
        ["support", tail @ ..] => help_topic_without_positionals(HelpTopic::Support, tail),
        [topic, tail @ ..] if auth_namespace::is_namespace(topic) => {
            auth_namespace::help_topic(tail)
        }
        [topic, tail @ ..] if auth_namespace::is_help_alias(topic) => {
            help_topic_without_positionals(HelpTopic::Auth, tail)
        }
        ["json" | "output", tail @ ..] => help_topic_without_positionals(HelpTopic::Json, tail),
        [topic, tail @ ..] if is_examples_help_alias(topic) => {
            help_topic_without_positionals(HelpTopic::Examples, tail)
        }
        [topic, ..] if is_project_help_alias(topic) => Ok(HelpTopic::Projects),
        ["usage", tail @ ..] => help_topic_without_positionals(HelpTopic::Usage, tail),
        [topic, tail @ ..] if is_watch_command_alias(topic) => subresource_help_topic(
            HelpTopic::Watch,
            tail,
            &["logs", "actions", "action", "events", "event"],
            WATCH_RESOURCE_NEXT_STEP,
        ),
        ["explain", tail @ ..] => explain_help_topic(tail),
        ["set", tail @ ..] => {
            subresource_help_topic(HelpTopic::Set, tail, &["issue"], SET_RESOURCE_NEXT_STEP)
        }
        ["logs", tail @ ..] => log_list_help_topic(tail),
        [topic, tail @ ..] if is_log_search_shortcut(topic) => log_search_help_topic(tail),
        ["issues", tail @ ..] => issue_alias_help_topic(tail),
        ["errors" | "error" | "exceptions" | "exception", tail @ ..] => {
            issue_alias_help_topic(tail)
        }
        ["actions" | "action" | "events" | "event", tail @ ..] => action_alias_help_topic(tail),
        ["releases", tail @ ..] => help_topic_without_positionals(HelpTopic::ReadReleases, tail),
        ["trace" | "span", tail @ ..] => trace_help_topic(tail),
        ["traces" | "spans", tail @ ..] => trace_collection_help_topic(tail),
        ["issue", tail @ ..] => singular_issue_help_topic(tail),
        ["resolve" | "close" | "ignore" | "reopen", tail @ ..] => issue_mutation_help_topic(tail),
        [topic, tail @ ..] if is_read_filter_help_alias(topic) => {
            help_topic_without_positionals(HelpTopic::Read, tail)
        }
        [id, "explain"] if infer_explain_target(id).is_some() => Ok(HelpTopic::Explain),
        [id, "explain", extra, ..] if infer_explain_target(id).is_some() => {
            Err(unexpected_help_argument(extra))
        }
        [id, action] if is_issue_id(id) && is_issue_status_action_alias(action) => {
            Ok(HelpTopic::Set)
        }
        [id, action, extra, ..] if is_issue_id(id) && is_issue_status_action_alias(action) => {
            Err(unexpected_help_argument(extra))
        }
        [other, ..] => Err(unknown_resource(other, HELP_NEXT_STEP)),
    }
}

/// Resolves explicit explain help, including copied IDs.
fn explain_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    match args {
        [] | ["issue" | "trace"] | ["issue" | "trace", _] => Ok(HelpTopic::Explain),
        ["issue" | "trace", _, extra, ..] => Err(unexpected_help_argument(extra)),
        [id] if infer_explain_target(id).is_some() => Ok(HelpTopic::Explain),
        [id, extra, ..] if infer_explain_target(id).is_some() => {
            Err(unexpected_help_argument(extra))
        }
        [resource, ..] => Err(unknown_resource(resource, EXPLAIN_RESOURCE_NEXT_STEP)),
    }
}

/// Resolves explicit read help, including copied detail commands.
fn read_or_detail_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    if let Some(topic) = read_command_shaped_help_topic(args) {
        return Ok(topic);
    }
    read_help_topic(args)
}

/// Resolves explicit natural read help, including copied detail commands.
fn natural_read_or_detail_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    if let Some(topic) = natural_read_command_shaped_help_topic(args) {
        return Ok(topic);
    }
    read_help_topic(args)
}

/// Resolves help for natural read verbs, preserving recency collection intent.
fn read_verb_help_topic(verb: &str, args: &[&str]) -> Result<HelpTopic, CliError> {
    let args = recency_count_help_args(verb, args);
    if is_recency_read_verb(verb) {
        match args {
            ["log"] => return Ok(HelpTopic::ReadLogs),
            ["issue"] => return Ok(HelpTopic::ReadIssues),
            ["release"] => return Ok(HelpTopic::ReadReleases),
            _ => {}
        }
    }
    natural_read_or_detail_help_topic(args)
}

/// Skips count words in recency help forms such as `last 10 logs --help`.
fn recency_count_help_args<'a>(verb: &str, args: &'a [&'a str]) -> &'a [&'a str] {
    match args {
        [count, tail @ ..]
            if is_recency_read_verb(verb)
                && !tail.is_empty()
                && count.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            tail
        }
        _ => args,
    }
}

/// Returns all non-flag arguments.
pub(super) fn positional_args(args: &[String]) -> Vec<&str> {
    args.iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect()
}

/// Rejects non-help flags in help commands.
pub(super) fn validate_help_flags(args: &[String]) -> Result<(), CliError> {
    let mut seen_json = false;
    for arg in args {
        if arg == "--json" && std::mem::replace(&mut seen_json, true) {
            return Err(CliError::DuplicateFlag {
                flag: "--json",
                next: "use --json once",
            });
        }
        if arg.starts_with('-') && !is_help_flag(arg) && arg != "--json" {
            return Err(unknown_flag(arg, HELP_NEXT_STEP));
        }
    }
    Ok(())
}

/// Returns an explicit help topic after rejecting stray positional arguments.
fn help_topic_without_positionals(topic: HelpTopic, args: &[&str]) -> Result<HelpTopic, CliError> {
    ensure_no_help_positionals(args)?;
    Ok(topic)
}

/// Rejects positional arguments that would otherwise be ignored by help.
pub(super) fn ensure_no_help_positionals(args: &[&str]) -> Result<(), CliError> {
    if let Some(argument) = args.first() {
        return Err(unexpected_help_argument(argument));
    }
    Ok(())
}

/// Builds a help-specific unexpected argument error.
fn unexpected_help_argument(argument: &str) -> CliError {
    CliError::UnexpectedArgument {
        argument: argument.to_owned(),
        command: "help",
        next: HELP_NEXT_STEP,
    }
}

/// Allows known subresource words to select a command help topic.
fn subresource_help_topic(
    topic: HelpTopic,
    args: &[&str],
    resources: &[&str],
    next: &'static str,
) -> Result<HelpTopic, CliError> {
    match args {
        [] => Ok(topic),
        [resource] if resources.contains(resource) => Ok(topic),
        [resource, extra, ..] if resources.contains(resource) => {
            Err(unexpected_help_argument(extra))
        }
        [resource, ..] => Err(unknown_resource(resource, next)),
    }
}

/// Resolves help for issue mutation shortcuts.
fn issue_mutation_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    subresource_help_topic(HelpTopic::Set, args, &["issue"], SET_RESOURCE_NEXT_STEP)
}

/// Resolves help for `read` resources.
fn read_help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    if let Some(extra) = copied_read_detail_extra_argument(args) {
        return Err(unexpected_help_argument(extra));
    }

    match args {
        [] => Ok(HelpTopic::Read),
        [verb, tail @ ..] if is_recency_read_verb(verb) => read_verb_help_topic(verb, tail),
        [status, tail @ ..] if is_known_issue_status(status) => status_first_issue_help_topic(tail),
        ["logs" | "log", tail @ ..] => log_list_help_topic(tail),
        [resource, tail @ ..] if is_action_collection_alias(resource) => {
            action_alias_help_topic(tail)
        }
        [resource] => read_resource_help_topic(resource),
        [resource, extra, ..] => match read_resource_help_topic(resource) {
            Ok(_) => Err(unexpected_help_argument(extra)),
            Err(error) => Err(error),
        },
    }
}

/// Returns the actual stray argument after copied read-detail IDs.
fn copied_read_detail_extra_argument<'a>(args: &'a [&'a str]) -> Option<&'a str> {
    match args {
        ["trace" | "traces" | "span" | "spans", id, extra, ..] if is_trace_id(id) => Some(extra),
        ["issue", id, extra, ..] if is_issue_id(id) => Some(extra),
        [resource, id, extra, ..] if is_issue_collection_alias(resource) && is_issue_id(id) => {
            Some(extra)
        }
        _ => None,
    }
}

/// Resolves a single `read` help resource.
fn read_resource_help_topic(resource: &str) -> Result<HelpTopic, CliError> {
    match resource {
        "logs" | "log" => Ok(HelpTopic::ReadLogs),
        "issues" | "errors" | "error" | "exceptions" | "exception" => Ok(HelpTopic::ReadIssues),
        "actions" | "action" | "events" | "event" => Ok(HelpTopic::ReadActions),
        "releases" | "release" => Ok(HelpTopic::ReadReleases),
        "trace" | "span" => Ok(HelpTopic::ReadTrace),
        "traces" | "spans" => Ok(HelpTopic::ReadTraces),
        "issue" => Ok(HelpTopic::ReadIssue),
        "project" | "projects" => Ok(HelpTopic::Read),
        alias if is_read_filter_help_alias(alias) => Ok(HelpTopic::Read),
        other => Err(unknown_read_resource(other)),
    }
}

/// Returns whether args request JSON output.
fn contains_json_flag(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--json")
}
