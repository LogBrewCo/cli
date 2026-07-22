//! CLI command grammar.

mod help_topics;
mod issue_shortcuts;
mod log_shortcuts;
mod support;
mod trace_reads;
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
use support::parse_support;
use trace_reads::{parse_trace_detail_or_explain, parse_trace_list_read};
use watch::parse_watch;

use crate::flags::{
    FlagScope, is_read_filter_word, is_simple_flag, normalize_log_level, normalize_status,
    parse_flags, validate_min_duration,
};
use crate::ids::{infer_explain_target, is_issue_id, is_pasted_detail_id, is_trace_id};
use crate::{
    CliError, Command, ExplainTarget, HelpTopic, ISSUE_STATUS_ARGUMENT_NEXT_STEP,
    ProjectCreateOptions, ProjectSetupSeenOptions, ReadOptions, ReadTarget, SetTarget,
    auth_namespace,
};

/// Standard next step for malformed help invocations.
const HELP_NEXT_STEP: &str = "run logbrew --help";
/// Valid resources for historical reads.
const READ_RESOURCE_NEXT_STEP: &str =
    "choose one of logs, issues, actions, releases, traces, trace, issue";
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
/// Help for recent trace discovery.
const READ_TRACES_NEXT_STEP: &str = "run logbrew read traces --help";
/// Help for backend-owned project setup discovery.
const PROJECTS_NEXT_STEP: &str = "run logbrew projects --help";
/// Help for backend-owned project setup seen calls.
const PROJECT_SETUP_SEEN_NEXT_STEP: &str = "run logbrew projects setup <project_id> --help";
/// Valid setup source values for setup seen calls.
const PROJECT_SETUP_SOURCE_NEXT_STEP: &str = "use --source api, cli, or sdk";
/// Valid resources for live watch.
const WATCH_RESOURCE_NEXT_STEP: &str = "choose logs, issues, actions, or omit a resource";
/// Valid resources for explain.
const EXPLAIN_RESOURCE_NEXT_STEP: &str = "choose issue or trace";
/// Valid resources for state mutation.
const SET_RESOURCE_NEXT_STEP: &str = "choose issue";
/// Filters trace detail reads cannot apply.
const TRACE_DETAIL_UNSUPPORTED_FLAGS: &[&str] = &[
    "--name",
    "--service",
    "--service-name",
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
    "--min-duration-ms",
];
/// Filters issue detail reads cannot apply.
const ISSUE_DETAIL_UNSUPPORTED_FLAGS: &[&str] = &[
    "--name",
    "--service",
    "--service-name",
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
    "--min-duration-ms",
];
/// Filters action list reads cannot apply.
const ACTION_LIST_UNSUPPORTED_FLAGS: &[&str] = &[
    "--trace",
    "--trace-id",
    "--level",
    "--severity",
    "--search",
    "--status",
    "--min-duration-ms",
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
    if is_setup_alias(head) && tail.iter().any(|arg| arg == "--create-project") {
        return parse_setup_create_project(tail);
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
        "status" | "whoami" | "me" | "health" | "ping" => parse_status(tail),
        "doctor" => parse_doctor(tail),
        "version" => parse_version(tail),
        "account" if tail.first().is_some_and(|arg| arg == "usage") => parse_usage(&tail[1..]),
        alias if auth_namespace::is_namespace(alias) => auth_namespace::parse(tail),
        alias if auth_namespace::is_help_alias(alias) => parse_help_alias(HelpTopic::Auth, tail),
        "json" | "output" => parse_help_alias(HelpTopic::Json, tail),
        alias if is_examples_help_alias(alias) => parse_help_alias(HelpTopic::Examples, tail),
        alias if is_project_help_alias(alias) => parse_project(tail),
        "usage" => parse_usage(tail),
        "support" => parse_support(tail),
        "investigate" => parse_investigate(tail),
        "debug-artifacts" => parse_native_debug_artifacts(tail),
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
        "logs" | "issues" | "errors" | "error" | "exceptions" | "exception" | "actions"
        | "events" | "event" | "action" | "releases" | "trace" | "issue" => {
            parse_read_resource(head, tail)
        }
        "span" if has_position_candidate(tail) => parse_read_resource("trace", tail),
        "resolve" | "close" | "ignore" | "reopen" => parse_issue_status_shortcut(head, tail),
        alias if is_watch_command_alias(alias) => parse_watch(tail),
        "explain" => parse_explain(tail),
        "set" => parse_set(tail),
        id if is_pasted_detail_id(id) => parse_pasted_detail_id(id, tail),
        _ => Err(unknown_command(head)),
    }
}

