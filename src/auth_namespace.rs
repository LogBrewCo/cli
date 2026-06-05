//! Auth namespace command parsing.

use crate::flags::{FlagScope, parse_flags};
use crate::{CliError, Command, HelpTopic};

/// Standard next step for malformed auth namespace invocations.
const AUTH_NEXT_STEP: &str = "run logbrew auth --help";

/// Parses the `auth` namespace as thin aliases over token-safe local commands.
pub fn parse(args: &[String]) -> Result<Command, CliError> {
    if positional_args(args).is_empty() {
        return parse_auth_help_alias(args);
    }

    let normalized = move_leading_json_to_tail(args);
    let Some((head, tail)) = normalized.split_first() else {
        return Ok(help_command(HelpTopic::Auth, args));
    };

    match head.as_str() {
        "help" => parse_help_command(tail),
        "login" => parse_login(tail),
        "logout" => parse_logout(tail),
        alias if is_status_alias(alias) || matches!(alias, "whoami" | "me") => parse_status(tail),
        alias if is_help_alias(alias) => parse_auth_help_alias(tail),
        other => Err(CliError::UnexpectedArgument {
            argument: other.to_owned(),
            command: "auth",
            next: AUTH_NEXT_STEP,
        }),
    }
}

/// Parses `logbrew auth help [topic]`.
fn parse_help_command(args: &[String]) -> Result<Command, CliError> {
    validate_flags(args)?;
    let positionals = positional_args(args);
    let topic = help_topic(positionals.as_slice())?;
    Ok(help_command(topic, args))
}

/// Resolves help for auth namespace subcommands.
pub fn help_topic(args: &[&str]) -> Result<HelpTopic, CliError> {
    let Some((head, tail)) = args.split_first() else {
        return Ok(HelpTopic::Auth);
    };
    let topic = match *head {
        "help" => HelpTopic::Auth,
        "login" => HelpTopic::Login,
        "logout" => HelpTopic::Logout,
        alias if is_status_alias(alias) => HelpTopic::Status,
        "whoami" | "me" => HelpTopic::Status,
        alias if is_help_alias(alias) => HelpTopic::Auth,
        other => {
            return Err(CliError::UnknownResource {
                resource: other.to_owned(),
                next: AUTH_NEXT_STEP,
            });
        }
    };
    ensure_no_positionals(tail)?;
    Ok(topic)
}

/// Returns whether a word should behave as the auth command namespace.
#[must_use]
pub fn is_namespace(value: &str) -> bool {
    matches!(value, "auth" | "authentication")
}

/// Returns whether a word should land on auth help.
#[must_use]
pub fn is_help_alias(value: &str) -> bool {
    matches!(
        value,
        "auth"
            | "authentication"
            | "token"
            | "tokens"
            | "credential"
            | "credentials"
            | "account"
            | "accounts"
            | "profile"
            | "profiles"
            | "identity"
            | "identities"
            | "user"
            | "users"
    )
}

/// Parses auth help with auth-specific recovery for bad namespace flags.
fn parse_auth_help_alias(args: &[String]) -> Result<Command, CliError> {
    validate_flags(args)?;
    ensure_no_positionals(positional_args(args).as_slice())?;
    Ok(help_command(HelpTopic::Auth, args))
}

/// Parses namespaced login.
fn parse_login(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Login)?;
    let json = flags.is_json();
    Ok(Command::Login {
        open_browser: flags.should_open_browser() && !json,
        json,
    })
}

/// Parses namespaced logout.
fn parse_logout(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Logout)?;
    Ok(Command::Logout {
        json: flags.is_json(),
    })
}

/// Parses namespaced status.
fn parse_status(args: &[String]) -> Result<Command, CliError> {
    let flags = parse_flags(args, FlagScope::Status)?;
    Ok(Command::Status {
        json: flags.is_json(),
    })
}

/// Moves a leading JSON flag behind the namespace subcommand.
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

/// Builds a help command.
fn help_command(topic: HelpTopic, args: &[String]) -> Command {
    Command::Help {
        topic,
        json: args.iter().any(|arg| arg == "--json"),
    }
}

/// Rejects non-help flags in auth namespace help.
fn validate_flags(args: &[String]) -> Result<(), CliError> {
    let mut seen_json = false;
    for arg in args {
        if arg == "--json" && std::mem::replace(&mut seen_json, true) {
            return Err(CliError::DuplicateFlag {
                flag: "--json",
                next: "use --json once",
            });
        }
        if arg.starts_with('-') && !matches!(arg.as_str(), "--help" | "-h" | "--json") {
            return Err(CliError::UnknownFlag {
                flag: arg.to_owned(),
                next: AUTH_NEXT_STEP,
            });
        }
    }
    Ok(())
}

/// Returns all non-flag arguments.
fn positional_args(args: &[String]) -> Vec<&str> {
    args.iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect()
}

/// Rejects positional arguments that would otherwise be ignored.
fn ensure_no_positionals(args: &[&str]) -> Result<(), CliError> {
    if let Some(argument) = args.first() {
        return Err(CliError::UnexpectedArgument {
            argument: (*argument).to_owned(),
            command: "auth",
            next: AUTH_NEXT_STEP,
        });
    }
    Ok(())
}

/// Returns whether a word is an auth-check alias.
fn is_status_alias(value: &str) -> bool {
    matches!(value, "status" | "health" | "ping" | "doctor")
}
