//! CLI command grammar.

mod analytics;
mod explain;
mod help_topics;
mod issue_shortcuts;
mod log_shortcuts;
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
use support::parse_support;
use watch::parse_watch;

use crate::flags::{FlagScope, normalize_status, parse_flags};
use crate::ids::{is_non_nil_uuid, is_pasted_detail_id, is_trace_id, is_uuid};
use crate::{
    CliError, Command, HelpTopic, ISSUE_STATUS_ARGUMENT_NEXT_STEP, LoginProvider,
    ProjectCreateOptions, ProjectIngestKeyCreateOptions, ProjectRepositoryOptions,
    ProjectSetupSeenOptions, RepositoryDiscoveryOptions, RepositorySetupTarget, SetTarget,
    auth_namespace,
};
use explain::{parse_explain, parse_investigate};
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
/// Help for backend-owned project setup discovery.
const PROJECTS_NEXT_STEP: &str = "run logbrew projects --help";
/// Help for backend-owned project setup seen calls.
const PROJECT_SETUP_SEEN_NEXT_STEP: &str = "run logbrew projects setup <project_id> --help";
/// Valid setup source values for setup seen calls.
const PROJECT_SETUP_SOURCE_NEXT_STEP: &str = "use --source api, cli, or sdk";
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
        "mcp" => parse_mcp(tail),
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
            expected_image_uuids: parsed.expected_image_uuids,
            dry_run: parsed.dry_run,
        }),
        json: parsed.json,
    })
}

/// Parses one exact artifact lookup.
fn parse_native_debug_lookup(args: &[String]) -> Result<Command, CliError> {
    let parsed = parse_native_debug_scope(args, true)?;
    let image_uuid = parsed
        .image_uuid
        .and_then(|value| normalize_native_debug_uuid(value.as_str()))
        .ok_or(CliError::InvalidNativeDebugIdentity)?;
    let architecture = parsed
        .architecture
        .filter(|value| ["arm", "arm64", "arm64e", "x86", "x86_64"].contains(&value.as_str()))
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
    /// Optional exact image UUID set for upload gating.
    expected_image_uuids: Vec<String>,
    /// Optional lookup architecture.
    architecture: Option<String>,
    /// Local-only artifact validation.
    dry_run: bool,
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
    let mut expected_image_uuids = Vec::new();
    let mut architecture = None;
    let mut dry_run = false;
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
        if flag == "--dry-run" && !lookup {
            if dry_run {
                return Err(CliError::InvalidNativeDebugCommand);
            }
            dry_run = true;
            index += 1;
            continue;
        }
        if flag == "--expect-image-uuid" && !lookup {
            let value = args
                .get(index + 1)
                .and_then(|value| normalize_native_debug_uuid(value.as_str()))
                .ok_or(CliError::InvalidNativeDebugIdentity)?;
            if expected_image_uuids
                .iter()
                .any(|existing| existing == &value)
            {
                return Err(CliError::InvalidNativeDebugIdentity);
            }
            expected_image_uuids.push(value);
            index += 2;
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
    if lookup && (!expected_image_uuids.is_empty() || dry_run) {
        return Err(CliError::InvalidNativeDebugCommand);
    }
    expected_image_uuids.sort();
    Ok(NativeDebugScope {
        project_id,
        release,
        environment,
        service,
        image_uuid,
        expected_image_uuids,
        architecture,
        dry_run,
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

/// Validates and canonicalizes one public native debug UUID.
fn normalize_native_debug_uuid(value: &str) -> Option<String> {
    if value.len() != 36 {
        return None;
    }
    let mut normalized = String::with_capacity(36);
    for (index, byte) in value.bytes().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
            normalized.push('-');
        } else if byte.is_ascii_hexdigit() {
            normalized.push(char::from(byte.to_ascii_lowercase()));
        } else {
            return None;
        }
    }
    Some(normalized)
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
    match move_leading_json_to_tail(args).as_slice() {
        [] => Ok(Command::Projects { json: false }),
        [flag] if flag == "--json" => Ok(Command::Projects { json: true }),
        [namespace, subcommand, tail @ ..]
            if matches!(namespace.as_str(), "key" | "keys") && subcommand == "create" =>
        {
            parse_project_ingest_key_create(tail)
        }
        [subcommand, tail @ ..] if subcommand == "create" => parse_project_create(tail),
        [subcommand, tail @ ..] if subcommand == "archive" => parse_project_archive(tail),
        [subcommand, tail @ ..] if subcommand == "delete" => parse_project_deletion(tail),
        [subcommand, tail @ ..] if subcommand == "repositories" => parse_project_repositories(tail),
        [subcommand, tail @ ..] if subcommand == "setup" && has_position_candidate(tail) => {
            parse_project_setup_seen(tail)
        }
        [_, ..] => Err(CliError::InvalidProjectsCommand),
    }
}

/// Parses repository catalog and bounded component-discovery reads.
fn parse_project_repositories(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    match normalized.as_slice() {
        [] => Ok(Command::ProjectRepositories {
            target: RepositorySetupTarget::Catalog,
            json: false,
        }),
        [json] if json == "--json" => Ok(Command::ProjectRepositories {
            target: RepositorySetupTarget::Catalog,
            json: true,
        }),
        [discover, tail @ ..] if discover == "discover" => parse_repository_discovery(tail),
        [_, ..] => Err(CliError::InvalidProjectsCommand),
    }
}

/// Parses one explicit provider repository component-discovery request.
fn parse_repository_discovery(args: &[String]) -> Result<Command, CliError> {
    let mut provider = None;
    let mut repository_id = None;
    let mut json = false;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(flag, value)| {
                (flag, Some(value))
            });
        match flag {
            "--provider" if provider.is_none() => {
                let value =
                    flag_value(args, &mut index, inline).ok_or(CliError::InvalidProjectsCommand)?;
                provider = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|provider: &LoginProvider| provider.hosts_repositories())
                        .ok_or(CliError::InvalidProjectsCommand)?,
                );
            }
            "--repository" if repository_id.is_none() => {
                let value =
                    flag_value(args, &mut index, inline).ok_or(CliError::InvalidProjectsCommand)?;
                repository_id = bounded_project_create_value(value, 256, false);
                if repository_id.is_none() {
                    return Err(CliError::InvalidProjectsCommand);
                }
            }
            "--json" if inline.is_none() && !json => json = true,
            _ => return Err(CliError::InvalidProjectsCommand),
        }
        index += 1;
    }
    Ok(Command::ProjectRepositories {
        target: RepositorySetupTarget::Discover(RepositoryDiscoveryOptions {
            provider: provider.ok_or(CliError::InvalidProjectsCommand)?,
            repository_id: repository_id.ok_or(CliError::InvalidProjectsCommand)?,
        }),
        json,
    })
}