/// Parses the closed Apple native debug-artifact grammar.
fn parse_native_debug_artifacts(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let Some((operation, tail)) = normalized.split_first() else {
        return Err(CliError::InvalidNativeDebugCommand);
    };
    match operation.as_str() {
        "upload" => parse_native_debug_upload(tail),
        "lookup" => parse_native_debug_lookup(tail),
        _ => Err(CliError::InvalidNativeDebugCommand),
    }
}

/// Parses one artifact upload and normalizes its public request scope.
fn parse_native_debug_upload(args: &[String]) -> Result<Command, CliError> {
    let Some((path, flags)) = args.split_first() else {
        return Err(CliError::InvalidNativeDebugCommand);
    };
    if path.is_empty() || path.chars().any(char::is_control) || path.starts_with('-') {
        return Err(CliError::InvalidNativeDebugCommand);
    }
    let parsed = parse_native_debug_scope(flags, false)?;
    Ok(Command::NativeDebugArtifacts {
        target: crate::NativeDebugArtifactsTarget::Upload(crate::NativeDebugUploadOptions {
            path: path.clone(),
            project_id: parsed.project_id,
            release: parsed.release,
            environment: parsed.environment,
            service: parsed.service,
        }),
        json: parsed.json,
    })
}

/// Parses one exact artifact lookup.
fn parse_native_debug_lookup(args: &[String]) -> Result<Command, CliError> {
    let parsed = parse_native_debug_scope(args, true)?;
    let image_uuid = parsed
        .image_uuid
        .filter(|value| is_canonical_lower_uuid(value))
        .ok_or(CliError::InvalidNativeDebugIdentity)?;
    let architecture = parsed
        .architecture
        .filter(|value| matches!(value.as_str(), "arm64" | "arm64e" | "x86_64"))
        .ok_or(CliError::InvalidNativeDebugIdentity)?;
    Ok(Command::NativeDebugArtifacts {
        target: crate::NativeDebugArtifactsTarget::Lookup(crate::NativeDebugLookupOptions {
            project_id: parsed.project_id,
            release: parsed.release,
            environment: parsed.environment,
            service: parsed.service,
            image_uuid,
            architecture,
        }),
        json: parsed.json,
    })
}

/// Duplicate-aware native debug-artifact flag accumulator.
#[derive(Default)]
struct NativeDebugScope {
    /// Account-owned project UUID.
    project_id: String,
    /// Exact normalized release.
    release: String,
    /// Exact normalized environment.
    environment: String,
    /// Exact normalized service.
    service: String,
    /// Optional lookup image UUID.
    image_uuid: Option<String>,
    /// Optional lookup architecture.
    architecture: Option<String>,
    /// Machine-readable output selection.
    json: bool,
}

/// Parses required scope flags without reflecting malformed values.
fn parse_native_debug_scope(args: &[String], lookup: bool) -> Result<NativeDebugScope, CliError> {
    let mut project_id = None;
    let mut release = None;
    let mut environment = None;
    let mut service = None;
    let mut image_uuid = None;
    let mut architecture = None;
    let mut json = false;
    let mut index = 0;
    while let Some(flag) = args.get(index) {
        if flag == "--json" {
            if json {
                return Err(CliError::InvalidNativeDebugCommand);
            }
            json = true;
            index += 1;
            continue;
        }
        let destination = match flag.as_str() {
            "--project" => &mut project_id,
            "--release" => &mut release,
            "--environment" => &mut environment,
            "--service" => &mut service,
            "--image-uuid" if lookup => &mut image_uuid,
            "--architecture" if lookup => &mut architecture,
            _ => return Err(CliError::InvalidNativeDebugCommand),
        };
        if destination.is_some() {
            return Err(CliError::InvalidNativeDebugCommand);
        }
        let value = args
            .get(index + 1)
            .ok_or(CliError::InvalidNativeDebugCommand)?;
        *destination = Some(value.clone());
        index += 2;
    }

    let project_id = project_id
        .filter(|value| is_canonical_lower_uuid(value))
        .ok_or(CliError::InvalidNativeDebugCommand)?;
    let release = normalize_native_scope(release).ok_or(CliError::InvalidNativeDebugCommand)?;
    let environment =
        normalize_native_scope(environment).ok_or(CliError::InvalidNativeDebugCommand)?;
    let service = normalize_native_scope(service).ok_or(CliError::InvalidNativeDebugCommand)?;
    if lookup != (image_uuid.is_some() && architecture.is_some()) {
        return Err(CliError::InvalidNativeDebugCommand);
    }
    Ok(NativeDebugScope {
        project_id,
        release,
        environment,
        service,
        image_uuid,
        architecture,
        json,
    })
}

