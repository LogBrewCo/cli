//! CLI real-user shortcut tests.

use super::{TestResult, assert_command, assert_path};
use logbrew_cli::{
    CliError, Command, ExplainTarget, IssueOccurrenceSelection, ReadOptions, ReadTarget, SetTarget,
    WatchOptions, WatchTarget, parse_command,
};

#[test]
fn parses_top_level_singular_collection_shortcuts() -> TestResult {
    let log = parse_command(["logbrew", "log", "--release", "checkout@1", "--json"])?;
    assert_eq!(
        log,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                release: Some("checkout@1".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        log.http_path().ok_or("log shortcut has endpoint")?,
        "/api/logs?release=checkout%401"
    );

    let release = parse_command([
        "logbrew",
        "release",
        "--environment",
        "production",
        "--json",
    ])?;
    assert_eq!(
        release,
        Command::Read {
            target: ReadTarget::Releases,
            options: Box::new(ReadOptions {
                environment: Some("production".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        release.http_path().ok_or("release shortcut has endpoint")?,
        "/api/telemetry/releases?environment=production"
    );
    Ok(())
}

#[test]
fn parses_common_human_terms_as_read_shortcuts() -> TestResult {
    let errors = parse_command([
        "logbrew",
        "errors",
        "--status",
        "unresolved",
        "--release",
        "checkout@1",
        "--json",
    ])?;

    assert_eq!(
        errors,
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::new(ReadOptions {
                release: Some("checkout@1".to_owned()),
                status: Some("unresolved".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        errors.http_path().ok_or("errors shortcut has endpoint")?,
        "/api/telemetry/issues?status=unresolved&release=checkout%401&response_version=4"
    );

    let events = parse_command([
        "logbrew",
        "events",
        "--name",
        "checkout_failed",
        "--environment",
        "production",
        "--json",
    ])?;

    assert_eq!(
        events,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                environment: Some("production".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        events.http_path().ok_or("events shortcut has endpoint")?,
        "/api/telemetry/actions?name=checkout_failed&environment=production"
    );

    let read_exceptions = parse_command(["logbrew", "read", "exceptions", "--json"])?;
    assert_eq!(
        read_exceptions,
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::default(),
            json: true,
        }
    );

    let read_event = parse_command(["logbrew", "read", "event", "--json"])?;
    assert_eq!(
        read_event,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::default(),
            json: true,
        }
    );
    Ok(())
}

#[test]
fn parses_issue_status_words_as_issue_list_filters() {
    for (args, expected_path) in [
        (
            &["logbrew", "latest", "issues", "ignored", "--json"][..],
            "/api/telemetry/issues?status=ignored&response_version=4",
        ),
        (
            &["logbrew", "last", "open", "issues", "--json"],
            "/api/telemetry/issues?status=unresolved&response_version=4",
        ),
        (
            &["logbrew", "read", "exceptions", "unresolved", "--json"],
            "/api/telemetry/issues?status=unresolved&response_version=4",
        ),
        (
            &["logbrew", "issue", "open", "--json"],
            "/api/telemetry/issues?status=unresolved&response_version=4",
        ),
        (
            &["logbrew", "read", "open", "issue", "--json"],
            "/api/telemetry/issues?status=unresolved&response_version=4",
        ),
        (
            &["logbrew", "read", "issue", "closed", "--json"],
            "/api/telemetry/issues?status=resolved&response_version=4",
        ),
        (
            &["logbrew", "show", "issue", "ignored", "--json"],
            "/api/telemetry/issues?status=ignored&response_version=4",
        ),
        (
            &["logbrew", "open", "issue", "--json"],
            "/api/telemetry/issues?status=unresolved&response_version=4",
        ),
        (
            &["logbrew", "last", "closed", "issue", "--json"],
            "/api/telemetry/issues?status=resolved&response_version=4",
        ),
        (
            &["logbrew", "ignored", "exceptions", "--json"],
            "/api/telemetry/issues?status=ignored&response_version=4",
        ),
        (
            &["logbrew", "resolved", "issues", "--json"],
            "/api/telemetry/issues?status=resolved&response_version=4",
        ),
    ] {
        assert_path(args, expected_path);
    }
}

#[test]
fn parses_issue_status_shortcuts_with_scope_and_count() {
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
            "/api/telemetry/issues?status=unresolved&release=checkout%401&response_version=4",
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
            "/api/telemetry/issues?status=resolved&environment=production&response_version=4",
        ),
        (
            &["logbrew", "last", "5", "open", "issues", "--json"],
            "/api/telemetry/issues?status=unresolved&limit=5&response_version=4",
        ),
        (
            &[
                "logbrew", "read", "recent", "3", "closed", "errors", "--json",
            ],
            "/api/telemetry/issues?status=resolved&limit=3&response_version=4",
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
            "/api/telemetry/issues?status=unresolved&release=checkout%401&response_version=4",
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
            "/api/telemetry/issues?status=resolved&environment=production&response_version=4",
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
            "/api/telemetry/issues?status=resolved&environment=production&response_version=4",
        ),
        (
            &["logbrew", "last", "5", "closed", "issue", "--json"],
            "/api/telemetry/issues?status=resolved&limit=5&response_version=4",
        ),
    ] {
        assert_path(args, expected_path);
    }
}

#[test]
fn keeps_filter_words_before_issue_status_shortcuts_recoverable() -> TestResult {
    let error = parse_command(["logbrew", "issues", "release", "checkout@1", "--json"])
        .err()
        .ok_or("filter word before issue status fails")?;

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
    .err()
    .ok_or("filter word after status-first issue shortcut fails")?;

    assert_eq!(
        status_first_error,
        CliError::UnexpectedArgument {
            argument: "release".to_owned(),
            command: "read",
            next: "use --release <release>",
        }
    );
    Ok(())
}

#[test]
fn parses_recency_words_as_read_shortcuts() -> TestResult {
    let latest_logs = parse_command(["logbrew", "latest", "logs", "--limit", "20", "--json"])?;
    assert_eq!(
        latest_logs,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                limit: Some("20".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        latest_logs.http_path().ok_or("latest logs has endpoint")?,
        "/api/logs?limit=20"
    );

    let recent_issues =
        parse_command(["logbrew", "recent", "issues", "--status", "open", "--json"])?;
    assert_eq!(
        recent_issues
            .http_path()
            .ok_or("recent issues has endpoint")?,
        "/api/telemetry/issues?status=unresolved&response_version=4"
    );

    let last_action = parse_command(["logbrew", "last", "action", "checkout_failed", "--json"])?;
    assert_eq!(
        last_action.http_path().ok_or("last action has endpoint")?,
        "/api/telemetry/actions?name=checkout_failed"
    );

    let newest_release = parse_command([
        "logbrew",
        "newest",
        "release",
        "--environment",
        "production",
        "--json",
    ])?;
    assert_eq!(
        newest_release
            .http_path()
            .ok_or("newest release has endpoint")?,
        "/api/telemetry/releases?environment=production"
    );
    Ok(())
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
            "/api/telemetry/issues?status=unresolved&limit=5&response_version=4",
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
        assert_path(args, expected_path);
    }
}

#[test]
fn parses_read_prefixed_recency_and_status_shortcuts() -> TestResult {
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
            "/api/telemetry/issues?status=unresolved&response_version=4",
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
            "/api/telemetry/issues?status=unresolved&release=checkout%401&response_version=4",
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
            "/api/telemetry/issues?status=resolved&environment=production&response_version=4",
        ),
    ] {
        let command = parse_command(args.iter().copied())?;

        assert_eq!(
            command
                .http_path()
                .ok_or("read-prefixed shortcut has endpoint")?,
            expected_path
        );
    }
    Ok(())
}

#[test]
fn keeps_filter_words_after_read_prefixed_status_shortcuts_recoverable() -> TestResult {
    let error = parse_command([
        "logbrew",
        "read",
        "open",
        "issues",
        "release",
        "checkout@1",
        "--json",
    ])
    .err()
    .ok_or("filter word after read-prefixed status shortcut fails")?;

    assert_eq!(
        error,
        CliError::UnexpectedArgument {
            argument: "release".to_owned(),
            command: "read",
            next: "use --release <release>",
        }
    );
    Ok(())
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
        assert_path(args, expected_path);
    }
}

#[test]
fn rejects_filter_words_after_positional_log_level_shortcuts() -> TestResult {
    let error = parse_command([
        "logbrew",
        "logs",
        "error",
        "release",
        "checkout@1",
        "--json",
    ])
    .err()
    .ok_or("filter word after positional log level fails")?;

    assert_eq!(
        error,
        CliError::UnexpectedArgument {
            argument: "release".to_owned(),
            command: "read",
            next: "use --release <release>",
        }
    );
    Ok(())
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
        assert_path(args, expected_path);
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
        assert_path(args, expected_path);
    }
}

#[test]
fn keeps_ambiguous_log_positionals_as_recoverable_errors() -> TestResult {
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
        let error = parse_command(args.iter().copied())
            .err()
            .ok_or("ambiguous positional fails")?;

        assert_eq!(
            error,
            CliError::UnexpectedArgument {
                argument: expected_argument.to_owned(),
                command: "read",
                next: expected_next,
            }
        );
    }
    Ok(())
}