/// Parses the closed secure existing-project ingest-key creation grammar.
fn parse_project_ingest_key_create(args: &[String]) -> Result<Command, CliError> {
    let Some((project_id, tail)) = args.split_first() else {
        return Err(CliError::InvalidProjectIngestKeyCreateCommand);
    };
    if !is_uuid(project_id) {
        return Err(CliError::InvalidProjectIngestKeyCreateCommand);
    }
    let mut label = None;
    let mut kind = None;
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
            "--label" if label.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectIngestKeyCreateCommand)?;
                label = Some(
                    bounded_project_create_value(value, 120, false)
                        .ok_or(CliError::InvalidProjectIngestKeyCreateCommand)?,
                );
            }
            "--kind" if kind.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectIngestKeyCreateCommand)?;
                kind = Some(match value.trim() {
                    "sdk" => String::from("sdk"),
                    "browser" => String::from("browser"),
                    "server" => String::from("server"),
                    "cli" => String::from("cli"),
                    "artifact" => String::from("artifact"),
                    _ => return Err(CliError::InvalidProjectIngestKeyCreateCommand),
                });
            }
            "--ingest-key-file" if ingest_key_file.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectIngestKeyCreateCommand)?;
                let trimmed = value.trim();
                if trimmed.is_empty()
                    || trimmed.len() > 4096
                    || trimmed.chars().any(char::is_control)
                {
                    return Err(CliError::InvalidProjectIngestKeyCreateCommand);
                }
                ingest_key_file = Some(trimmed.to_owned());
            }
            "--abandon-retry" if inline_value.is_none() && !abandon_retry => {
                abandon_retry = true;
            }
            "--json" if inline_value.is_none() && !json => json = true,
            _ => return Err(CliError::InvalidProjectIngestKeyCreateCommand),
        }
        index += 1;
    }

    Ok(Command::ProjectIngestKeyCreate {
        options: ProjectIngestKeyCreateOptions {
            project_id: project_id.to_ascii_lowercase(),
            label: label.unwrap_or_else(|| String::from("CLI-created SDK key")),
            kind: kind.unwrap_or_else(|| String::from("sdk")),
            ingest_key_file: ingest_key_file
                .ok_or(CliError::InvalidProjectIngestKeyCreateCommand)?,
            abandon_retry,
        },
        json,
    })
}

