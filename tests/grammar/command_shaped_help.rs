//! CLI command-shaped help recovery tests.

use logbrew_cli::{Command, HelpTopic, parse_command};

#[test]
fn parses_command_shaped_help_after_detail_and_mutation_ids()
-> Result<(), Box<dyn std::error::Error>> {
    for (topic, cases) in [
        (
            HelpTopic::ReadIssue,
            &[
                &["issue", "issue_123", "--help"][..],
                &["issue", "issue_123", "help"][..],
                &["read", "issue", "issue_123", "--help"][..],
                &["read", "errors", "issue_123", "--help"][..],
                &["read", "issue", "issue_123", "help"][..],
                &["get", "issue", "issue_123", "--help"][..],
                &["show", "errors", "issue_123", "--help"][..],
                &["get", "exceptions", "issue_123", "help"][..],
                &["list", "errors", "issue_123", "--help"][..],
                &["list", "issue", "issue_123", "--help"][..],
                &["errors", "issue_123", "--help"][..],
                &["exceptions", "issue_123", "help"][..],
            ][..],
        ),
        (
            HelpTopic::ReadTrace,
            &[
                &["trace", "trace_123", "--help"][..],
                &["trace", "trace_123", "help"][..],
                &["read", "trace", "trace_123", "--help"][..],
                &["show", "traces", "trace_123", "--help"][..],
                &["get", "spans", "trace_123", "help"][..],
                &["list", "traces", "trace_123", "--help"][..],
            ][..],
        ),
        (
            HelpTopic::Explain,
            &[
                &["explain", "issue", "issue_123", "--help"][..],
                &["explain", "trace", "trace_123", "--help"][..],
                &["explain", "trace", "trace_123", "help"][..],
                &["explain", "issue_123", "--help"][..],
                &["issue", "issue_123", "explain", "--help"][..],
                &["trace", "trace_123", "explain", "--help"][..],
                &["issue_123", "explain", "--help"][..],
                &["trace_123", "explain", "help"][..],
            ][..],
        ),
        (
            HelpTopic::Set,
            &[
                &["set", "issue", "issue_123", "resolved", "--help"][..],
                &["set", "issue", "issue_123", "resolved", "help"][..],
                &["resolve", "issue_123", "--help"][..],
                &["resolve", "issue_123", "help"][..],
                &["issue", "issue_123", "resolve", "--help"][..],
                &["issue", "issue_123", "resolve", "help"][..],
                &["issue", "issue_123", "resolved", "--help"][..],
                &["issue", "issue_123", "open", "help"][..],
                &["issue_123", "resolve", "--help"][..],
                &["issue_123", "close", "help"][..],
                &["issue_123", "closed", "--help"][..],
                &["issue_123", "ignored", "help"][..],
                &["closed", "issue_123", "--help"][..],
                &["open", "issue_123", "help"][..],
                &["resolved", "--help"][..],
                &["closed", "--help"][..],
                &["ignored", "help"][..],
                &["open", "help"][..],
                &["unresolved", "--help"][..],
            ][..],
        ),
    ] {
        for args in cases {
            let command = parse_command(std::iter::once("logbrew").chain(args.iter().copied()))?;
            assert_eq!(command, Command::Help { topic, json: false });
        }
    }
    Ok(())
}

#[test]
fn parses_json_help_for_status_and_detail_targets() -> Result<(), Box<dyn std::error::Error>> {
    for (args, topic) in [
        (
            &["logbrew", "--json", "issue", "issue_123", "--help"][..],
            HelpTopic::ReadIssue,
        ),
        (
            &[
                "logbrew",
                "help",
                "issue",
                "issue_123",
                "resolved",
                "--json",
            ],
            HelpTopic::Set,
        ),
        (
            &["logbrew", "help", "issue_123", "ignored", "--json"],
            HelpTopic::Set,
        ),
        (
            &["logbrew", "help", "closed", "issue_123", "--json"],
            HelpTopic::Set,
        ),
    ] {
        assert_eq!(parse_command(args)?, Command::Help { topic, json: true });
    }
    Ok(())
}

#[test]
fn parses_issue_status_shortcut_help_as_real_user_topics() -> Result<(), Box<dyn std::error::Error>>
{
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
        let command = parse_command(args)?;

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadIssues,
                json: false
            }
        );
    }
    Ok(())
}

#[test]
fn parses_read_prefixed_shortcut_help_as_real_user_topics() -> Result<(), Box<dyn std::error::Error>>
{
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
        let command = parse_command(args)?;

        assert_eq!(command, Command::Help { topic, json: false });
    }
    Ok(())
}

#[test]
fn parses_log_shortcut_help_as_real_user_topics() -> Result<(), Box<dyn std::error::Error>> {
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
        let command = parse_command(args)?;

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadLogs,
                json: false
            }
        );
    }
    Ok(())
}

#[test]
fn parses_explicit_help_for_detail_read_aliases() -> Result<(), Box<dyn std::error::Error>> {
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
        let command = parse_command(args)?;

        assert_eq!(command, Command::Help { topic, json: true });
    }
    Ok(())
}
