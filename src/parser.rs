//! CLI command grammar.

mod analytics;
mod explain;
mod help_topics;
mod issue_shortcuts;
mod log_shortcuts;
mod native_debug;
mod operations;
mod project;
mod read;
mod support;
mod trace_reads;
mod watch;

use analytics::parse_analytics;
use help_topics::{
    command_shaped_help_topic, contains_help_flag, ensure_no_help_positionals, help_command,
    help_topic, is_direct_filter_help_alias, is_help_flag, parse_help, parse_help_alias,
    parse_literal_help, positional_args, validate_help_flags,
};
use issue_shortcuts::{
    is_issue_status_action_alias, parse_bare_issue_status_shortcut, parse_issue_status_shortcut,
    parse_status_first_issue_id_shortcut,
};
use log_shortcuts::literal_log_search_separator_index;
use operations::parse_operations;
use support::parse_support;
use watch::parse_watch;

use crate::flags::{FlagScope, normalize_status, parse_flags};
use crate::ids::{is_pasted_detail_id, is_trace_id, is_uuid};
use crate::{
    BillingTier, CliError, Command, HelpTopic, ISSUE_STATUS_ARGUMENT_NEXT_STEP, SetTarget,
    auth_namespace,
};
use explain::{parse_explain, parse_investigate};
use native_debug::parse_native_debug_artifacts;
use project::{parse_project, parse_setup_create_project};
use read::{
    has_issue_id_candidate, has_status_first_issue_resource_candidate, is_known_issue_status,
    is_log_search_shortcut, is_read_verb, log_search_shortcut_label, parse_pasted_detail_id,
    parse_read, parse_read_resource, parse_read_verb, parse_search_shortcut,
    parse_status_first_issue_read,
};

/// Standard next step for malformed help invocations.
const HELP_NEXT_STEP: &str = "run logbrew --help";
/// Recovery hint for users who type trace terminology as a top-level command.
const TRACE_COMMAND_NEXT_STEP: &str =
    "use logbrew trace <trace_id> or logbrew explain trace <trace_id>";
/// Valid resources for live watch.
const WATCH_RESOURCE_NEXT_STEP: &str = "choose logs, issues, actions, or omit a resource";
/// Valid resources for state mutation.
const SET_RESOURCE_NEXT_STEP: &str = "choose issue";
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
    if is_setup_alias(head) && tail.iter().any(|arg| arg == "--create-project") {
        return parse_setup_create_project(tail);
    }
    if contains_help_flag(tail) && literal_log_search_separator_index(head, tail).is_none() {
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
        "status" | "health" | "ping" => parse_status(tail),
        "whoami" | "me" => parse_whoami(tail),
        "doctor" => parse_doctor(tail),
        "version" => parse_version(tail),
        "account" if tail.first().is_some_and(|arg| arg == "usage") => parse_usage(&tail[1..]),
        alias if auth_namespace::is_namespace(alias) => auth_namespace::parse(tail),
        alias if auth_namespace::is_help_alias(alias) => parse_help_alias(HelpTopic::Auth, tail),
        "json" | "output" => parse_help_alias(HelpTopic::Json, tail),
        alias if is_examples_help_alias(alias) => parse_help_alias(HelpTopic::Examples, tail),
        alias if is_project_help_alias(alias) => parse_project(tail),
        "usage" => parse_usage(tail),
        "mcp" | "billing" | "operations" => parse_account_namespace(head, tail),
        "support" => parse_support(tail),
        "investigate" => parse_investigate(tail),
        "debug-artifacts" => parse_native_debug_artifacts(tail),
        "analytics" => parse_analytics(tail),
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
        alias if matches!(alias, "trace" | "span") && !has_position_candidate(tail) => {
            parse_help_alias(HelpTopic::ReadTrace, tail)
        }
        alias if matches!(alias, "traces" | "spans") && has_trace_id_candidate(tail) => {
            parse_read_resource("trace", tail)
        }
        "traces" | "spans" => parse_read_resource("traces", tail),
        "issues" if is_issue_triage_action(tail) => crate::issue_triage::parse(tail),
        "issues" if crate::issue_ownership::is_command(tail) => crate::issue_ownership::parse(tail),
        "inbox" => crate::issue_ownership::parse_inbox(tail),
        "logs" if crate::issue_ownership::is_log_rule(tail) => {
            crate::issue_ownership::parse_log_rule(tail)
        }
        "logs" | "issues" | "errors" | "error" | "exceptions" | "exception" | "actions"
        | "events" | "event" | "action" | "releases" | "metrics" | "metric" | "trace" | "issue" => {
            parse_read_resource(head, tail)
        }
        "span" if has_position_candidate(tail) => parse_read_resource("trace", tail),
        "resolve" | "close" | "ignore" | "reopen" => parse_issue_status_shortcut(head, tail),
        alias if is_watch_command_alias(alias) => parse_watch(tail),
        "explain" => parse_explain(tail),
        "deploy" => crate::deployment::parse(tail),
        "set" => parse_set(tail),
        id if is_pasted_detail_id(id) => parse_pasted_detail_id(id, tail),
        _ => Err(unknown_command(head)),
    }
}

