//! Public CLI grammar and local recovery contracts.

use crate::{TestResult, assert_command, assert_help, render_error, rendered_json_error};
use logbrew_cli::parse_command;

mod action_filter_recovery;
mod command_shaped_help;
mod commands;
mod flag_recovery;
mod help_errors;
mod help_text;
mod issue_mutation_shortcuts;
mod issue_recovery;
mod local_commands;
mod parse_errors;
mod search_commands;
mod setup_readiness;
mod shortcut_commands;
mod watch_errors;

fn assert_path(args: &[&str], expected: &str) {
    assert_eq!(
        parse_command(args.iter().copied()).map(|command| command.http_path()),
        Ok(Some(expected.to_owned())),
        "unexpected endpoint for {args:?}"
    );
}

fn assert_json_error(args: &[&str], code: &str, message: &str, next: &str) -> TestResult {
    let (body, _) = rendered_json_error(args)?;
    assert_eq!(body["ok"], false, "{args:?}");
    assert_eq!(body["error"], code, "{args:?}");
    assert_eq!(body["message"], message, "{args:?}");
    assert_eq!(body["next"], next, "{args:?}");
    Ok(())
}

fn assert_human_error(args: &[&str], message: &str, next: &str) -> TestResult {
    assert_eq!(
        render_error(args, false)?,
        format!("{message}\nNext: {next}\n"),
        "{args:?}"
    );
    Ok(())
}
