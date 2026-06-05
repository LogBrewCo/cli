//! Reserved watch command parsing and recovery hints.

use super::{WATCH_RESOURCE_NEXT_STEP, take_required_position, unknown_resource};
use crate::flags::{FlagScope, parse_flags};
use crate::{CliError, Command, WatchTarget};

/// Historical log recovery for reserved watch filters.
const WATCH_LOG_FILTER_NEXT_STEP: &str =
    "use logbrew logs with filters for historical data until live watch is available";
/// Historical log recovery for reserved watch positionals.
const WATCH_LOG_POSITIONAL_NEXT_STEP: &str = "use logbrew logs --level <level> or --search <text> \
                                              for historical data until live watch is available";
/// Historical action recovery for reserved watch filters.
const WATCH_ACTION_FILTER_NEXT_STEP: &str =
    "use logbrew actions with filters for historical data until live watch is available";
/// Historical action recovery for reserved watch positionals.
const WATCH_ACTION_POSITIONAL_NEXT_STEP: &str =
    "use logbrew actions --name <name> for historical data until live watch is available";

/// Parses `watch`.
pub(super) fn parse_watch(args: &[String]) -> Result<Command, CliError> {
    let (resource, rest) = take_required_position(args, "resource", WATCH_RESOURCE_NEXT_STEP)?;
    let target = match resource.as_str() {
        "logs" => WatchTarget::Logs,
        "actions" | "action" | "events" | "event" => WatchTarget::Actions,
        other => return Err(unknown_resource(other, WATCH_RESOURCE_NEXT_STEP)),
    };
    if let Some(error) = reserved_watch_tail_error(target, rest.as_slice())? {
        return Err(error);
    }
    let flags = parse_flags(rest.as_slice(), FlagScope::Watch)?;
    Ok(Command::Watch {
        target,
        json: flags.is_json(),
    })
}

/// Returns target-specific recovery for filters/positionals on the reserved watch flow.
fn reserved_watch_tail_error(
    target: WatchTarget,
    args: &[String],
) -> Result<Option<CliError>, CliError> {
    reject_duplicate_json(args)?;
    for arg in args {
        if arg == "--json" {
            continue;
        }
        if let Some(flag) = historical_filter_flag(target, arg) {
            return Ok(Some(CliError::UnsupportedFlag {
                flag: flag.to_owned(),
                command: "watch",
                next: filter_next_step(target),
            }));
        }
        if arg.starts_with('-') {
            return Ok(None);
        }
        return Ok(Some(CliError::UnexpectedArgument {
            argument: arg.to_owned(),
            command: "watch",
            next: positional_next_step(target),
        }));
    }
    Ok(None)
}

/// Preserves duplicate JSON recovery before watch fallback hints inspect positionals.
fn reject_duplicate_json(args: &[String]) -> Result<(), CliError> {
    let mut seen_json = false;
    for arg in args {
        if arg != "--json" {
            continue;
        }
        if seen_json {
            return Err(CliError::DuplicateFlag {
                flag: "--json",
                next: "use --json once",
            });
        }
        seen_json = true;
    }
    Ok(())
}

/// Returns read-filter flags that map cleanly to a historical read fallback for watch.
fn historical_filter_flag(target: WatchTarget, arg: &str) -> Option<&str> {
    let flag = arg.split_once('=').map_or(arg, |(name, _)| name);
    match target {
        WatchTarget::Logs => match flag {
            "--level" | "--search" | "--trace" | "--trace-id" | "--project" | "--project-id"
            | "--release" | "--environment" | "--env" | "--since" | "--limit" => Some(flag),
            _ => None,
        },
        WatchTarget::Actions => match flag {
            "--name" | "--user" | "--distinct-id" | "--project" | "--project-id" | "--release"
            | "--environment" | "--env" | "--since" | "--limit" => Some(flag),
            _ => None,
        },
    }
}

/// Returns the target-specific read fallback for unsupported watch filters.
const fn filter_next_step(target: WatchTarget) -> &'static str {
    match target {
        WatchTarget::Logs => WATCH_LOG_FILTER_NEXT_STEP,
        WatchTarget::Actions => WATCH_ACTION_FILTER_NEXT_STEP,
    }
}

/// Returns the target-specific read fallback for unsupported watch positionals.
const fn positional_next_step(target: WatchTarget) -> &'static str {
    match target {
        WatchTarget::Logs => WATCH_LOG_POSITIONAL_NEXT_STEP,
        WatchTarget::Actions => WATCH_ACTION_POSITIONAL_NEXT_STEP,
    }
}
