//! Live watch command parsing and recovery hints.

use super::{WATCH_RESOURCE_NEXT_STEP, unknown_resource};
use crate::flags::normalize_log_level;
use crate::{CliError, Command, WatchOptions, WatchTarget};

/// Historical log recovery for unsupported watch filters.
const WATCH_LOG_FILTER_NEXT_STEP: &str = "use logbrew logs with filters for historical data, or logbrew watch logs --severity <severity> --json for live severity filtering";
/// Historical log recovery for unsupported watch positionals.
const WATCH_LOG_POSITIONAL_NEXT_STEP: &str = "use logbrew logs --severity <severity> or --search \
                                              <text> for historical data, or logbrew watch logs --json for live logs";
/// Historical issue recovery for unsupported watch filters.
const WATCH_ISSUE_FILTER_NEXT_STEP: &str = "use logbrew issues with filters for historical data, or logbrew watch issues --severity <severity> --json for live severity filtering";
/// Historical issue recovery for unsupported watch positionals.
const WATCH_ISSUE_POSITIONAL_NEXT_STEP: &str = "use logbrew issues with filters for historical data, or logbrew watch issues --json for live issues";
/// Historical action recovery for unsupported watch filters.
const WATCH_ACTION_FILTER_NEXT_STEP: &str = "use logbrew actions with filters for historical data, or logbrew watch actions --json for live actions";
/// Historical action recovery for unsupported watch positionals.
const WATCH_ACTION_POSITIONAL_NEXT_STEP: &str = "use logbrew actions --name <name> for historical data, or logbrew watch actions --json for live actions";
/// Trace/span recovery for unsupported live watch resources.
const WATCH_TRACE_RESOURCE_NEXT_STEP: &str =
    "watch streams logs, issues, and actions; use logbrew trace <trace_id> to read a trace";

/// Parses `watch`.
pub(super) fn parse_watch(args: &[String]) -> Result<Command, CliError> {
    let (target, rest) = watch_target_and_tail(args)?;
    let (json, options) = parse_watch_flags(target, rest)?;
    Ok(Command::Watch {
        target,
        options,
        json,
    })
}

/// Resolves an optional watch resource and leaves flags for later parsing.
fn watch_target_and_tail(args: &[String]) -> Result<(WatchTarget, &[String]), CliError> {
    let Some((first, tail)) = args.split_first() else {
        return Ok((WatchTarget::All, args));
    };
    if first.starts_with('-') {
        return Ok((WatchTarget::All, args));
    }
    let target = match first.as_str() {
        "all" | "events" | "event" => WatchTarget::All,
        "logs" | "log" => WatchTarget::Logs,
        "issues" | "issue" => WatchTarget::Issues,
        "actions" | "action" => WatchTarget::Actions,
        other => return Err(unknown_resource(other, watch_resource_next_step(other))),
    };
    Ok((target, tail))
}

/// Returns a recovery hint for unsupported watch resources.
fn watch_resource_next_step(resource: &str) -> &'static str {
    match resource {
        "trace" | "traces" | "span" | "spans" => WATCH_TRACE_RESOURCE_NEXT_STEP,
        _ => WATCH_RESOURCE_NEXT_STEP,
    }
}

/// Parses watch flags and target-specific client-side filters.
fn parse_watch_flags(
    target: WatchTarget,
    args: &[String],
) -> Result<(bool, WatchOptions), CliError> {
    let mut json = false;
    let mut seen_severity = false;
    let mut options = WatchOptions::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--json" {
            if json {
                return Err(CliError::DuplicateFlag {
                    flag: "--json",
                    next: "use --json once",
                });
            }
            json = true;
            index += 1;
            continue;
        }

        let (flag, inline_value) = split_inline_value(arg);
        if matches!(flag, "--severity" | "--level") {
            if !target_allows_severity(target) {
                return Err(CliError::UnsupportedFlag {
                    flag: flag.to_owned(),
                    command: "watch",
                    next: filter_next_step(target),
                });
            }
            if seen_severity {
                return Err(CliError::DuplicateFlag {
                    flag: "--severity",
                    next: "use --severity once",
                });
            }
            seen_severity = true;
            let value = take_watch_flag_value(args, &mut index, "--severity", inline_value)?;
            options.severity = normalize_watch_severities(value.as_str())?;
            index += 1;
            continue;
        }

        if let Some(flag) = historical_filter_flag(target, arg) {
            return Err(CliError::UnsupportedFlag {
                flag: flag.to_owned(),
                command: "watch",
                next: filter_next_step(target),
            });
        }
        if let Some(flag) = known_unsupported_watch_flag(arg) {
            return Err(CliError::UnsupportedFlag {
                flag: flag.to_owned(),
                command: "watch",
                next: "run logbrew watch --help",
            });
        }
        if arg.starts_with('-') {
            return Err(CliError::UnknownFlag {
                flag: arg.to_owned(),
                next: "run logbrew watch --help",
            });
        }
        return Err(CliError::UnexpectedArgument {
            argument: arg.clone(),
            command: "watch",
            next: positional_next_step(target),
        });
    }
    Ok((json, options))
}

