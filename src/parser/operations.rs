//! Operations performance view grammar.

use crate::{CliError, Command, OperationsOptions};

/// Parses `logbrew operations [list|show <id>]` with window, context, and output flags.
pub(super) fn parse_operations(args: &[String]) -> Result<Command, CliError> {
    let (operation, flags) = match args.split_first() {
        None => (None, args),
        Some((action, tail)) if action == "list" => (None, tail),
        Some((action, tail)) if action == "show" => match tail.split_first() {
            Some((id, flags)) if crate::ids::is_operation_id(id) => (Some(id.clone()), flags),
            Some(_) => return Err(CliError::InvalidOperationId),
            None => {
                return Err(CliError::MissingArgument {
                    argument: "operation_id",
                    next: "run logbrew operations list for the ids",
                });
            }
        },
        Some((flag, _)) if flag.starts_with("--") => (None, args),
        Some(_) => return Err(CliError::InvalidOperationsCommand),
    };
    let mut options = OperationsOptions {
        operation,
        ..OperationsOptions::default()
    };
    let mut json = false;
    let mut index = 0;
    while let Some(raw) = flags.get(index) {
        let (flag, inline) = raw
            .split_once('=')
            .map_or((raw.as_str(), None), |(flag, value)| (flag, Some(value)));
        if flag == "--json" {
            if inline.is_some() || json {
                return Err(CliError::InvalidOperationsCommand);
            }
            json = true;
            index += 1;
            continue;
        }
        let slot = match flag {
            "--since" => &mut options.since,
            "--until" => &mut options.until,
            "--environment" | "--env" => &mut options.environment,
            "--release" => &mut options.release,
            _ => return Err(CliError::InvalidOperationsCommand),
        };
        if inline.is_none() {
            index += 1;
        }
        let value = inline
            .map(str::to_owned)
            .or_else(|| {
                flags
                    .get(index)
                    .filter(|value| !value.starts_with("--"))
                    .cloned()
            })
            .ok_or(CliError::InvalidOperationsCommand)?;
        if slot.is_some() || value.is_empty() || value.chars().any(char::is_control) {
            return Err(CliError::InvalidOperationsCommand);
        }
        *slot = Some(value);
        index += 1;
    }
    Ok(Command::Operations { options, json })
}