#[test]
fn parses_action_aliases_with_names_as_name_filters() -> TestResult {
    let events = parse_command([
        "logbrew",
        "events",
        "checkout_failed",
        "--release",
        "checkout@1",
        "--environment",
        "production",
        "--json",
    ])?;

    assert_eq!(
        events,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                release: Some("checkout@1".to_owned()),
                environment: Some("production".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        events
            .http_path()
            .ok_or("event name shortcut has endpoint")?,
        "/api/telemetry/actions?name=checkout_failed&release=checkout%401&environment=production"
    );

    let action = parse_command([
        "logbrew",
        "action",
        "--json",
        "checkout_failed",
        "--env",
        "production",
    ])?;
    assert_eq!(
        action,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                environment: Some("production".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        action
            .http_path()
            .ok_or("action name shortcut has endpoint")?,
        "/api/telemetry/actions?name=checkout_failed&environment=production"
    );

    let read_events = parse_command(["logbrew", "read", "events", "checkout_failed", "--json"])?;
    assert_eq!(
        read_events,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        read_events
            .http_path()
            .ok_or("read event name shortcut has endpoint")?,
        "/api/telemetry/actions?name=checkout_failed"
    );

    let read_action = parse_command(["logbrew", "read", "action", "checkout_failed", "--json"])?;
    assert_eq!(
        read_action
            .http_path()
            .ok_or("read action name shortcut has endpoint")?,
        "/api/telemetry/actions?name=checkout_failed"
    );
    Ok(())
}

#[test]
fn parses_live_reading_verbs_as_watch_shortcuts() -> TestResult {
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
        let command = parse_command(args.iter().copied())?;

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
    Ok(())
}

#[test]
fn parses_issue_status_shortcuts_as_mutations() -> TestResult {
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
        let command = parse_command(args.iter().copied())?;

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
            command.http_path().ok_or("status shortcut has endpoint")?,
            "/api/telemetry/issues/issue_123?response_version=4"
        );
        assert_eq!(
            command.request_body().ok_or("status shortcut has body")?,
            serde_json::json!({ "status": expected_status })
        );
    }
    Ok(())
}