/// Restricts public UUID inputs to lowercase dashed canonical form.
fn is_canonical_lower_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23)
                    && (byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
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

/// Returns whether a word should land on example-oriented help.
fn is_examples_help_alias(value: &str) -> bool {
    matches!(
        value,
        "example" | "examples" | "sample" | "samples" | "recipe" | "recipes"
    )
}

/// Returns whether a word should run the non-mutating setup plan.
fn is_setup_alias(value: &str) -> bool {
    matches!(value, "setup" | "init" | "install" | "configure" | "sdk")
}

/// Returns whether a word should land on backend-owned project setup help.
fn is_project_help_alias(value: &str) -> bool {
    matches!(value, "project" | "projects")
}

/// Returns whether a word should use the live watch placeholder flow.
fn is_watch_command_alias(value: &str) -> bool {
    matches!(value, "watch" | "tail" | "follow" | "stream")
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
        provider: flags.login_provider(),
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
    if args.iter().any(|arg| arg == "--create-project") {
        return parse_setup_create_project(args);
    }
    let (mut mcp, mut sample_data, mut project_id, mut rest) = (false, false, None, Vec::new());
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--mcp" if !mcp => {
                mcp = true;
                continue;
            }
            "--sample-data" if !sample_data => {
                sample_data = true;
                continue;
            }
            "--project" | "--project-id" => args.next().map(String::as_str),
            other => {
                let value = other
                    .strip_prefix("--project=")
                    .or_else(|| other.strip_prefix("--project-id="));
                if value.is_none() {
                    rest.push(arg.clone());
                    continue;
                }
                value
            }
        };
        match value {
            Some(value) if project_id.is_none() && is_uuid(value) => {
                project_id = Some(value.to_owned());
            }
            _ => {
                return Err(CliError::MissingFlagValue {
                    flag: "--project",
                    next: "use --project once with a project UUID from logbrew projects",
                });
            }
        }
    }
    let flags = parse_flags(rest.as_slice(), FlagScope::Setup)?;
    Ok(Command::Setup {
        auto: flags.is_auto(),
        yes: flags.skip_prompts(),
        json: flags.is_json(),
        mcp,
        project_id,
        sample_data,
    })
}

/// Takes an inline or following flag value without reflecting invalid input.
fn flag_value<'a>(
    args: &'a [String],
    index: &mut usize,
    inline: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(value) = inline {
        return Some(value);
    }
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
}

/// Parses `status`.
fn parse_status(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Status)?;
    Ok(Command::Status {
        json: flags.is_json(),
    })
}

/// Parses an authenticated account identity read.
fn parse_whoami(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::WhoAmI)?;
    Ok(Command::WhoAmI {
        json: flags.is_json(),
    })
}

/// Parses the closed MCP grant list and confirmed revocation grammar.
fn parse_mcp(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    let json = json_count == 1;
    match values.as_slice() {
        ["grants"] if json_count <= 1 => Ok(Command::McpGrants { json }),
        ["revoke", grant_id, "--yes"] if json_count <= 1 && is_uuid(grant_id) => {
            Ok(Command::McpRevoke {
                grant_id: grant_id.to_ascii_lowercase(),
                json,
            })
        }
        _ => Err(CliError::InvalidMcpCommand),
    }
}

/// Returns whether `logbrew issues` continues with a change rather than a read.
fn is_issue_triage_action(args: &[String]) -> bool {
    args.iter()
        .find(|arg| *arg != "--json")
        .is_some_and(|action| {
            matches!(
                action.as_str(),
                "archive" | "resolve" | "reopen" | "priority" | "assign" | "unassign" | "assignees"
            )
        })
}

/// Parses the account-scoped `mcp`, `billing`, and `operations` namespaces.
fn parse_account_namespace(head: &str, args: &[String]) -> Result<Command, CliError> {
    match head {
        "mcp" => parse_mcp(args),
        "operations" => parse_operations(args),
        _ => parse_billing(args),
    }
}

/// Parses the closed plan checkout and billing management grammar.
fn parse_billing(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    let tier = match values.as_slice() {
        _ if json_count > 1 => return Err(CliError::InvalidBillingCommand),
        ["upgrade", "pro"] => Some(BillingTier::Pro),
        ["upgrade", "business"] => Some(BillingTier::Business),
        ["portal"] => None,
        _ => return Err(CliError::InvalidBillingCommand),
    };
    Ok(Command::Billing {
        tier,
        json: json_count == 1,
    })
}

/// Parses the closed authenticated account-usage read grammar.
fn parse_usage(args: &[String]) -> Result<Command, CliError> {
    match args {
        [] => Ok(Command::Usage { json: false }),
        [flag] if flag == "--json" => Ok(Command::Usage { json: true }),
        _ => Err(CliError::InvalidUsageCommand),
    }
}

/// Parses bare status-compatible doctor or one strict project-scoped diagnostic.
fn parse_doctor(args: &[String]) -> Result<Command, CliError> {
    if args.iter().all(|arg| arg == "--json") {
        return parse_status(args);
    }

    let mut project_id = None;
    let mut json = false;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if let Some(value) = argument
            .strip_prefix("--project=")
            .or_else(|| argument.strip_prefix("--project-id="))
        {
            if project_id.is_some() || !is_uuid(value) {
                return Err(CliError::InvalidDoctorCommand);
            }
            project_id = Some(value.to_owned());
            index += 1;
            continue;
        }
        match argument.as_str() {
            "--json" if !json => json = true,
            "--project" | "--project-id" if project_id.is_none() => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(CliError::InvalidDoctorCommand);
                };
                if !is_uuid(value) {
                    return Err(CliError::InvalidDoctorCommand);
                }
                project_id = Some(value.clone());
            }
            _ => return Err(CliError::InvalidDoctorCommand),
        }
        index += 1;
    }

    project_id.map_or(Err(CliError::InvalidDoctorCommand), |project_id| {
        Ok(Command::Doctor { project_id, json })
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

/// Returns whether args begin with an obvious copied trace id after optional `--json`.
fn has_trace_id_candidate(args: &[String]) -> bool {
    move_leading_json_to_tail(args)
        .first()
        .is_some_and(|arg| is_trace_id(arg))
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
