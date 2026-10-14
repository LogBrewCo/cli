//! CLI search shortcut tests.

use super::{assert_command, assert_help};
use logbrew_cli::{Command, HelpTopic, ReadOptions, ReadTarget, parse_command};

#[test]
fn parses_search_as_log_search_shortcut() {
    for args in [
        &[
            "logbrew",
            "search",
            "checkout",
            "failed",
            "--release",
            "api@1",
            "--environment",
            "production",
            "--level",
            "error",
            "--json",
        ][..],
        &[
            "logbrew",
            "search",
            "checkout failed",
            "--release",
            "api@1",
            "--environment",
            "production",
            "--level",
            "error",
            "--json",
        ],
        &[
            "logbrew",
            "--json",
            "search",
            "checkout failed",
            "--release",
            "api@1",
            "--environment",
            "production",
            "--level",
            "error",
        ],
        &[
            "logbrew",
            "search",
            "--json",
            "checkout failed",
            "--release",
            "api@1",
            "--environment",
            "production",
            "--level",
            "error",
        ],
    ] {
        assert_filtered_search(args);
    }
}

#[test]
fn parses_search_separator_as_literal_log_search_shortcut() {
    for (args, search, json) in [
        (
            &["logbrew", "--json", "search", "--", "--timeout"][..],
            "--timeout",
            true,
        ),
        (
            &["logbrew", "search", "--", "--timeout", "failed", "--json"][..],
            "--timeout failed",
            true,
        ),
        (&["logbrew", "search", "--", "--json"][..], "--json", false),
        (
            &["logbrew", "--json", "search", "--", "--json"][..],
            "--json",
            true,
        ),
        (
            &["logbrew", "search", "--", "--json", "--json"][..],
            "--json",
            true,
        ),
        (&["logbrew", "search", "--", "--help"][..], "--help", false),
        (
            &["logbrew", "--json", "search", "--", "--help"][..],
            "--help",
            true,
        ),
        (
            &["logbrew", "search", "--", "--help", "--json"][..],
            "--help",
            true,
        ),
    ] {
        assert_command(
            args,
            Command::Read {
                target: ReadTarget::Logs,
                options: Box::new(ReadOptions {
                    search: Some(search.to_owned()),
                    ..ReadOptions::default()
                }),
                json,
            },
        );
    }
}

#[test]
fn parses_logs_separator_as_literal_log_search_shortcut() -> Result<(), Box<dyn std::error::Error>>
{
    for (args, expected_path) in [
        (
            &["logbrew", "logs", "--", "--timeout", "--json"][..],
            "/api/logs?search=--timeout",
        ),
        (
            &["logbrew", "logs", "--", "--help", "--json"][..],
            "/api/logs?search=--help",
        ),
        (
            &["logbrew", "--json", "logs", "--", "--json"][..],
            "/api/logs?search=--json",
        ),
        (
            &["logbrew", "read", "logs", "--", "--help", "--json"][..],
            "/api/logs?search=--help",
        ),
        (
            &["logbrew", "last", "5", "logs", "--", "--help", "--json"][..],
            "/api/logs?search=--help&limit=5",
        ),
        (
            &[
                "logbrew", "read", "recent", "5", "logs", "--", "--help", "--json",
            ][..],
            "/api/logs?search=--help&limit=5",
        ),
        (
            &["logbrew", "logs", "--search", "--", "--timeout", "--json"][..],
            "/api/logs?search=--timeout",
        ),
        (
            &["logbrew", "logs", "--search", "--", "--help", "--json"][..],
            "/api/logs?search=--help",
        ),
        (
            &["logbrew", "--json", "logs", "--search", "--", "--json"][..],
            "/api/logs?search=--json",
        ),
        (
            &["logbrew", "logs", "--", "--json", "--json"][..],
            "/api/logs?search=--json",
        ),
    ] {
        assert_json_log_path(args, expected_path)?;
    }
    Ok(())
}

#[test]
fn parses_filtered_logs_separator_without_losing_filters() -> Result<(), Box<dyn std::error::Error>>
{
    for (args, expected_path) in [
        (
            &[
                "logbrew",
                "logs",
                "--level",
                "error",
                "--",
                "--timeout",
                "--json",
            ][..],
            "/api/logs?severity=error&search=--timeout",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--level",
                "error",
                "--search",
                "--",
                "--timeout",
                "--json",
            ][..],
            "/api/logs?severity=error&search=--timeout",
        ),
        (
            &["logbrew", "logs", "--search=--timeout", "failed", "--json"][..],
            "/api/logs?search=--timeout%20failed",
        ),
        (
            &[
                "logbrew",
                "--json",
                "logs",
                "--level",
                "error",
                "--search=--timeout",
                "failed",
            ][..],
            "/api/logs?severity=error&search=--timeout%20failed",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--release",
                "filtered@1",
                "--",
                "--timeout",
                "failed",
                "--json",
            ],
            "/api/logs?search=--timeout%20failed&release=filtered%401",
        ),
    ] {
        assert_json_log_path(args, expected_path)?;
    }
    Ok(())
}

#[test]
fn parses_find_and_grep_as_log_search_shortcuts() {
    for args in [
        &[
            "logbrew",
            "find",
            "checkout failed",
            "--release",
            "api@1",
            "--environment",
            "production",
            "--level",
            "error",
            "--json",
        ][..],
        &[
            "logbrew",
            "--json",
            "grep",
            "checkout failed",
            "--release",
            "api@1",
            "--environment",
            "production",
            "--level",
            "error",
        ],
    ] {
        assert_filtered_search(args);
    }
}

#[test]
fn parses_search_help_as_logs_help() {
    for args in [
        &["logbrew", "search", "--help"][..],
        &["logbrew", "search", "checkout", "--help"],
        &["logbrew", "search", "help"],
        &["logbrew", "search", "help", "checkout"],
        &["logbrew", "search", "checkout", "help"],
        &["logbrew", "help", "search"],
        &["logbrew", "help", "search", "checkout"],
        &["logbrew", "find", "--help"],
        &["logbrew", "find", "checkout", "--help"],
        &["logbrew", "grep", "help"],
        &["logbrew", "grep", "help", "timeout"],
        &["logbrew", "help", "find"],
        &["logbrew", "help", "find", "checkout"],
        &["logbrew", "help", "grep"],
        &["logbrew", "help", "grep", "timeout"],
    ] {
        assert_help(args, HelpTopic::ReadLogs, false);
    }

    assert_help(
        &["logbrew", "help", "search", "checkout", "--json"],
        HelpTopic::ReadLogs,
        true,
    );
    assert!(
        parse_command(["logbrew", "help", "search", "checkout", "extra"]).is_err(),
        "extra search help terms remain errors"
    );
}

fn assert_filtered_search(args: &[&str]) {
    assert_command(
        args,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                level: Some("error".to_owned()),
                search: Some("checkout failed".to_owned()),
                release: Some("api@1".to_owned()),
                environment: Some("production".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        },
    );
}

fn assert_json_log_path(
    args: &[&str],
    expected_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let command = parse_command(args.iter().copied())?;
    assert_eq!(
        command.http_path().as_deref(),
        Some(expected_path),
        "unexpected log-search endpoint for {args:?}"
    );
    assert!(
        matches!(command, Command::Read { json: true, .. }),
        "log-search command must preserve JSON output for {args:?}"
    );
    Ok(())
}
