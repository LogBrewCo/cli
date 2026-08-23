//! CLI command-shaped help recovery tests.

use logbrew_cli::{Command, HelpTopic, parse_command};

#[test]
fn parses_command_shaped_help_after_detail_and_mutation_ids() {
    let cases = [
        (
            vec!["logbrew", "issue", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "issue", "issue_123", "help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "read", "issue", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "read", "errors", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "read", "issue", "issue_123", "help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "get", "issue", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "show", "errors", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "get", "exceptions", "issue_123", "help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "list", "errors", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "list", "issue", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "errors", "issue_123", "--help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "exceptions", "issue_123", "help"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "trace", "trace_123", "--help"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "trace", "trace_123", "help"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "read", "trace", "trace_123", "--help"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "show", "traces", "trace_123", "--help"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "get", "spans", "trace_123", "help"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "list", "traces", "trace_123", "--help"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "explain", "issue", "issue_123", "--help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "explain", "trace", "trace_123", "--help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "explain", "trace", "trace_123", "help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "explain", "issue_123", "--help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "issue", "issue_123", "explain", "--help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "trace", "trace_123", "explain", "--help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "issue_123", "explain", "--help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "trace_123", "explain", "help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "set", "issue", "issue_123", "resolved", "--help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "set", "issue", "issue_123", "resolved", "help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "resolve", "issue_123", "--help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "resolve", "issue_123", "help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue", "issue_123", "resolve", "--help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue", "issue_123", "resolve", "help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue", "issue_123", "resolved", "--help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue", "issue_123", "open", "help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue_123", "resolve", "--help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue_123", "close", "help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue_123", "closed", "--help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "issue_123", "ignored", "help"],
            HelpTopic::Set,
        ),
        (
            vec!["logbrew", "closed", "issue_123", "--help"],
            HelpTopic::Set,
        ),
        (vec!["logbrew", "open", "issue_123", "help"], HelpTopic::Set),
        (vec!["logbrew", "resolved", "--help"], HelpTopic::Set),
        (vec!["logbrew", "closed", "--help"], HelpTopic::Set),
        (vec!["logbrew", "ignored", "help"], HelpTopic::Set),
        (vec!["logbrew", "open", "help"], HelpTopic::Set),
        (vec!["logbrew", "unresolved", "--help"], HelpTopic::Set),
    ];

    for (args, topic) in cases {
        let command = parse_command(args).expect("command-shaped help parses");

        assert_eq!(command, Command::Help { topic, json: false });
    }

    assert_eq!(
        parse_command(["logbrew", "--json", "issue", "issue_123", "--help"])
            .expect("global json command-shaped help parses"),
        Command::Help {
            topic: HelpTopic::ReadIssue,
            json: true
        }
    );
    assert_eq!(
        parse_command([
            "logbrew",
            "help",
            "issue",
            "issue_123",
            "resolved",
            "--json"
        ])
        .expect("explicit status-word issue help parses"),
        Command::Help {
            topic: HelpTopic::Set,
            json: true
        }
    );
    assert_eq!(
        parse_command(["logbrew", "help", "issue_123", "ignored", "--json"])
            .expect("explicit pasted status-word issue help parses"),
        Command::Help {
            topic: HelpTopic::Set,
            json: true
        }
    );
    assert_eq!(
        parse_command(["logbrew", "help", "closed", "issue_123", "--json"])
            .expect("explicit status-first issue help parses"),
        Command::Help {
            topic: HelpTopic::Set,
            json: true
        }
    );
}

