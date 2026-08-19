//! CLI search shortcut tests.

use logbrew_cli::{Command, HelpTopic, ReadOptions, ReadTarget, parse_command};

#[test]
fn parses_search_as_log_search_shortcut() {
    for (args, search, level, release, environment) in [
        (
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
            "checkout failed",
            Some("error"),
            Some("api@1"),
            Some("production"),
        ),
        (
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
            ][..],
            "checkout failed",
            Some("error"),
            Some("api@1"),
            Some("production"),
        ),
        (
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
            ][..],
            "checkout failed",
            Some("error"),
            Some("api@1"),
            Some("production"),
        ),
        (
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
            ][..],
            "checkout failed",
            Some("error"),
            Some("api@1"),
            Some("production"),
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("search shortcut parses");

        assert_eq!(
            command,
            Command::Read {
                target: ReadTarget::Logs,
                options: Box::new(ReadOptions {
                    name: None,
                    service: None,
                    since: None,
                    user: None,
                    trace: None,
                    level: level.map(str::to_owned),
                    search: Some(search.to_owned()),
                    project: None,
                    release: release.map(str::to_owned),
                    environment: environment.map(str::to_owned),
                    status: None,
                    limit: None,
                    min_duration_ms: None,
                    pagination: None,
                    cursor_time: None,
                    cursor_id: None,
                }),
                json: true,
            }
        );
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
        let command = parse_command(args.iter().copied()).expect("search separator parses");

        assert_eq!(
            command,
            Command::Read {
                target: ReadTarget::Logs,
                options: Box::new(ReadOptions {
                    name: None,
                    service: None,
                    since: None,
                    user: None,
                    trace: None,
                    level: None,
                    search: Some(search.to_owned()),
                    project: None,
                    release: None,
                    environment: None,
                    status: None,
                    limit: None,
                    min_duration_ms: None,
                    pagination: None,
                    cursor_time: None,
                    cursor_id: None,
                }),
                json,
            }
        );
    }
}

#[test]
fn parses_logs_separator_as_literal_log_search_shortcut() {
    for (args, expected_path, json) in [
        (
            &["logbrew", "logs", "--", "--timeout", "--json"][..],
            "/api/logs?search=--timeout",
            true,
        ),
        (
            &["logbrew", "logs", "--", "--help", "--json"][..],
            "/api/logs?search=--help",
            true,
        ),
        (
            &["logbrew", "--json", "logs", "--", "--json"][..],
            "/api/logs?search=--json",
            true,
        ),
        (
            &["logbrew", "read", "logs", "--", "--help", "--json"][..],
            "/api/logs?search=--help",
            true,
        ),
        (
            &["logbrew", "last", "5", "logs", "--", "--help", "--json"][..],
            "/api/logs?search=--help&limit=5",
            true,
        ),
        (
            &[
                "logbrew", "read", "recent", "5", "logs", "--", "--help", "--json",
            ][..],
            "/api/logs?search=--help&limit=5",
            true,
        ),
        (
            &["logbrew", "logs", "--search", "--", "--timeout", "--json"][..],
            "/api/logs?search=--timeout",
            true,
        ),
        (
            &["logbrew", "logs", "--search", "--", "--help", "--json"][..],
            "/api/logs?search=--help",
            true,
        ),
        (
            &["logbrew", "--json", "logs", "--search", "--", "--json"][..],
            "/api/logs?search=--json",
            true,
        ),
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
            true,
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
            true,
        ),
        (
            &["logbrew", "logs", "--search=--timeout", "failed", "--json"][..],
            "/api/logs?search=--timeout%20failed",
            true,
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
            true,
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
            true,
        ),
        (
            &["logbrew", "logs", "--", "--json", "--json"][..],
            "/api/logs?search=--json",
            true,
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("logs separator parses");

        assert_eq!(
            command.http_path().expect("read logs has endpoint"),
            expected_path
        );
        assert!(matches!(command, Command::Read { json: actual, .. } if actual == json));
    }
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
        let command = parse_command(args.iter().copied()).expect("search alias parses");

        assert_eq!(
            command,
            Command::Read {
                target: ReadTarget::Logs,
                options: Box::new(ReadOptions {
                    name: None,
                    service: None,
                    since: None,
                    user: None,
                    trace: None,
                    level: Some("error".to_owned()),
                    search: Some("checkout failed".to_owned()),
                    project: None,
                    release: Some("api@1".to_owned()),
                    environment: Some("production".to_owned()),
                    status: None,
                    limit: None,
                    min_duration_ms: None,
                    pagination: None,
                    cursor_time: None,
                    cursor_id: None,
                }),
                json: true,
            }
        );
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
        let command = parse_command(args.iter().copied()).expect("search help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadLogs,
                json: false
            }
        );
    }

    assert_eq!(
        parse_command(["logbrew", "help", "search", "checkout", "--json"])
            .expect("explicit search term help parses"),
        Command::Help {
            topic: HelpTopic::ReadLogs,
            json: true
        }
    );
    assert!(
        parse_command(["logbrew", "help", "search", "checkout", "extra"]).is_err(),
        "extra search help terms remain errors"
    );
}
