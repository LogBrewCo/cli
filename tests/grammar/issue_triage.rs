//! `logbrew issues archive|resolve|reopen|priority` and the issue priority read filter.

use super::{TestResult, assert_json_error, assert_path};
use logbrew_cli::{Command, HttpMethod, IssueBulkOptions, SetTarget, parse_command};

const PROJECT: &str = "123e4567-e89b-12d3-a456-426614174000";
const ISSUE_A: &str = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_B: &str = "223e4567-e89b-12d3-a456-426614174002";
const NEXT: &str = "use logbrew issues archive|resolve|reopen <issue_id>... or logbrew issues priority high|medium|low <issue_id>..., with --project <project_id> for more than one issue (up to 100) and optional --json";

fn parse(line: &str) -> TestResult<Command> {
    Ok(parse_command(line.split_whitespace())?)
}

#[test]
fn one_issue_changes_through_its_own_endpoint_in_version_2() -> TestResult {
    for (line, target, body) in [
        (
            format!("logbrew issues archive {ISSUE_A} --json"),
            SetTarget::IssueStatus {
                id: ISSUE_A.into(),
                status: "archived".into(),
            },
            serde_json::json!({"status": "archived"}),
        ),
        (
            format!("logbrew --json issues reopen {ISSUE_A}"),
            SetTarget::IssueStatus {
                id: ISSUE_A.into(),
                status: "unresolved".into(),
            },
            serde_json::json!({"status": "unresolved"}),
        ),
        (
            format!("logbrew issues priority low {ISSUE_A} --json"),
            SetTarget::IssuePriority {
                id: ISSUE_A.into(),
                priority: "low".into(),
            },
            serde_json::json!({"priority": "low"}),
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(command, Command::Set { target, json: true }, "{line}");
        assert_eq!(
            command.http_path(),
            Some(format!(
                "/api/telemetry/issues/{ISSUE_A}?response_version=2"
            )),
            "{line}"
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Patch), "{line}");
        assert_eq!(command.request_body(), Some(body), "{line}");
    }
    Ok(())
}

#[test]
fn several_issues_of_one_project_change_in_one_bulk_request() -> TestResult {
    let upper = ISSUE_B.to_ascii_uppercase();
    for (line, status, priority) in [
        (
            format!("logbrew issues resolve {ISSUE_A} {upper} --project {PROJECT}"),
            Some("resolved"),
            None,
        ),
        (
            format!("logbrew issues archive --project-id {PROJECT} {ISSUE_A} {ISSUE_B}"),
            Some("archived"),
            None,
        ),
        (
            format!("logbrew issues priority high {ISSUE_A} {ISSUE_B} --project {PROJECT}"),
            None,
            Some("high"),
        ),
    ] {
        let command = parse(&line)?;
        let options = IssueBulkOptions {
            project_id: PROJECT.into(),
            issue_ids: vec![ISSUE_A.into(), ISSUE_B.into()],
            status: status.map(str::to_owned),
            priority: priority.map(str::to_owned),
        };
        assert_eq!(
            command,
            Command::Set {
                target: SetTarget::IssueBulk(options),
                json: false
            },
            "{line}"
        );
        assert_eq!(
            command.http_path().as_deref(),
            Some("/api/telemetry/issues/bulk")
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Post));
        let mut body = serde_json::json!({"project_id": PROJECT, "issue_ids": [ISSUE_A, ISSUE_B]});
        if let Some(status) = status {
            body["status"] = status.into();
        }
        if let Some(priority) = priority {
            body["priority"] = priority.into();
        }
        assert_eq!(command.request_body(), Some(body), "{line}");
    }
    Ok(())
}

#[test]
fn issue_changes_refuse_ambiguous_or_unbounded_requests() -> TestResult {
    let too_many = (0..=100)
        .map(|index| format!("223e4567-e89b-12d3-a456-{index:012}"))
        .collect::<Vec<_>>()
        .join(" ");
    for line in [
        String::from("logbrew issues archive"),
        format!("logbrew issues archive {ISSUE_A} {ISSUE_B}"),
        format!("logbrew issues archive {ISSUE_A} {ISSUE_A} --project {PROJECT}"),
        format!("logbrew issues archive issue_123 {ISSUE_B} --project {PROJECT}"),
        format!("logbrew issues archive {ISSUE_A} --project not-a-project"),
        format!("logbrew issues archive {ISSUE_A} --project {PROJECT} --project {PROJECT}"),
        format!("logbrew issues archive {ISSUE_A} --json --json"),
        format!("logbrew issues archive {ISSUE_A} --yes"),
        format!("logbrew issues priority urgent {ISSUE_A}"),
        format!("logbrew issues priority {ISSUE_A}"),
        format!("logbrew issues resolve {too_many} --project {PROJECT}"),
    ] {
        let args = line.split_whitespace().collect::<Vec<_>>();
        assert_json_error(
            &args,
            "invalid_issue_command",
            "invalid issue command",
            NEXT,
        )?;
    }
    Ok(())
}

#[test]
fn issue_reads_filter_by_priority_and_archived_status() -> TestResult {
    assert_path(
        &[
            "logbrew",
            "issues",
            "--priority",
            "HIGH",
            "--status",
            "archived",
            "--json",
        ],
        "/api/telemetry/issues?status=archived&priority=high&response_version=2",
    );
    assert_json_error(
        &["logbrew", "issues", "--priority", "urgent", "--json"],
        "unknown_priority",
        "unknown issue priority: urgent",
        "use --priority high, medium, or low",
    )?;
    assert!(
        parse_command(["logbrew", "logs", "--priority", "high"]).is_err(),
        "issues only"
    );
    Ok(())
}
