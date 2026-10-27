//! Grammar for project commands and setup project creation.

use super::{
    flag_value, has_position_candidate, move_leading_json_to_tail, take_required_position,
    unknown_flag,
};
use crate::ids::{is_non_nil_uuid, is_uuid};
use crate::{
    CliError, Command, HelpTopic, LoginProvider, ProjectCreateOptions,
    ProjectIngestKeyCreateOptions, ProjectRepositoryOptions, ProjectSetupSeenOptions,
    RepositoryDiscoveryOptions, RepositorySetupTarget,
};

/// Help for backend-owned project setup discovery.
const PROJECTS_NEXT_STEP: &str = "run logbrew projects --help";
/// Help for backend-owned project setup seen calls.
const PROJECT_SETUP_SEEN_NEXT_STEP: &str = "run logbrew projects setup <project_id> --help";
/// Valid setup source values for setup seen calls.
const PROJECT_SETUP_SOURCE_NEXT_STEP: &str = "use --source api, cli, or sdk";

/// Parses the help-only backend project creation shape advertised by setup help.
pub(super) fn parse_setup_create_project(args: &[String]) -> Result<Command, CliError> {
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
pub(super) fn parse_project(args: &[String]) -> Result<Command, CliError> {
    match move_leading_json_to_tail(args).as_slice() {
        [] => Ok(Command::Projects { json: false }),
        [flag] if flag == "--json" => Ok(Command::Projects { json: true }),
        [namespace, subcommand, tail @ ..]
            if matches!(namespace.as_str(), "key" | "keys") && subcommand == "create" =>
        {
            parse_project_ingest_key_create(tail)
        }
        [namespace, subcommand, tail @ ..]
            if matches!(namespace.as_str(), "key" | "keys") && subcommand == "revoke" =>
        {
            parse_project_key_revoke(tail)
        }
        [namespace, subcommand, tail @ ..]
            if matches!(namespace.as_str(), "key" | "keys") && subcommand == "list" =>
        {
            parse_project_key_list(tail)
        }
        [subcommand, tail @ ..] if subcommand == "rename" => parse_project_rename(tail),
        [subcommand, tail @ ..] if subcommand == "restore" => parse_project_restore(tail),
        [subcommand, tail @ ..] if subcommand == "archived" => match tail {
            [] => Ok(Command::ProjectArchivedList { json: false }),
            [flag] if flag == "--json" => Ok(Command::ProjectArchivedList { json: true }),
            _ => Err(CliError::InvalidProjectsCommand),
        },
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
                    "read" => String::from("read"),
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

/// Parses the closed, explicitly confirmed project key revocation grammar.
fn parse_project_key_revoke(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    match values.as_slice() {
        [project_id, key_id, "--yes"]
            if json_count <= 1 && is_uuid(project_id) && is_uuid(key_id) =>
        {
            Ok(Command::ProjectKeyRevoke {
                project_id: project_id.to_ascii_lowercase(),
                key_id: key_id.to_ascii_lowercase(),
                json: json_count == 1,
            })
        }
        _ => Err(CliError::InvalidProjectsCommand),
    }
}

/// Parses `keys list <project_id> [--json]`.
fn parse_project_key_list(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    match values.as_slice() {
        [project_id] if json_count <= 1 && is_uuid(project_id) => Ok(Command::ProjectKeyList {
            project_id: project_id.to_ascii_lowercase(),
            json: json_count == 1,
        }),
        _ => Err(CliError::InvalidProjectsCommand),
    }
}

/// Parses `restore <project_id> [--json]`.
fn parse_project_restore(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    match values.as_slice() {
        [project_id] if json_count <= 1 && is_uuid(project_id) => Ok(Command::ProjectRestore {
            project_id: project_id.to_ascii_lowercase(),
            json: json_count == 1,
        }),
        _ => Err(CliError::InvalidProjectsCommand),
    }
}

/// Parses `rename <project_id> --name <name> [--json]`.
fn parse_project_rename(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let json_count = normalized.iter().filter(|value| *value == "--json").count();
    let values = normalized
        .iter()
        .map(String::as_str)
        .filter(|value| *value != "--json")
        .collect::<Vec<_>>();
    let [project_id, "--name", name] = values.as_slice() else {
        return Err(CliError::InvalidProjectsCommand);
    };
    let name = bounded_project_create_value(name, 120, false)
        .filter(|name| !name.starts_with('-'))
        .ok_or(CliError::InvalidProjectsCommand)?;
    if json_count > 1 || !is_uuid(project_id) {
        return Err(CliError::InvalidProjectsCommand);
    }
    Ok(Command::ProjectRename {
        project_id: project_id.to_ascii_lowercase(),
        name,
        json: json_count == 1,
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