#[test]
fn parses_issue_status_shortcut_help_as_real_user_topics() {
    for args in [
        &["logbrew", "issues", "open", "--help"][..],
        &["logbrew", "open", "issues", "--help"],
        &["logbrew", "help", "open", "issues"],
        &["logbrew", "open", "help", "issues"],
        &["logbrew", "open", "issue", "--help"],
        &["logbrew", "help", "open", "issue"],
        &["logbrew", "open", "help", "issue"],
        &["logbrew", "errors", "closed", "--help"],
        &["logbrew", "closed", "errors", "--help"],
        &["logbrew", "closed", "issue", "--help"],
        &["logbrew", "ignored", "exceptions", "--help"],
        &["logbrew", "issue", "open", "--help"],
        &["logbrew", "help", "issue", "open"],
        &["logbrew", "issue", "help", "open"],
        &["logbrew", "read", "issue", "closed", "--help"],
        &["logbrew", "help", "read", "issue", "closed"],
        &["logbrew", "read", "open", "issue", "--help"],
        &["logbrew", "help", "read", "open", "issue"],
        &["logbrew", "last", "closed", "issue", "--help"],
        &["logbrew", "help", "last", "5", "closed", "issue"],
        &["logbrew", "show", "issue", "ignored", "--help"],
    ] {
        let command = parse_command(args).expect("issue status shortcut help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadIssues,
                json: false
            }
        );
    }
}

#[test]
fn parses_read_prefixed_shortcut_help_as_real_user_topics() {
    for (args, topic) in [
        (
            &["logbrew", "read", "log", "--help"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "help", "read", "release"],
            HelpTopic::ReadReleases,
        ),
        (
            &["logbrew", "last", "10", "logs", "--help"],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "help", "read", "recent", "5", "issues"],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "latest", "3", "events", "--help"],
            HelpTopic::ReadActions,
        ),
        (
            &["logbrew", "read", "latest", "logs", "--help"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "help", "read", "latest", "logs"],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "read", "last", "actions", "--help"],
            HelpTopic::ReadActions,
        ),
        (
            &["logbrew", "help", "read", "newest", "releases"],
            HelpTopic::ReadReleases,
        ),
        (
            &["logbrew", "read", "open", "issues", "--help"],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "help", "read", "open", "issues"],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "read", "closed", "errors", "--help"],
            HelpTopic::ReadIssues,
        ),
    ] {
        let command = parse_command(args).expect("read-prefixed shortcut help parses");

        assert_eq!(command, Command::Help { topic, json: false });
    }
}

#[test]
fn parses_log_shortcut_help_as_real_user_topics() {
    for args in [
        &["logbrew", "logs", "error", "--help"][..],
        &["logbrew", "logs", "warning", "checkout", "failed", "--help"],
        &["logbrew", "logs", "checkout", "failed", "help"],
        &[
            "logbrew", "read", "logs", "error", "checkout", "failed", "--help",
        ],
        &["logbrew", "help", "logs", "error", "checkout", "failed"],
        &[
            "logbrew", "help", "read", "logs", "warning", "checkout", "failed",
        ],
    ] {
        let command = parse_command(args).expect("log shortcut help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadLogs,
                json: false
            }
        );
    }
}

#[test]
fn parses_explicit_help_for_detail_read_aliases() {
    let cases = [
        (
            vec!["logbrew", "help", "read", "errors", "issue_123", "--json"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "help", "show", "errors", "issue_123", "--json"],
            HelpTopic::ReadIssue,
        ),
        (
            vec![
                "logbrew",
                "help",
                "get",
                "exceptions",
                "issue_123",
                "--json",
            ],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "help", "list", "issue", "issue_123", "--json"],
            HelpTopic::ReadIssue,
        ),
        (
            vec!["logbrew", "help", "read", "trace", "trace_123", "--json"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "help", "trace", "trace_123", "--json"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "help", "show", "traces", "trace_123", "--json"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "help", "list", "spans", "trace_123", "--json"],
            HelpTopic::ReadTrace,
        ),
        (
            vec!["logbrew", "help", "trace", "trace_123", "explain", "--json"],
            HelpTopic::Explain,
        ),
        (
            vec![
                "logbrew",
                "help",
                "traces",
                "trace_123",
                "explain",
                "--json",
            ],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "help", "explain", "issue_123", "--json"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "help", "explain", "trace", "trace_123", "--json"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "help", "issue_123", "explain", "--json"],
            HelpTopic::Explain,
        ),
    ];

    for (args, topic) in cases {
        let command = parse_command(args).expect("explicit detail help parses");

        assert_eq!(command, Command::Help { topic, json: true });
    }
}
