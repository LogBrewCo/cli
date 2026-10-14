//! CLI help error recovery tests.

use super::{TestResult, assert_human_error, assert_json_error};

#[test]
fn rejects_unknown_flag_in_explicit_help() -> TestResult {
    assert_json_error(
        &["logbrew", "help", "logs", "--bogus", "--json"],
        "unknown_flag",
        "unknown flag: --bogus",
        "run logbrew --help",
    )
}

#[test]
fn rejects_known_filter_flag_in_help_shortcut() -> TestResult {
    assert_human_error(
        &["logbrew", "logs", "--help", "--release", "api@1"],
        "unknown flag: --release",
        "run logbrew --help",
    )
}

#[test]
fn rejects_extra_argument_after_root_help() -> TestResult {
    assert_json_error(
        &["logbrew", "--help", "logs", "--json"],
        "unexpected_argument",
        "unexpected argument for help: logs",
        "run logbrew --help",
    )
}

#[test]
fn rejects_extra_argument_in_help_shortcut() -> TestResult {
    assert_human_error(
        &["logbrew", "logs", "--help", "checkout@1"],
        "unexpected argument for help: checkout@1",
        "run logbrew --help",
    )
}

#[test]
fn rejects_extra_argument_in_read_help_topic() -> TestResult {
    assert_json_error(
        &["logbrew", "help", "read", "logs", "extra", "--json"],
        "unexpected_argument",
        "unexpected argument for help: extra",
        "run logbrew --help",
    )
}

#[test]
fn rejects_extra_argument_in_subcommand_resource_help_topic() -> TestResult {
    assert_human_error(
        &["logbrew", "help", "explain", "trace", "trace_123", "extra"],
        "unexpected argument for help: extra",
        "run logbrew --help",
    )
}

#[test]
fn rejects_actual_extra_argument_after_copied_detail_help_ids() -> TestResult {
    for args in [
        &["logbrew", "help", "trace", "trace_123", "extra"][..],
        &["logbrew", "help", "trace", "trace_123", "explain", "extra"],
        &["logbrew", "help", "issue", "issue_123", "extra"],
        &["logbrew", "help", "issue", "issue_123", "explain", "extra"],
        &["logbrew", "help", "read", "trace", "trace_123", "extra"],
        &["logbrew", "help", "show", "errors", "issue_123", "extra"],
    ] {
        assert_human_error(
            args,
            "unexpected argument for help: extra",
            "run logbrew --help",
        )?;
    }
    Ok(())
}

#[test]
fn rejects_actual_extra_argument_after_copied_explain_help_ids() -> TestResult {
    for args in [
        &["logbrew", "help", "issue_123", "explain", "extra"][..],
        &["logbrew", "help", "trace_123", "explain", "extra"],
        &["logbrew", "help", "explain", "issue_123", "extra"],
        &[
            "logbrew",
            "help",
            "explain",
            "4bf92f3577b34da6a3ce929d0e0e4736",
            "extra",
        ],
    ] {
        assert_human_error(
            args,
            "unexpected argument for help: extra",
            "run logbrew --help",
        )?;
    }
    Ok(())
}
