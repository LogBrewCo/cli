//! CLI flag recovery tests.

use super::{TestResult, assert_json_error};

#[test]
fn read_unknown_flags_point_to_resource_help() -> TestResult {
    for (args, next) in [
        (
            &["logbrew", "logs", "--sort", "desc", "--json"][..],
            "run logbrew read logs --help",
        ),
        (
            &["logbrew", "issues", "--sort", "desc", "--json"][..],
            "run logbrew read issues --help",
        ),
        (
            &["logbrew", "actions", "--sort", "desc", "--json"][..],
            "run logbrew read actions --help",
        ),
        (
            &["logbrew", "releases", "--sort", "desc", "--json"][..],
            "run logbrew read releases --help",
        ),
        (
            &["logbrew", "trace", "trace_123", "--sort", "desc", "--json"][..],
            "run logbrew read trace --help",
        ),
    ] {
        assert_json_error(args, "unknown_flag", "unknown flag: --sort", next)?;
    }
    Ok(())
}

#[test]
fn command_unknown_flags_point_to_command_help() -> TestResult {
    for (args, next) in [
        (
            &["logbrew", "login", "--bogus", "--json"][..],
            "run logbrew login --help",
        ),
        (
            &["logbrew", "status", "--bogus", "--json"][..],
            "run logbrew status --help",
        ),
        (
            &["logbrew", "setup", "--bogus", "--json"][..],
            "run logbrew setup --help",
        ),
        (
            &["logbrew", "watch", "logs", "--bogus", "--json"][..],
            "run logbrew watch --help",
        ),
    ] {
        assert_json_error(args, "unknown_flag", "unknown flag: --bogus", next)?;
    }
    Ok(())
}

#[test]
fn issue_status_shortcut_flags_point_to_shortcut_help() -> TestResult {
    for (args, error, message, next) in [
        (
            &["logbrew", "resolve", "issue_123", "--bogus", "--json"][..],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew resolve --help",
        ),
        (
            &["logbrew", "close", "issue_123", "--bogus", "--json"],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew close --help",
        ),
        (
            &["logbrew", "ignore", "issue_123", "--auto", "--json"][..],
            "unsupported_flag",
            "unsupported flag for ignore: --auto",
            "run logbrew ignore --help",
        ),
        (
            &[
                "logbrew",
                "reopen",
                "issue_123",
                "--release",
                "api@1",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for reopen: --release",
            "run logbrew reopen --help",
        ),
        (
            &["logbrew", "closed", "issue_123", "--bogus", "--json"][..],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew closed --help",
        ),
        (
            &[
                "logbrew",
                "open",
                "issue_123",
                "--release",
                "api@1",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for open: --release",
            "run logbrew open --help",
        ),
    ] {
        assert_json_error(args, error, message, next)?;
    }
    Ok(())
}

#[test]
fn bare_issue_status_flags_point_to_shortcut_help() -> TestResult {
    for (args, error, message, next) in [
        (
            &["logbrew", "open", "--bogus", "--json"][..],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew open --help",
        ),
        (
            &["logbrew", "closed", "--release", "api@1", "--json"][..],
            "unsupported_flag",
            "unsupported flag for closed: --release",
            "run logbrew closed --help",
        ),
    ] {
        assert_json_error(args, error, message, next)?;
    }
    Ok(())
}

#[test]
fn auth_namespace_help_rejects_duplicate_json() -> TestResult {
    for args in [
        &["logbrew", "auth", "--json", "--json"][..],
        &["logbrew", "auth", "token", "--json", "--json"][..],
    ] {
        assert_json_error(
            args,
            "duplicate_flag",
            "duplicate flag: --json",
            "use --json once",
        )?;
    }
    Ok(())
}