/// Trims and bounds one public native artifact scope string.
fn normalize_native_scope(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed.len() <= 256 && !trimmed.chars().any(char::is_control))
        .then(|| trimmed.to_owned())
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

/// Parses the closed, read-only issue investigation grammar.
fn parse_investigate(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    match normalized.as_slice() {
        [resource, issue_id]
            if resource == "issue" && is_safe_investigation_issue_id(issue_id.as_str()) =>
        {
            Ok(Command::InvestigateIssue {
                issue_id: issue_id.clone(),
                json: false,
            })
        }
        [resource, issue_id, json]
            if resource == "issue"
                && is_safe_investigation_issue_id(issue_id.as_str())
                && json == "--json" =>
        {
            Ok(Command::InvestigateIssue {
                issue_id: issue_id.clone(),
                json: true,
            })
        }
        _ => Err(CliError::InvalidInvestigationCommand),
    }
}

/// Restricts investigation IDs to canonical lowercase dashed UUIDs.
fn is_safe_investigation_issue_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23)
                    && (byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
}

/// Parses non-mutating discovery help for backend-owned future workflows.
fn parse_discovery_help(topic: HelpTopic, args: &[String]) -> Result<Command, CliError> {
    validate_help_flags(args)?;
    Ok(Command::Help {
        topic,
        json: args.iter().any(|arg| arg == "--json"),
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
    let flags = parse_flags(args, FlagScope::Setup)?;
    Ok(Command::Setup {
        auto: flags.is_auto(),
        yes: flags.skip_prompts(),
        json: flags.is_json(),
    })
}

/// Parses the help-only backend project creation shape advertised by setup help.
fn parse_setup_create_project(args: &[String]) -> Result<Command, CliError> {
    let mut seen_create_project = false;
    let mut seen_json = false;

    for arg in args {
        match arg.as_str() {
            "--create-project" => {
                if std::mem::replace(&mut seen_create_project, true) {
                    return Err(CliError::DuplicateFlag {
                        flag: "--create-project",
                        next: "use --create-project once",
                    });
                }
            }
            "--json" => {
                if std::mem::replace(&mut seen_json, true) {
                    return Err(CliError::DuplicateFlag {
                        flag: "--json",
                        next: "use --json once",
                    });
                }
            }
            "--help" | "-h" => {}
            flag if flag.starts_with('-') => {
                return Err(unknown_flag(flag, PROJECTS_NEXT_STEP));
            }
            argument => {
                return Err(CliError::UnexpectedArgument {
                    argument: argument.to_owned(),
                    command: "setup",
                    next: PROJECTS_NEXT_STEP,
                });
            }
        }
    }

    Ok(Command::Help {
        topic: HelpTopic::Projects,
        json: seen_json,
    })
}

/// Parses backend-owned project commands.
fn parse_project(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    if let Some((subcommand, tail)) = normalized.split_first()
        && subcommand == "create"
    {
        return parse_project_create(tail);
    }
    if let Some((subcommand, tail)) = normalized.split_first()
        && subcommand == "setup"
        && has_position_candidate(tail)
    {
        return parse_project_setup_seen(tail);
    }
    parse_discovery_help(HelpTopic::Projects, args)
}

/// Parses the closed secure project creation grammar.
fn parse_project_create(args: &[String]) -> Result<Command, CliError> {
    let Some((name, tail)) = args.split_first() else {
        return Err(CliError::InvalidProjectCreateCommand);
    };
    if name.starts_with('-') {
        return Err(CliError::InvalidProjectCreateCommand);
    }
    let name = bounded_project_create_value(name, 120, false)
        .ok_or(CliError::InvalidProjectCreateCommand)?;
    if name.starts_with('-') {
        return Err(CliError::InvalidProjectCreateCommand);
    }
    let mut runtime = None;
    let mut environment = None;
    let mut ingest_key_file = None;
    let mut abandon_retry = false;
    let mut json = false;
    let mut index = 0;

    while let Some(argument) = tail.get(index) {
        let (flag, inline_value) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(flag, value)| {
                (flag, Some(value))
            });
        match flag {
            "--runtime" if runtime.is_none() => {
                let value = project_create_flag_value(tail, &mut index, inline_value)?;
                runtime = optional_project_create_value(value, 64)?;
            }
            "--environment" if environment.is_none() => {
                let value = project_create_flag_value(tail, &mut index, inline_value)?;
                environment = optional_project_create_value(value, 64)?;
            }
            "--ingest-key-file" if ingest_key_file.is_none() => {
                let value = project_create_flag_value(tail, &mut index, inline_value)?;
                let trimmed = value.trim();
                if trimmed.is_empty()
                    || trimmed.len() > 4096
                    || trimmed.chars().any(char::is_control)
                {
                    return Err(CliError::InvalidProjectCreateCommand);
                }
                ingest_key_file = Some(trimmed.to_owned());
            }
            "--abandon-retry" if inline_value.is_none() && !abandon_retry => {
                abandon_retry = true;
            }
            "--json" if inline_value.is_none() && !json => json = true,
            _ => return Err(CliError::InvalidProjectCreateCommand),
        }
        index += 1;
    }

    let ingest_key_file = ingest_key_file.ok_or(CliError::InvalidProjectCreateCommand)?;
    Ok(Command::ProjectCreate {
        options: ProjectCreateOptions {
            name,
            runtime,
            environment,
            ingest_key_file,
            abandon_retry,
        },
        json,
    })
}

/// Takes an inline or following project-create flag value without reflection.
fn project_create_flag_value<'a>(
    args: &'a [String],
    index: &mut usize,
    inline: Option<&'a str>,
) -> Result<&'a str, CliError> {
    if let Some(value) = inline {
        return Ok(value);
    }
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .ok_or(CliError::InvalidProjectCreateCommand)
}

