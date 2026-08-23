//! Issue mutation shortcut tests.

use logbrew_cli::{Command, HelpTopic, ReadOptions, ReadTarget, SetTarget, help, parse_command};

#[test]
fn set_help_advertises_issue_mutation_aliases() {
    let text = help::help_text(HelpTopic::Set);

    assert!(text.contains("logbrew issue <issue_id> resolve [--json]"));
    assert!(text.contains("logbrew issue <issue_id> close [--json]"));
    assert!(text.contains("logbrew <issue_id> resolve [--json]"));
    assert!(text.contains("logbrew resolved <issue_id> [--json]"));
    assert!(text.contains("logbrew closed <issue_id> [--json]"));
    assert!(text.contains("logbrew ignored <issue_id> [--json]"));
    assert!(text.contains("logbrew open <issue_id> [--json]"));
    assert!(text.contains("logbrew unresolved <issue_id> [--json]"));
    assert!(text.contains("Close is an alias for resolved."));
    assert!(text.contains(
        "Issue-first, pasted-ID, and status-first aliases are useful after reading issue detail."
    ));
}

#[test]
fn parses_status_first_issue_ids_as_mutations() {
    for (args, expected_status) in [
        (
            &["logbrew", "resolved", "issue_123", "--json"][..],
            "resolved",
        ),
        (&["logbrew", "closed", "--json", "issue_123"], "resolved"),
        (&["logbrew", "ignored", "issue-123", "--json"], "ignored"),
        (&["logbrew", "open", "issue_123", "--json"], "unresolved"),
    ] {
        let command = parse_command(args.iter().copied()).expect("status-first mutation parses");
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
            command
                .http_path()
                .expect("status-first mutation has endpoint"),
            format!("/api/telemetry/issues/{expected_id}")
        );
        assert_eq!(
            command
                .request_body()
                .expect("status-first mutation has body"),
            serde_json::json!({ "status": expected_status })
        );
    }
}

#[test]
fn keeps_status_first_issue_collections_as_reads() {
    let command =
        parse_command(["logbrew", "open", "issues", "--json"]).expect("issue list parses");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Issues,
            options: Box::new(ReadOptions {
                status: Some("unresolved".to_owned()),
                ..ReadOptions::default()
            }),
            json: true,
        }
    );
}