/// Parses the closed, explicitly confirmed project archival grammar.
fn parse_project_archive(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    let [project_id, confirmation] = values.as_slice() else {
        return Err(CliError::InvalidProjectArchiveCommand);
    };
    if confirmation != &"--yes" || json_count > 1 || !is_uuid(project_id) {
        return Err(CliError::InvalidProjectArchiveCommand);
    }
    Ok(Command::ProjectArchive {
        project_id: project_id.to_ascii_lowercase(),
        json: json_count == 1,
    })
}

/// Parses irreversible deletion only after the caller repeats the exact project UUID.
fn parse_project_deletion(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    let (project_id, confirmation) = match values.as_slice() {
        [project_id, "--confirm", confirmation] => (*project_id, *confirmation),
        [project_id, inline] => (
            *project_id,
            inline
                .strip_prefix("--confirm=")
                .ok_or(CliError::InvalidProjectDeletionCommand)?,
        ),
        _ => return Err(CliError::InvalidProjectDeletionCommand),
    };
    if json_count > 1 || !is_uuid(project_id) || !project_id.eq_ignore_ascii_case(confirmation) {
        return Err(CliError::InvalidProjectDeletionCommand);
    }
    Ok(Command::ProjectDeletion {
        project_id: project_id.to_ascii_lowercase(),
        json: json_count == 1,
    })
}

/// Parses the closed secure project creation grammar.
#[expect(
    clippy::too_many_lines,
    reason = "one closed grammar keeps mutually dependent project creation flags visible"
)]
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
    let mut provider = None;
    let mut repository_id = None;
    let mut discovery_id = None;
    let mut component_ids = Vec::new();
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
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectCreateCommand)?;
                runtime = optional_project_create_value(value, 64)?;
            }
            "--environment" if environment.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectCreateCommand)?;
                environment = optional_project_create_value(value, 64)?;
            }
            "--provider" if provider.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectCreateCommand)?;
                provider = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|provider: &LoginProvider| provider.hosts_repositories())
                        .ok_or(CliError::InvalidProjectCreateCommand)?,
                );
            }
            "--repository" if repository_id.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectCreateCommand)?;
                repository_id = bounded_project_create_value(value, 256, false);
                if repository_id.is_none() {
                    return Err(CliError::InvalidProjectCreateCommand);
                }
            }
            "--discovery" if discovery_id.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectCreateCommand)?;
                if !is_non_nil_uuid(value) {
                    return Err(CliError::InvalidProjectCreateCommand);
                }
                discovery_id = Some(value.to_ascii_lowercase());
            }
            "--component" if component_ids.len() < 32 => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectCreateCommand)?;
                if !is_non_nil_uuid(value) {
                    return Err(CliError::InvalidProjectCreateCommand);
                }
                component_ids.push(value.to_ascii_lowercase());
            }
            "--ingest-key-file" if ingest_key_file.is_none() => {
                let value = flag_value(tail, &mut index, inline_value)
                    .ok_or(CliError::InvalidProjectCreateCommand)?;
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

    component_ids.sort_unstable();
    if component_ids.windows(2).any(|pair| pair[0] == pair[1])
        || provider.is_some() != repository_id.is_some()
        || discovery_id.is_some() == component_ids.is_empty()
        || repository_id.is_none() && discovery_id.is_some()
    {
        return Err(CliError::InvalidProjectCreateCommand);
    }
    let repository = provider
        .zip(repository_id)
        .map(|(provider, id)| ProjectRepositoryOptions {
            provider,
            id,
            discovery_id,
            component_ids,
        });
    let ingest_key_file = ingest_key_file.ok_or(CliError::InvalidProjectCreateCommand)?;
    Ok(Command::ProjectCreate {
        options: ProjectCreateOptions {
            name,
            runtime,
            environment,
            repository,
            ingest_key_file,
            abandon_retry,
        },
        json,
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
