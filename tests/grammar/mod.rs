//! Public CLI grammar and local recovery contracts.

#[macro_use]
#[path = "../async_test.rs"]
mod async_test;
pub(crate) use async_test::run_async;

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