#[test]
fn parses_issue_aliases_with_ids_as_issue_detail_shortcuts() -> TestResult {
    for args in [
        &["logbrew", "issues", "issue_123", "--json"][..],
        &["logbrew", "errors", "issue_123", "--json"],
        &["logbrew", "exception", "--json", "issue-123"],
        &["logbrew", "read", "exceptions", "issue_123", "--json"],
    ] {
        let command = parse_command(args.iter().copied())?;

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
            command.http_path().ok_or("issue alias id has endpoint")?,
            format!("/api/telemetry/issues/{expected_id}?response_version=4")
        );
    }
    Ok(())
}

#[test]
fn parses_json_before_shortcut_ids() {
    for (args, target) in [
        (
            &["logbrew", "issue", "--json", "issue_123"][..],
            ReadTarget::Issue("issue_123".to_owned()),
        ),
        (
            &["logbrew", "trace", "--json", "trace-123"][..],
            ReadTarget::Trace("trace-123".to_owned()),
        ),
    ] {
        assert_command(
            args,
            Command::Read {
                target,
                options: Box::default(),
                json: true,
            },
        );
    }
    for args in [
        &["logbrew", "resolve", "--json", "issue_123"][..],
        &["logbrew", "issue", "--json", "issue_123", "resolve"],
        &["logbrew", "issue_123", "resolve", "--json"],
        &["logbrew", "--json", "issue_123", "close"],
    ] {
        assert_command(
            args,
            Command::Set {
                target: SetTarget::IssueStatus {
                    id: "issue_123".to_owned(),
                    status: "resolved".to_owned(),
                },
                json: true,
            },
        );
    }
}

#[test]
fn parses_trace_vocabulary_with_ids_as_trace_detail_shortcuts() {
    let trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";
    for args in [
        &["logbrew", "traces", trace_id, "--json"][..],
        &["logbrew", "spans", "--json", trace_id],
        &["logbrew", "read", "spans", trace_id, "--json"],
    ] {
        assert_command(
            args,
            Command::Read {
                target: ReadTarget::Trace(trace_id.to_owned()),
                options: Box::default(),
                json: true,
            },
        );
    }
}

