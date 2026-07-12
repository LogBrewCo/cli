//! CLI real-user shortcut tests.

use logbrew_cli::{
    CliError, Command, ExplainTarget, ReadOptions, ReadTarget, SetTarget, WatchOptions,
    WatchTarget, parse_command,
};

#[test]
fn parses_top_level_singular_collection_shortcuts() {
    let log = parse_command(["logbrew", "log", "--release", "checkout@1", "--json"])
        .expect("log shortcut parses");
    assert_eq!(
        log,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: Some("checkout@1".to_owned()),
                environment: None,
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
    assert_eq!(
        log.http_path().expect("log shortcut has endpoint"),
        "/api/logs?release=checkout%401"
    );

    let release = parse_command([
        "logbrew",
        "release",
        "--environment",
        "production",
        "--json",
    ])
    .expect("release shortcut parses");
    assert_eq!(
        release,
        Command::Read {
            target: ReadTarget::Releases,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
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
    assert_eq!(
        release.http_path().expect("release shortcut has endpoint"),
        "/api/telemetry/releases?environment=production"
    );
}

#[test]
fn parses_common_human_terms_as_read_shortcuts() {
    let errors = parse_command([
        "logbrew",
        "errors",
        "--status",
        "unresolved",
        "--release",
        "checkout@1",
        "--json",
    ])
    .expect("errors shortcut parses");

    assert_eq!(
        errors,
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: Some("checkout@1".to_owned()),
                environment: None,
                status: Some("unresolved".to_owned()),
                limit: None,
                min_duration_ms: None,
                pagination: None,
                cursor_time: None,
                cursor_id: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        errors.http_path().expect("errors shortcut has endpoint"),
        "/api/telemetry/issues?status=unresolved&release=checkout%401"
    );

    let events = parse_command([
        "logbrew",
        "events",
        "--name",
        "checkout_failed",
        "--environment",
        "production",
        "--json",
    ])
    .expect("events shortcut parses");

    assert_eq!(
        events,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
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
    assert_eq!(
        events.http_path().expect("events shortcut has endpoint"),
        "/api/telemetry/actions?name=checkout_failed&environment=production"
    );

    let read_exceptions =
        parse_command(["logbrew", "read", "exceptions", "--json"]).expect("exceptions parse");
    assert_eq!(
        read_exceptions,
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
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
            json: true,
        }
    );

    let read_event = parse_command(["logbrew", "read", "event", "--json"]).expect("event parse");
    assert_eq!(
        read_event,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
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
            json: true,
        }
    );
}

#[test]
fn parses_issue_status_words_as_issue_list_filters() {
    for (args, expected_path) in [
        (
            &[
                "logbrew",
                "issues",
                "open",
                "--release",
                "checkout@1",
                "--json",
            ][..],
            "/api/telemetry/issues?status=unresolved&release=checkout%401",
        ),
        (
            &[
                "logbrew",
                "errors",
                "closed",
                "--environment",
                "production",
                "--json",
            ],
            "/api/telemetry/issues?status=resolved&environment=production",
        ),
        (
            &["logbrew", "latest", "issues", "ignored", "--json"],
            "/api/telemetry/issues?status=ignored",
        ),
        (
            &["logbrew", "last", "open", "issues", "--json"],
            "/api/telemetry/issues?status=unresolved",
        ),
        (
            &["logbrew", "last", "5", "open", "issues", "--json"],
            "/api/telemetry/issues?status=unresolved&limit=5",
        ),
        (
            &[
                "logbrew", "read", "recent", "3", "closed", "errors", "--json",
            ],
            "/api/telemetry/issues?status=resolved&limit=3",
        ),
        (
            &["logbrew", "read", "exceptions", "unresolved", "--json"],
            "/api/telemetry/issues?status=unresolved",
        ),
        (
            &["logbrew", "issue", "open", "--json"],
            "/api/telemetry/issues?status=unresolved",
        ),
        (
            &["logbrew", "read", "open", "issue", "--json"],
            "/api/telemetry/issues?status=unresolved",
        ),
        (
            &["logbrew", "read", "issue", "closed", "--json"],
            "/api/telemetry/issues?status=resolved",
        ),
        (
            &["logbrew", "show", "issue", "ignored", "--json"],
            "/api/telemetry/issues?status=ignored",
        ),
        (
            &[
                "logbrew",
                "open",
                "issues",
                "--release",
                "checkout@1",
                "--json",
            ],
            "/api/telemetry/issues?status=unresolved&release=checkout%401",
        ),
        (
            &["logbrew", "open", "issue", "--json"],
            "/api/telemetry/issues?status=unresolved",
        ),
        (
            &[
                "logbrew",
                "closed",
                "errors",
                "--environment",
                "production",
                "--json",
            ],
            "/api/telemetry/issues?status=resolved&environment=production",
        ),
        (
            &[
                "logbrew",
                "closed",
                "issue",
                "--environment",
                "production",
                "--json",
            ],
            "/api/telemetry/issues?status=resolved&environment=production",
        ),
        (
            &["logbrew", "last", "closed", "issue", "--json"],
            "/api/telemetry/issues?status=resolved",
        ),
        (
            &["logbrew", "last", "5", "closed", "issue", "--json"],
            "/api/telemetry/issues?status=resolved&limit=5",
        ),
        (
            &["logbrew", "ignored", "exceptions", "--json"],
            "/api/telemetry/issues?status=ignored",
        ),
        (
            &["logbrew", "resolved", "issues", "--json"],
            "/api/telemetry/issues?status=resolved",
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("issue status shortcut parses");

        assert_eq!(
            command.http_path().expect("issue list has endpoint"),
            expected_path
        );
    }
}

#[test]
fn keeps_filter_words_before_issue_status_shortcuts_recoverable() {
    let error = parse_command(["logbrew", "issues", "release", "checkout@1", "--json"])
        .expect_err("filter word before issue status fails");

    assert_eq!(
        error,
        CliError::UnexpectedArgument {
            argument: "release".to_owned(),
            command: "read",
            next: "use --release <release>",
        }
    );

    let status_first_error = parse_command([
        "logbrew",
        "open",
        "issues",
        "release",
        "checkout@1",
        "--json",
    ])
    .expect_err("filter word after status-first issue shortcut fails");

    assert_eq!(
        status_first_error,
        CliError::UnexpectedArgument {
            argument: "release".to_owned(),
            command: "read",
            next: "use --release <release>",
        }
    );
}

#[test]
fn parses_recency_words_as_read_shortcuts() {
    let latest_logs = parse_command(["logbrew", "latest", "logs", "--limit", "20", "--json"])
        .expect("latest logs parses");
    assert_eq!(
        latest_logs,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
                environment: None,
                status: None,
                limit: Some("20".to_owned()),
                min_duration_ms: None,
                pagination: None,
                cursor_time: None,
                cursor_id: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        latest_logs.http_path().expect("latest logs has endpoint"),
        "/api/logs?limit=20"
    );

    let recent_issues =
        parse_command(["logbrew", "recent", "issues", "--status", "open", "--json"])
            .expect("recent issues parses");
    assert_eq!(
        recent_issues
            .http_path()
            .expect("recent issues has endpoint"),
        "/api/telemetry/issues?status=unresolved"
    );

    let last_action = parse_command(["logbrew", "last", "action", "checkout_failed", "--json"])
        .expect("last action parses");
    assert_eq!(
        last_action.http_path().expect("last action has endpoint"),
        "/api/telemetry/actions?name=checkout_failed"
    );

    let newest_release = parse_command([
        "logbrew",
        "newest",
        "release",
        "--environment",
        "production",
        "--json",
    ])
    .expect("newest release parses");
    assert_eq!(
        newest_release
            .http_path()
            .expect("newest release has endpoint"),
        "/api/telemetry/releases?environment=production"
    );
}

#[test]
fn parses_recency_count_before_resource_as_limit() {
    for (args, expected_path) in [
        (
            &["logbrew", "last", "10", "logs", "--json"][..],
            "/api/logs?limit=10",
        ),
        (
            &[
                "logbrew", "read", "recent", "5", "issues", "--status", "open", "--json",
            ],
            "/api/telemetry/issues?status=unresolved&limit=5",
        ),
        (
            &[
                "logbrew",
                "latest",
                "3",
                "events",
                "checkout_failed",
                "--json",
            ],
            "/api/telemetry/actions?name=checkout_failed&limit=3",
        ),
        (
            &[
                "logbrew",
                "newest",
                "2",
                "release",
                "--environment",
                "production",
                "--json",
            ],
            "/api/telemetry/releases?environment=production&limit=2",
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("recency count shortcut parses");

        assert_eq!(
            command.http_path().expect("recency count has endpoint"),
            expected_path
        );
    }
}

#[test]
fn parses_read_prefixed_recency_and_status_shortcuts() {
    for (args, expected_path) in [
        (
            &[
                "logbrew", "read", "latest", "logs", "--limit", "20", "--json",
            ][..],
            "/api/logs?limit=20",
        ),
        (
            &[
                "logbrew", "read", "recent", "issues", "--status", "open", "--json",
            ],
            "/api/telemetry/issues?status=unresolved",
        ),
        (
            &[
                "logbrew",
                "read",
                "last",
                "action",
                "checkout_failed",
                "--json",
            ],
            "/api/telemetry/actions?name=checkout_failed",
        ),
        (
            &[
                "logbrew",
                "read",
                "newest",
                "release",
                "--environment",
                "production",
                "--json",
            ],
            "/api/telemetry/releases?environment=production",
        ),
        (
            &[
                "logbrew",
                "read",
                "open",
                "issues",
                "--release",
                "checkout@1",
                "--json",
            ],
            "/api/telemetry/issues?status=unresolved&release=checkout%401",
        ),
        (
            &[
                "logbrew",
                "read",
                "closed",
                "errors",
                "--environment",
                "production",
                "--json",
            ],
            "/api/telemetry/issues?status=resolved&environment=production",
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("read-prefixed shortcut parses");

        assert_eq!(
            command
                .http_path()
                .expect("read-prefixed shortcut has endpoint"),
            expected_path
        );
    }
}

#[test]
fn keeps_filter_words_after_read_prefixed_status_shortcuts_recoverable() {
    let error = parse_command([
        "logbrew",
        "read",
        "open",
        "issues",
        "release",
        "checkout@1",
        "--json",
    ])
    .expect_err("filter word after read-prefixed status shortcut fails");

    assert_eq!(
        error,
        CliError::UnexpectedArgument {
            argument: "release".to_owned(),
            command: "read",
            next: "use --release <release>",
        }
    );
}

#[test]
fn parses_positional_log_level_shortcuts_for_noisy_logs() {
    for (args, expected_path) in [
        (
            &["logbrew", "logs", "error", "--json"][..],
            "/api/logs?severity=error",
        ),
        (
            &["logbrew", "read", "logs", "warning", "--json"],
            "/api/logs?severity=warning",
        ),
        (
            &["logbrew", "latest", "logs", "fatal", "--json"],
            "/api/logs?severity=critical",
        ),
        (
            &["logbrew", "--json", "logs", "err"],
            "/api/logs?severity=error",
        ),
        (
            &["logbrew", "logs", "error", "checkout", "failed", "--json"],
            "/api/logs?severity=error&search=checkout%20failed",
        ),
        (
            &[
                "logbrew",
                "latest",
                "logs",
                "warning",
                "checkout failed",
                "--release",
                "api@1",
                "--json",
            ],
            "/api/logs?severity=warning&search=checkout%20failed&release=api%401",
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("command parses");

        assert_eq!(
            command.http_path().expect("read logs has endpoint"),
            expected_path
        );
    }
}

#[test]
fn rejects_filter_words_after_positional_log_level_shortcuts() {
    let error = parse_command([
        "logbrew",
        "logs",
        "error",
        "release",
        "checkout@1",
        "--json",
    ])
    .expect_err("filter word after positional log level fails");

    assert_eq!(
        error,
        CliError::UnexpectedArgument {
            argument: "release".to_owned(),
            command: "read",
            next: "use --release <release>",
        }
    );
}

#[test]
fn parses_natural_log_search_after_log_shortcuts() {
    for (args, expected_path) in [
        (
            &["logbrew", "logs", "checkout", "failed", "--json"][..],
            "/api/logs?search=checkout%20failed",
        ),
        (
            &[
                "logbrew",
                "latest",
                "logs",
                "checkout failed",
                "--level",
                "error",
                "--release",
                "api@1",
                "--json",
            ],
            "/api/logs?severity=error&search=checkout%20failed&release=api%401",
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("natural logs search parses");

        assert_eq!(
            command.http_path().expect("read logs has endpoint"),
            expected_path
        );
    }
}

#[test]
fn parses_natural_log_search_after_explicit_log_filters() {
    for (args, expected_path) in [
        (
            &[
                "logbrew", "logs", "--level", "error", "checkout", "failed", "--json",
            ][..],
            "/api/logs?severity=error&search=checkout%20failed",
        ),
        (
            &[
                "logbrew",
                "latest",
                "logs",
                "--level",
                "warning",
                "checkout failed",
                "--release",
                "api@1",
                "--json",
            ],
            "/api/logs?severity=warning&search=checkout%20failed&release=api%401",
        ),
        (
            &[
                "logbrew", "logs", "--search", "checkout", "failed", "--level", "error", "--json",
            ],
            "/api/logs?severity=error&search=checkout%20failed",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--release",
                "checkout@1",
                "checkout",
                "failed",
                "--environment",
                "production",
                "--json",
            ],
            "/api/logs?search=checkout%20failed&release=checkout%401&environment=production",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--environment=production",
                "checkout",
                "failed",
                "--limit",
                "10",
                "--json",
            ],
            "/api/logs?search=checkout%20failed&environment=production&limit=10",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--trace",
                "trace_123",
                "checkout",
                "failed",
                "--json",
            ],
            "/api/logs?search=checkout%20failed&trace_id=trace_123",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--service-name",
                "checkout-api",
                "checkout",
                "failed",
                "--json",
            ],
            "/api/logs?service_name=checkout-api&search=checkout%20failed",
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("explicit filter search parses");

        assert_eq!(
            command.http_path().expect("read logs has endpoint"),
            expected_path
        );
    }
}

#[test]
fn keeps_ambiguous_log_positionals_as_recoverable_errors() {
    for (args, expected_argument, expected_next) in [
        (
            &["logbrew", "logs", "checkout@1.2.3", "--json"][..],
            "checkout@1.2.3",
            "use --release <release> or run logbrew read --help",
        ),
        (
            &["logbrew", "logs", "env", "production", "--json"],
            "env",
            "use --environment <environment> or --env <environment>",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--level",
                "error",
                "release",
                "checkout@1",
                "--json",
            ],
            "release",
            "use --release <release>",
        ),
    ] {
        let error = parse_command(args.iter().copied()).expect_err("ambiguous positional fails");

        assert_eq!(
            error,
            CliError::UnexpectedArgument {
                argument: expected_argument.to_owned(),
                command: "read",
                next: expected_next,
            }
        );
    }
}

#[test]
fn parses_action_aliases_with_names_as_name_filters() {
    let events = parse_command([
        "logbrew",
        "events",
        "checkout_failed",
        "--release",
        "checkout@1",
        "--environment",
        "production",
        "--json",
    ])
    .expect("event name shortcut parses");

    assert_eq!(
        events,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: Some("checkout@1".to_owned()),
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
    assert_eq!(
        events
            .http_path()
            .expect("event name shortcut has endpoint"),
        "/api/telemetry/actions?name=checkout_failed&release=checkout%401&environment=production"
    );

    let action = parse_command([
        "logbrew",
        "action",
        "--json",
        "checkout_failed",
        "--env",
        "production",
    ])
    .expect("global json action name shortcut parses");
    assert_eq!(
        action,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
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
    assert_eq!(
        action
            .http_path()
            .expect("action name shortcut has endpoint"),
        "/api/telemetry/actions?name=checkout_failed&environment=production"
    );

    let read_events = parse_command(["logbrew", "read", "events", "checkout_failed", "--json"])
        .expect("read event name shortcut parses");
    assert_eq!(
        read_events,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
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
            json: true,
        }
    );
    assert_eq!(
        read_events
            .http_path()
            .expect("read event name shortcut has endpoint"),
        "/api/telemetry/actions?name=checkout_failed"
    );

    let read_action = parse_command(["logbrew", "read", "action", "checkout_failed", "--json"])
        .expect("read action name shortcut parses");
    assert_eq!(
        read_action
            .http_path()
            .expect("read action name shortcut has endpoint"),
        "/api/telemetry/actions?name=checkout_failed"
    );
}

#[test]
fn parses_live_reading_verbs_as_watch_shortcuts() {
    for (args, expected_target, expected_json) in [
        (
            &["logbrew", "tail", "logs", "--json"][..],
            WatchTarget::Logs,
            true,
        ),
        (
            &["logbrew", "follow", "actions"],
            WatchTarget::Actions,
            false,
        ),
        (
            &["logbrew", "stream", "events", "--json"],
            WatchTarget::All,
            true,
        ),
        (
            &["logbrew", "--json", "tail", "logs"],
            WatchTarget::Logs,
            true,
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("watch shortcut parses");

        assert_eq!(
            command,
            Command::Watch {
                target: expected_target,
                options: WatchOptions::default(),
                json: expected_json
            }
        );
        assert!(command.http_path().is_none());
    }
}

#[test]
fn parses_issue_status_shortcuts_as_mutations() {
    for (args, expected_status) in [
        (
            &["logbrew", "resolve", "issue_123", "--json"][..],
            "resolved",
        ),
        (&["logbrew", "close", "issue_123", "--json"], "resolved"),
        (&["logbrew", "ignore", "issue_123", "--json"], "ignored"),
        (&["logbrew", "reopen", "issue_123", "--json"], "unresolved"),
        (
            &["logbrew", "issue", "issue_123", "resolve", "--json"],
            "resolved",
        ),
        (
            &["logbrew", "issue", "issue_123", "close", "--json"],
            "resolved",
        ),
        (
            &["logbrew", "issue", "issue_123", "ignore", "--json"],
            "ignored",
        ),
        (
            &["logbrew", "issue", "issue_123", "reopen", "--json"],
            "unresolved",
        ),
        (
            &["logbrew", "issue", "issue_123", "resolved", "--json"],
            "resolved",
        ),
        (
            &["logbrew", "issue", "issue_123", "closed", "--json"],
            "resolved",
        ),
        (
            &["logbrew", "issue", "issue_123", "ignored", "--json"],
            "ignored",
        ),
        (
            &["logbrew", "issue", "issue_123", "open", "--json"],
            "unresolved",
        ),
        (&["logbrew", "issue_123", "resolved", "--json"], "resolved"),
        (&["logbrew", "issue_123", "closed", "--json"], "resolved"),
        (&["logbrew", "issue_123", "ignored", "--json"], "ignored"),
        (&["logbrew", "issue_123", "open", "--json"], "unresolved"),
    ] {
        let command = parse_command(args.iter().copied()).expect("status shortcut parses");

        assert_eq!(
            command,
            Command::Set {
                target: SetTarget::IssueStatus {
                    id: "issue_123".to_owned(),
                    status: expected_status.to_owned(),
                },
                json: true,
            }
        );
        assert_eq!(
            command.http_path().expect("status shortcut has endpoint"),
            "/api/telemetry/issues/issue_123"
        );
        assert_eq!(
            command.request_body().expect("status shortcut has body"),
            serde_json::json!({ "status": expected_status })
        );
    }
}

#[test]
fn parses_issue_aliases_with_ids_as_issue_detail_shortcuts() {
    for args in [
        &["logbrew", "issues", "issue_123", "--json"][..],
        &["logbrew", "errors", "issue_123", "--json"],
        &["logbrew", "exception", "--json", "issue-123"],
        &["logbrew", "read", "exceptions", "issue_123", "--json"],
    ] {
        let command = parse_command(args.iter().copied()).expect("issue alias id parses");

        let expected_id = if args.contains(&"issue-123") {
            "issue-123"
        } else {
            "issue_123"
        };
        assert_eq!(
            command,
            Command::Read {
                target: ReadTarget::Issue(expected_id.to_owned()),
                options: Box::default(),
                json: true,
            }
        );
        assert_eq!(
            command.http_path().expect("issue alias id has endpoint"),
            format!("/api/telemetry/issues/{expected_id}")
        );
    }
}

#[test]
fn parses_json_before_shortcut_ids() {
    let issue = parse_command(["logbrew", "issue", "--json", "issue_123"]).expect("issue parses");
    assert_eq!(
        issue,
        Command::Read {
            target: ReadTarget::Issue("issue_123".to_owned()),
            options: Box::default(),
            json: true,
        }
    );

    let trace = parse_command(["logbrew", "trace", "--json", "trace-123"]).expect("trace parses");
    assert_eq!(
        trace,
        Command::Read {
            target: ReadTarget::Trace("trace-123".to_owned()),
            options: Box::default(),
            json: true,
        }
    );

    let resolve =
        parse_command(["logbrew", "resolve", "--json", "issue_123"]).expect("resolve parses");
    assert_eq!(
        resolve,
        Command::Set {
            target: SetTarget::IssueStatus {
                id: "issue_123".to_owned(),
                status: "resolved".to_owned(),
            },
            json: true,
        }
    );

    let issue_first_resolve = parse_command(["logbrew", "issue", "--json", "issue_123", "resolve"])
        .expect("issue-first resolve parses");
    assert_eq!(
        issue_first_resolve,
        Command::Set {
            target: SetTarget::IssueStatus {
                id: "issue_123".to_owned(),
                status: "resolved".to_owned(),
            },
            json: true,
        }
    );

    let pasted_id_resolve = parse_command(["logbrew", "issue_123", "resolve", "--json"])
        .expect("pasted id resolve parses");
    assert_eq!(
        pasted_id_resolve,
        Command::Set {
            target: SetTarget::IssueStatus {
                id: "issue_123".to_owned(),
                status: "resolved".to_owned(),
            },
            json: true,
        }
    );

    let pasted_id_close =
        parse_command(["logbrew", "--json", "issue_123", "close"]).expect("pasted id close parses");
    assert_eq!(
        pasted_id_close,
        Command::Set {
            target: SetTarget::IssueStatus {
                id: "issue_123".to_owned(),
                status: "resolved".to_owned(),
            },
            json: true,
        }
    );
}

#[test]
fn parses_trace_vocabulary_with_ids_as_trace_detail_shortcuts() {
    let trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";

    let traces = parse_command(["logbrew", "traces", trace_id, "--json"]).expect("traces parses");
    assert_eq!(
        traces,
        Command::Read {
            target: ReadTarget::Trace(trace_id.to_owned()),
            options: Box::default(),
            json: true,
        }
    );

    let spans = parse_command(["logbrew", "spans", "--json", trace_id]).expect("spans parses");
    assert_eq!(
        spans,
        Command::Read {
            target: ReadTarget::Trace(trace_id.to_owned()),
            options: Box::default(),
            json: true,
        }
    );

    let read_spans =
        parse_command(["logbrew", "read", "spans", trace_id, "--json"]).expect("read spans parses");
    assert_eq!(
        read_spans,
        Command::Read {
            target: ReadTarget::Trace(trace_id.to_owned()),
            options: Box::default(),
            json: true,
        }
    );
}

#[test]
fn parses_common_read_verbs_as_read_shortcuts() {
    let show_logs = parse_command([
        "logbrew",
        "show",
        "logs",
        "--release",
        "checkout@1",
        "--json",
    ])
    .expect("show logs parses");

    assert_eq!(
        show_logs,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: Some("checkout@1".to_owned()),
                environment: None,
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
    assert_eq!(
        show_logs.http_path().expect("show logs has endpoint"),
        "/api/logs?release=checkout%401"
    );

    let list_errors = parse_command(["logbrew", "list", "errors", "--status", "open"])
        .expect("list errors parses");
    assert_eq!(
        list_errors,
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
                environment: None,
                status: Some("unresolved".to_owned()),
                limit: None,
                min_duration_ms: None,
                pagination: None,
                cursor_time: None,
                cursor_id: None,
            }),
            json: false,
        }
    );

    for (args, target, path) in [
        (
            &["logbrew", "show", "log", "--json"][..],
            ReadTarget::Logs,
            "/api/logs",
        ),
        (
            &["logbrew", "show", "release", "--json"],
            ReadTarget::Releases,
            "/api/telemetry/releases",
        ),
        (
            &["logbrew", "list", "issue", "--json"],
            ReadTarget::Issues,
            "/api/telemetry/issues",
        ),
        (
            &["logbrew", "list", "log", "--json"],
            ReadTarget::Logs,
            "/api/logs",
        ),
        (
            &["logbrew", "list", "release", "--json"],
            ReadTarget::Releases,
            "/api/telemetry/releases",
        ),
        (
            &["logbrew", "get", "log", "--json"],
            ReadTarget::Logs,
            "/api/logs",
        ),
        (
            &["logbrew", "get", "release", "--json"],
            ReadTarget::Releases,
            "/api/telemetry/releases",
        ),
        (
            &["logbrew", "read", "log", "--json"],
            ReadTarget::Logs,
            "/api/logs",
        ),
        (
            &["logbrew", "read", "release", "--json"],
            ReadTarget::Releases,
            "/api/telemetry/releases",
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("singular read parses");
        assert_eq!(
            command,
            Command::Read {
                target,
                options: Box::default(),
                json: true
            }
        );
        assert_eq!(
            command.http_path().expect("singular read has endpoint"),
            path
        );
    }

    let get_issue = parse_command(["logbrew", "get", "issue", "issue_123", "--json"])
        .expect("get issue parses");
    assert_eq!(
        get_issue,
        Command::Read {
            target: ReadTarget::Issue("issue_123".to_owned()),
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
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
            json: true,
        }
    );
    assert_eq!(
        get_issue.http_path().expect("get issue has endpoint"),
        "/api/telemetry/issues/issue_123"
    );
}

#[test]
fn parses_pasted_explain_ids_without_resource_word() {
    let issue = parse_command(["logbrew", "explain", "issue_123", "--json"])
        .expect("explain issue id parses");

    assert_eq!(
        issue,
        Command::Explain {
            target: ExplainTarget::Issue("issue_123".to_owned()),
            json: true,
        }
    );
    assert_eq!(
        issue.http_path().expect("inferred issue has endpoint"),
        "/api/telemetry/issues/issue_123"
    );

    let uuid_issue = parse_command([
        "logbrew",
        "explain",
        "123e4567-e89b-12d3-a456-426614174000",
        "--json",
    ])
    .expect("explain uuid issue parses");
    assert_eq!(
        uuid_issue,
        Command::Explain {
            target: ExplainTarget::Issue("123e4567-e89b-12d3-a456-426614174000".to_owned()),
            json: true,
        }
    );

    let trace = parse_command([
        "logbrew",
        "explain",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "--json",
    ])
    .expect("explain trace id parses");

    assert_eq!(
        trace,
        Command::Explain {
            target: ExplainTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            json: true,
        }
    );
    assert_eq!(
        trace.http_path().expect("inferred trace has endpoint"),
        "/api/telemetry/traces/4bf92f3577b34da6a3ce929d0e0e4736"
    );

    let issue_suffix = parse_command(["logbrew", "issue_123", "explain", "--json"])
        .expect("issue id explain suffix parses");
    assert_eq!(
        issue_suffix,
        Command::Explain {
            target: ExplainTarget::Issue("issue_123".to_owned()),
            json: true,
        }
    );

    let trace_suffix = parse_command([
        "logbrew",
        "--json",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "explain",
    ])
    .expect("trace id explain suffix parses");
    assert_eq!(
        trace_suffix,
        Command::Explain {
            target: ExplainTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            json: true,
        }
    );
}

#[test]
fn parses_resource_detail_explain_suffixes() {
    let issue = parse_command(["logbrew", "issue", "issue_123", "explain", "--json"])
        .expect("issue detail explain suffix parses");
    assert_eq!(
        issue,
        Command::Explain {
            target: ExplainTarget::Issue("issue_123".to_owned()),
            json: true,
        }
    );

    let trace = parse_command([
        "logbrew",
        "--json",
        "trace",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "explain",
    ])
    .expect("trace detail explain suffix parses");
    assert_eq!(
        trace,
        Command::Explain {
            target: ExplainTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            json: true,
        }
    );
}

#[test]
fn parses_pasted_detail_ids_as_read_shortcuts() {
    let issue = parse_command(["logbrew", "issue_123", "--json"]).expect("issue id parses");
    assert_eq!(
        issue,
        Command::Read {
            target: ReadTarget::Issue("issue_123".to_owned()),
            options: Box::default(),
            json: true,
        }
    );
    assert_eq!(
        issue.http_path().expect("issue id has endpoint"),
        "/api/telemetry/issues/issue_123"
    );

    let trace = parse_command(["logbrew", "--json", "4bf92f3577b34da6a3ce929d0e0e4736"])
        .expect("trace id parses");
    assert_eq!(
        trace,
        Command::Read {
            target: ReadTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            options: Box::default(),
            json: true,
        }
    );
    assert_eq!(
        trace.http_path().expect("trace id has endpoint"),
        "/api/telemetry/traces/4bf92f3577b34da6a3ce929d0e0e4736"
    );
}
