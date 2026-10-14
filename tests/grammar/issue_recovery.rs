//! CLI issue shortcut recovery tests.

use super::{TestResult, assert_json_error};

#[test]
fn rejects_bare_issue_status_words_with_target_next_step() -> TestResult {
    for (args, next) in [
        (
            &["logbrew", "open", "--json"][..],
            "use logbrew open <issue_id> or logbrew open issues",
        ),
        (
            &["logbrew", "closed", "--json"][..],
            "use logbrew closed <issue_id> or logbrew closed issues",
        ),
        (
            &["logbrew", "ignored", "--json"][..],
            "use logbrew ignored <issue_id> or logbrew ignored issues",
        ),
    ] {
        assert_json_error(
            args,
            "missing_argument",
            "missing argument: issue_id_or_issues",
            next,
        )?;
    }
    Ok(())
}

#[test]
fn rejects_issue_mutation_shortcuts_without_ids_with_command_next_step() -> TestResult {
    for (args, next) in [
        (
            &["logbrew", "resolve", "--json"][..],
            "use logbrew resolve <issue_id>",
        ),
        (
            &["logbrew", "close", "--json"][..],
            "use logbrew close <issue_id>",
        ),
        (
            &["logbrew", "ignore", "--json"][..],
            "use logbrew ignore <issue_id>",
        ),
        (
            &["logbrew", "reopen", "--json"][..],
            "use logbrew reopen <issue_id>",
        ),
    ] {
        assert_json_error(args, "missing_argument", "missing argument: issue_id", next)?;
    }
    Ok(())
}

#[test]
fn issue_first_mutation_flags_preserve_shortcut_recovery() -> TestResult {
    for (args, error, message, next) in [
        (
            &[
                "logbrew",
                "issue",
                "issue_123",
                "resolve",
                "--release",
                "api@1",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for resolve: --release",
            "run logbrew resolve --help",
        ),
        (
            &[
                "logbrew",
                "issue",
                "issue_123",
                "ignored",
                "--auto",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for ignored: --auto",
            "run logbrew ignored --help",
        ),
    ] {
        assert_json_error(args, error, message, next)?;
    }
    Ok(())
}