#[test]
fn parses_common_read_verbs_as_read_shortcuts() -> TestResult {
    let show_logs = parse_command([
        "logbrew",
        "show",
        "logs",
        "--release",
        "checkout@1",
        "--json",
    ])?;

    assert_eq!(
        show_logs,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                release: Some("checkout@1".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
    assert_eq!(
        show_logs.http_path().ok_or("show logs has endpoint")?,
        "/api/logs?release=checkout%401"
    );

    let list_errors = parse_command(["logbrew", "list", "errors", "--status", "open"])?;
    assert_eq!(
        list_errors,
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::new(ReadOptions {
                status: Some("unresolved".to_owned()),
                ..ReadOptions::default()
            }),
            json: false,
        }
    );

    let get_issue = parse_command(["logbrew", "get", "issue", "issue_123", "--json"])?;
    assert_eq!(
        get_issue,
        Command::Read {
            target: ReadTarget::Issue("issue_123".to_owned()),
            options: Box::default(),
            json: true,
        }
    );
    assert_eq!(
        get_issue.http_path().ok_or("get issue has endpoint")?,
        "/api/telemetry/issues/issue_123?response_version=4"
    );
    Ok(())
}

#[test]
fn parses_singular_collections_with_read_verbs() -> TestResult {
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
            "/api/telemetry/issues?response_version=4",
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
        let command = parse_command(args.iter().copied())?;
        assert_eq!(
            command,
            Command::Read {
                target,
                options: Box::default(),
                json: true
            }
        );
        assert_eq!(
            command.http_path().ok_or("singular read has endpoint")?,
            path
        );
    }
    Ok(())
}

#[test]
fn parses_pasted_explain_ids_without_resource_word() -> TestResult {
    let issue = parse_command(["logbrew", "explain", "issue_123", "--json"])?;

    assert_eq!(
        issue,
        Command::Explain {
            target: ExplainTarget::Issue {
                id: "issue_123".to_owned(),
                occurrence: IssueOccurrenceSelection::Recommended,
            },
            json: true,
        }
    );
    assert_eq!(
        issue.http_path().ok_or("inferred issue has endpoint")?,
        "/api/telemetry/issues/issue_123/investigation?response_version=14&selection=recommended"
    );

    let uuid_issue = parse_command([
        "logbrew",
        "explain",
        "123e4567-e89b-12d3-a456-426614174000",
        "--json",
    ])?;
    assert_eq!(
        uuid_issue,
        Command::Explain {
            target: ExplainTarget::Issue {
                id: "123e4567-e89b-12d3-a456-426614174000".to_owned(),
                occurrence: IssueOccurrenceSelection::Recommended,
            },
            json: true,
        }
    );

    let trace = parse_command([
        "logbrew",
        "explain",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "--json",
    ])?;

    assert_eq!(
        trace,
        Command::Explain {
            target: ExplainTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            json: true,
        }
    );
    assert_eq!(
        trace.http_path().ok_or("inferred trace has endpoint")?,
        "/api/telemetry/traces/4bf92f3577b34da6a3ce929d0e0e4736/investigation?response_version=3"
    );

    let issue_suffix = parse_command(["logbrew", "issue_123", "explain", "--json"])?;
    assert_eq!(
        issue_suffix,
        Command::Explain {
            target: ExplainTarget::Issue {
                id: "issue_123".to_owned(),
                occurrence: IssueOccurrenceSelection::Recommended,
            },
            json: true,
        }
    );

    let trace_suffix = parse_command([
        "logbrew",
        "--json",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "explain",
    ])?;
    assert_eq!(
        trace_suffix,
        Command::Explain {
            target: ExplainTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            json: true,
        }
    );
    Ok(())
}

#[test]
fn parses_resource_detail_explain_suffixes() -> TestResult {
    let issue = parse_command(["logbrew", "issue", "issue_123", "explain", "--json"])?;
    assert_eq!(
        issue,
        Command::Explain {
            target: ExplainTarget::Issue {
                id: "issue_123".to_owned(),
                occurrence: IssueOccurrenceSelection::Recommended,
            },
            json: true,
        }
    );

    let trace = parse_command([
        "logbrew",
        "--json",
        "trace",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "explain",
    ])?;
    assert_eq!(
        trace,
        Command::Explain {
            target: ExplainTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            json: true,
        }
    );
    Ok(())
}

#[test]
fn parses_pasted_detail_ids_as_read_shortcuts() -> TestResult {
    let issue = parse_command(["logbrew", "issue_123", "--json"])?;
    assert_eq!(
        issue,
        Command::Read {
            target: ReadTarget::Issue("issue_123".to_owned()),
            options: Box::default(),
            json: true,
        }
    );
    assert_eq!(
        issue.http_path().ok_or("issue id has endpoint")?,
        "/api/telemetry/issues/issue_123?response_version=4"
    );

    let trace = parse_command(["logbrew", "--json", "4bf92f3577b34da6a3ce929d0e0e4736"])?;
    assert_eq!(
        trace,
        Command::Read {
            target: ReadTarget::Trace("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
            options: Box::default(),
            json: true,
        }
    );
    assert_eq!(
        trace.http_path().ok_or("trace id has endpoint")?,
        "/api/telemetry/traces/4bf92f3577b34da6a3ce929d0e0e4736"
    );
    Ok(())
}
