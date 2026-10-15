//! Issue mutation shortcut tests.

use super::{TestResult, assert_command};
use logbrew_cli::{Command, HelpTopic, ReadOptions, ReadTarget, SetTarget, help, parse_command};

#[test]
fn set_help_advertises_issue_mutation_aliases() {
    let text = help::help_text(HelpTopic::Set);

    for fragment in [
        "logbrew issue <issue_id> resolve [--json]",
        "logbrew issue <issue_id> close [--json]",
        "logbrew <issue_id> resolve [--json]",
        "logbrew resolved <issue_id> [--json]",
        "logbrew closed <issue_id> [--json]",
        "logbrew ignored <issue_id> [--json]",
        "logbrew open <issue_id> [--json]",
        "logbrew unresolved <issue_id> [--json]",
        "Close is an alias for resolved.",
        "Issue-first, pasted-ID, and status-first aliases are useful after reading issue detail.",
    ] {
        assert!(
            text.contains(fragment),
            "missing set-help fragment: {fragment}"
        );
    }
}

#[test]
fn parses_status_first_issue_ids_as_mutations() -> TestResult {
    for (args, expected_status) in [
        (
            &["logbrew", "resolved", "issue_123", "--json"][..],
            "resolved",
        ),
        (&["logbrew", "closed", "--json", "issue_123"], "resolved"),
        (&["logbrew", "ignored", "issue-123", "--json"], "ignored"),
        (&["logbrew", "open", "issue_123", "--json"], "unresolved"),
    ] {
        let command = parse_command(args.iter().copied())?;
        let expected_id = if args.contains(&"issue-123") {
            "issue-123"
        } else {
            "issue_123"
        };

        assert_eq!(
            command,
            Command::Set {
                target: SetTarget::IssueStatus {
                    id: expected_id.to_owned(),
                    status: expected_status.to_owned(),
                },
                json: true,
            }
        );
        assert_eq!(
            command.http_path(),
            Some(format!(
                "/api/telemetry/issues/{expected_id}?response_version=2"
            ))
        );
        assert_eq!(
            command.request_body(),
            Some(serde_json::json!({ "status": expected_status }))
        );
    }
    Ok(())
}

#[test]
fn keeps_status_first_issue_collections_as_reads() {
    assert_command(
        &["logbrew", "open", "issues", "--json"],
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::new(ReadOptions {
                status: Some("unresolved".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        },
    );
}