/// Splits `--flag=value` while leaving ordinary flags untouched.
fn split_inline_value(flag: &str) -> (&str, Option<&str>) {
    flag.split_once('=')
        .map_or((flag, None), |(name, value)| (name, Some(value)))
}

/// Reads a watch flag value from either inline or following argument form.
fn take_watch_flag_value(
    args: &[String],
    index: &mut usize,
    flag: &'static str,
    inline_value: Option<&str>,
) -> Result<String, CliError> {
    if let Some(value) = inline_value {
        if value.is_empty() {
            return Err(missing_watch_flag_value(flag));
        }
        return Ok(value.to_owned());
    }
    *index += 1;
    let value = args
        .get(*index)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| missing_watch_flag_value(flag))?;
    Ok(value.clone())
}

/// Builds a watch missing-value error.
const fn missing_watch_flag_value(flag: &'static str) -> CliError {
    CliError::MissingFlagValue {
        flag,
        next: "provide one of info, warning, error, critical",
    }
}

/// Normalizes a comma-separated watch severity list.
fn normalize_watch_severities(value: &str) -> Result<Vec<String>, CliError> {
    let mut severities = Vec::new();
    for part in value.split(',') {
        let severity = normalize_log_level(part.trim())?;
        if !severities.contains(&severity) {
            severities.push(severity);
        }
    }
    if severities.is_empty() {
        return Err(CliError::MissingFlagValue {
            flag: "--severity",
            next: "provide one of info, warning, error, critical",
        });
    }
    Ok(severities)
}

/// Returns whether a target can apply severity filters.
const fn target_allows_severity(target: WatchTarget) -> bool {
    matches!(
        target,
        WatchTarget::All | WatchTarget::Logs | WatchTarget::Issues
    )
}

/// Returns known cross-command flags that watch does not accept.
fn known_unsupported_watch_flag(arg: &str) -> Option<&str> {
    let flag = arg.split_once('=').map_or(arg, |(name, _)| name);
    match flag {
        "--auto" | "--yes" | "--no-open" | "--status" => Some(flag),
        _ => None,
    }
}

/// Returns read-filter flags that map cleanly to a historical read fallback for watch.
fn historical_filter_flag(target: WatchTarget, arg: &str) -> Option<&str> {
    let flag = arg.split_once('=').map_or(arg, |(name, _)| name);
    match target {
        WatchTarget::All => match flag {
            "--search" | "--trace" | "--trace-id" | "--project" | "--project-id" | "--release"
            | "--environment" | "--env" | "--since" | "--limit" | "--name" | "--user"
            | "--distinct-id" | "--status" => Some(flag),
            _ => None,
        },
        WatchTarget::Logs => match flag {
            "--search" | "--trace" | "--trace-id" | "--project" | "--project-id" | "--release"
            | "--environment" | "--env" | "--since" | "--limit" => Some(flag),
            _ => None,
        },
        WatchTarget::Issues => match flag {
            "--project" | "--project-id" | "--release" | "--environment" | "--env" | "--status"
            | "--limit" => Some(flag),
            _ => None,
        },
        WatchTarget::Actions => match flag {
            "--name" | "--user" | "--distinct-id" | "--project" | "--project-id" | "--release"
            | "--environment" | "--env" | "--since" | "--limit" | "--severity" | "--level" => {
                Some(flag)
            }
            _ => None,
        },
    }
}

/// Returns the target-specific read fallback for unsupported watch filters.
const fn filter_next_step(target: WatchTarget) -> &'static str {
    match target {
        WatchTarget::All => {
            "use REST read commands for filtered historical data, or logbrew watch --severity error,critical --json for live severity filtering"
        }
        WatchTarget::Logs => WATCH_LOG_FILTER_NEXT_STEP,
        WatchTarget::Issues => WATCH_ISSUE_FILTER_NEXT_STEP,
        WatchTarget::Actions => WATCH_ACTION_FILTER_NEXT_STEP,
    }
}

/// Returns the target-specific read fallback for unsupported watch positionals.
const fn positional_next_step(target: WatchTarget) -> &'static str {
    match target {
        WatchTarget::All => {
            "use logbrew watch --json for all live events, or choose logs, issues, or actions"
        }
        WatchTarget::Logs => WATCH_LOG_POSITIONAL_NEXT_STEP,
        WatchTarget::Issues => WATCH_ISSUE_POSITIONAL_NEXT_STEP,
        WatchTarget::Actions => WATCH_ACTION_POSITIONAL_NEXT_STEP,
    }
}