/// Trims one bounded control-safe project-create field.
fn bounded_project_create_value(value: &str, limit: usize, allow_blank: bool) -> Option<String> {
    let value = value.trim();
    let length = value.chars().count();
    if value.chars().any(char::is_control) || length > limit || (!allow_blank && length == 0) {
        return None;
    }
    (!value.is_empty()).then(|| value.to_owned())
}

/// Normalizes one optional field while distinguishing blank from invalid.
fn optional_project_create_value(value: &str, limit: usize) -> Result<Option<String>, CliError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    bounded_project_create_value(trimmed, limit, false)
        .map(Some)
        .ok_or(CliError::InvalidProjectCreateCommand)
}

/// Parses `projects setup <project_id>`.
fn parse_project_setup_seen(args: &[String]) -> Result<Command, CliError> {
    let (project_id, tail) =
        take_required_position(args, "project_id", PROJECT_SETUP_SEEN_NEXT_STEP)?;
    let (options, json) = parse_project_setup_seen_flags(tail.as_slice())?;
    Ok(Command::ProjectSetupSeen {
        project_id,
        options,
        json,
    })
}

/// Parses flags for backend setup seen calls.
fn parse_project_setup_seen_flags(
    args: &[String],
) -> Result<(ProjectSetupSeenOptions, bool), CliError> {
    let mut options = ProjectSetupSeenOptions::default();
    let mut json = false;
    let mut seen = Vec::new();
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        let (flag, inline_value) = split_project_setup_seen_inline_value(arg.as_str());
        match flag {
            "--json" if inline_value.is_none() => {
                mark_project_setup_seen_flag(&mut seen, "--json")?;
                json = true;
            }
            "--runtime" => {
                mark_project_setup_seen_flag(&mut seen, "--runtime")?;
                options.runtime = Some(project_setup_seen_flag_value(
                    args,
                    &mut index,
                    "--runtime",
                    inline_value,
                )?);
            }
            "--source" => {
                mark_project_setup_seen_flag(&mut seen, "--source")?;
                options.source = Some(validate_project_setup_seen_source(
                    project_setup_seen_flag_value(args, &mut index, "--source", inline_value)?
                        .as_str(),
                )?);
            }
            "--environment" | "--env" => {
                mark_project_setup_seen_flag(&mut seen, "--environment")?;
                let visible_flag = if flag == "--env" {
                    "--env"
                } else {
                    "--environment"
                };
                options.environment = Some(project_setup_seen_flag_value(
                    args,
                    &mut index,
                    visible_flag,
                    inline_value,
                )?);
            }
            flag if flag.starts_with('-') => {
                return Err(unknown_flag(flag, PROJECT_SETUP_SEEN_NEXT_STEP));
            }
            argument => {
                return Err(CliError::UnexpectedArgument {
                    argument: argument.to_owned(),
                    command: "projects setup",
                    next: PROJECT_SETUP_SEEN_NEXT_STEP,
                });
            }
        }
        index += 1;
    }

    Ok((options, json))
}

/// Splits a value-taking project setup flag.
fn split_project_setup_seen_inline_value(flag: &str) -> (&str, Option<&str>) {
    flag.split_once('=')
        .map_or((flag, None), |(name, value)| (name, Some(value)))
}

/// Records a project setup flag and rejects duplicate occurrences.
fn mark_project_setup_seen_flag(
    seen: &mut Vec<&'static str>,
    flag: &'static str,
) -> Result<(), CliError> {
    if seen.contains(&flag) {
        return Err(CliError::DuplicateFlag {
            flag,
            next: project_setup_seen_duplicate_next(flag),
        });
    }
    seen.push(flag);
    Ok(())
}

/// Returns the recovery step for duplicate project setup flags.
fn project_setup_seen_duplicate_next(flag: &'static str) -> &'static str {
    match flag {
        "--json" => "use --json once",
        "--runtime" => "use --runtime once",
        "--source" => "use --source once",
        "--environment" => "use --environment once",
        _ => "use the flag once",
    }
}

/// Reads a value for a project setup flag.
fn project_setup_seen_flag_value(
    args: &[String],
    index: &mut usize,
    flag: &'static str,
    inline_value: Option<&str>,
) -> Result<String, CliError> {
    if let Some(value) = inline_value {
        if value.is_empty() {
            return Err(missing_project_setup_seen_flag_value(flag));
        }
        return Ok(value.to_owned());
    }
    *index += 1;
    let Some(value) = args.get(*index) else {
        return Err(missing_project_setup_seen_flag_value(flag));
    };
    if value.starts_with('-') {
        return Err(missing_project_setup_seen_flag_value(flag));
    }
    Ok(value.clone())
}

/// Builds a missing-value error for project setup flags.
fn missing_project_setup_seen_flag_value(flag: &'static str) -> CliError {
    CliError::MissingFlagValue {
        flag,
        next: project_setup_seen_missing_value_next(flag),
    }
}

/// Returns the recovery step for missing project setup flag values.
fn project_setup_seen_missing_value_next(flag: &'static str) -> &'static str {
    match flag {
        "--runtime" => "provide a value after --runtime",
        "--source" => PROJECT_SETUP_SOURCE_NEXT_STEP,
        "--environment" => "provide a value after --environment",
        "--env" => "provide a value after --env",
        _ => "provide a value after the flag",
    }
}

/// Validates setup source values accepted by the public backend contract.
fn validate_project_setup_seen_source(source: &str) -> Result<String, CliError> {
    match source {
        "api" | "cli" | "sdk" => Ok(source.to_owned()),
        other => Err(CliError::InvalidSetupSource(other.to_owned())),
    }
}

/// Parses `status`.
fn parse_status(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Status)?;
    Ok(Command::Status {
        json: flags.is_json(),
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
            if project_id.is_some() || !crate::ids::is_uuid(value) {
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
                if !crate::ids::is_uuid(value) {
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
                "--user",
                "--distinct-id",
                "--trace",
                "--trace-id",
                "--level",
                "--severity",
                "--search",
                "--status",
                "--min-duration-ms",
            ],
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
        &[
            "--name",
            "--user",
            "--distinct-id",
            "--status",
            "--min-duration-ms",
        ],
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
            "--user",
            "--distinct-id",
            "--trace",
            "--trace-id",
            "--level",
            "--severity",
            "--search",
            "--min-duration-ms",
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
            | "--service"
            | "--service-name"
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
            | "--min-duration-ms"
            | "--pagination"
            | "--cursor-time"
            | "--cursor-id"
    )
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
        ReadTarget::Traces => filters
            .first_trace_list_unsupported_flag()
            .map(|flag| (flag, "read traces", READ_TRACES_NEXT_STEP)),
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
    match target {
        ReadTarget::Logs => validate_read_cursor(filters, CliError::InvalidLogCursor)?,
        ReadTarget::Actions => validate_read_cursor(filters, CliError::InvalidActionCursor)?,
        ReadTarget::Issues => validate_read_cursor(filters, CliError::InvalidIssueCursor)?,
        ReadTarget::Releases | ReadTarget::Traces | ReadTarget::Trace(_) | ReadTarget::Issue(_) => {
        }
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
