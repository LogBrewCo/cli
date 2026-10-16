//! `logbrew issues archive|resolve|reopen|priority|assign|unassign|assignees` and the issue
//! priority and assignee read filters.

use super::{TestResult, assert_json_error, assert_path};
use logbrew_cli::{Command, HttpMethod, IssueBulkOptions, ReadTarget, SetTarget, parse_command};

const PROJECT: &str = "123e4567-e89b-12d3-a456-426614174000";
const ISSUE_A: &str = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_B: &str = "223e4567-e89b-12d3-a456-426614174002";
const USER: &str = "323e4567-e89b-12d3-a456-426614174003";
const NEXT: &str = "use logbrew issues archive|resolve|reopen|unassign <issue_id>..., logbrew issues priority high|medium|low <issue_id>..., or logbrew issues assign <user_id|me> <issue_id>..., with --project <project_id> for more than one issue (up to 100), or logbrew issues assignees --project <project_id>, each with optional --json";

fn parse(line: &str) -> TestResult<Command> {
    Ok(parse_command(line.split_whitespace())?)
}

#[test]
fn one_issue_changes_through_its_own_endpoint_in_version_3() -> TestResult {
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
        (
            format!("logbrew issues assign me {ISSUE_A} --json"),
            SetTarget::IssueAssignee {
                id: ISSUE_A.into(),
                assignee: "me".into(),
            },
            serde_json::json!({"assignee": "me"}),
        ),
        (
            format!(
                "logbrew issues assign {} {ISSUE_A} --json",
                USER.to_ascii_uppercase()
            ),
            SetTarget::IssueAssignee {
                id: ISSUE_A.into(),
                assignee: USER.into(),
            },
            serde_json::json!({"assignee": USER}),
        ),
        (
            format!("logbrew --json issues unassign {ISSUE_A}"),
            SetTarget::IssueAssignee {
                id: ISSUE_A.into(),
                assignee: "none".into(),
            },
            serde_json::json!({"assignee": "none"}),
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(command, Command::Set { target, json: true }, "{line}");
        assert_eq!(
            command.http_path(),
            Some(format!(
                "/api/telemetry/issues/{ISSUE_A}?response_version=3"
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
    for (line, field, value) in [
        (
            format!("logbrew issues resolve {ISSUE_A} {upper} --project {PROJECT}"),
            "status",
            "resolved",
        ),
        (
            format!("logbrew issues archive --project-id {PROJECT} {ISSUE_A} {ISSUE_B}"),
            "status",
            "archived",
        ),
        (
            format!("logbrew issues priority high {ISSUE_A} {ISSUE_B} --project {PROJECT}"),
            "priority",
            "high",
        ),
        (
            format!("logbrew issues assign {USER} {ISSUE_A} {ISSUE_B} --project {PROJECT}"),
            "assignee",
            USER,
        ),
        (
            format!("logbrew issues assign me {ISSUE_A} {ISSUE_B} --project {PROJECT}"),
            "assignee",
            "me",
        ),
        (
            format!("logbrew issues unassign {ISSUE_A} {upper} --project {PROJECT}"),
            "assignee",
            "none",
        ),
    ] {
        let command = parse(&line)?;
        let options = IssueBulkOptions {
            project_id: PROJECT.into(),
            issue_ids: vec![ISSUE_A.into(), ISSUE_B.into()],
            status: (field == "status").then(|| value.to_owned()),
            priority: (field == "priority").then(|| value.to_owned()),
            assignee: (field == "assignee").then(|| value.to_owned()),
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
        body[field] = value.into();
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
        String::from("logbrew issues assign"),
        format!("logbrew issues assign {ISSUE_A}"),
        format!("logbrew issues assign not-a-user {ISSUE_A}"),
        format!("logbrew issues assign none {ISSUE_A}"),
        format!("logbrew issues assign me {ISSUE_A} {ISSUE_B}"),
        format!("logbrew issues assign me {ISSUE_A} {ISSUE_A} --project {PROJECT}"),
        format!("logbrew issues assign me {too_many} --project {PROJECT}"),
        format!("logbrew issues assign me {ISSUE_A} --yes"),
        format!("logbrew issues assign me {ISSUE_A} --json --json"),
        String::from("logbrew issues unassign"),
        format!("logbrew issues unassign {ISSUE_A} {ISSUE_B}"),
        format!("logbrew issues unassign {ISSUE_A} {ISSUE_A} --project {PROJECT}"),
        format!("logbrew issues unassign {too_many} --project {PROJECT}"),
        format!("logbrew issues unassign {ISSUE_A} --yes"),
        format!("logbrew issues unassign {ISSUE_A} --json --json"),
        String::from("logbrew issues assignees"),
        String::from("logbrew issues assignees --project not-a-project"),
        format!("logbrew issues assignees {PROJECT}"),
        format!("logbrew issues assignees --project {PROJECT} --yes"),
        format!("logbrew issues assignees --project {PROJECT} --json --json"),
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
        "/api/telemetry/issues?status=archived&priority=high&response_version=3",
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

#[test]
fn assignees_list_everyone_who_can_be_assigned_in_a_project() -> TestResult {
    for (line, json) in [
        (
            format!("logbrew issues assignees --project {PROJECT}"),
            false,
        ),
        (
            format!(
                "logbrew --json issues assignees --project-id {}",
                PROJECT.to_ascii_uppercase()
            ),
            true,
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(
            command,
            Command::Read {
                target: ReadTarget::IssueAssignees(PROJECT.into()),
                options: Box::default(),
                json,
            },
            "{line}"
        );
        assert_eq!(
            command.http_path(),
            Some(format!(
                "/api/telemetry/issues/assignees?project_id={PROJECT}"
            ))
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Get));
        assert_eq!(command.request_body(), None);
    }
    Ok(())
}

#[test]
fn issue_reads_filter_by_assignee() -> TestResult {
    let upper = USER.to_ascii_uppercase();
    for (args, query) in [
        (vec!["logbrew", "issues", "--assignee", "me"], "assignee=me"),
        (
            vec!["logbrew", "read", "issues", "--assignee", "NONE"],
            "assignee=none",
        ),
        (
            vec!["logbrew", "issues", "--assignee", upper.as_str()],
            &format!("assignee={USER}"),
        ),
        (
            vec![
                "logbrew",
                "issues",
                "open",
                "--assignee",
                "me",
                "--priority",
                "high",
            ],
            "status=unresolved&priority=high&assignee=me",
        ),
    ] {
        assert_path(
            &args,
            &format!("/api/telemetry/issues?{query}&response_version=3"),
        );
    }
    let next = "use --assignee me, none, or a user ID";
    for (value, echoed) in [("someone", "someone"), ("ME-TOO", "me-too")] {
        assert_json_error(
            &["logbrew", "issues", "--assignee", value, "--json"],
            "invalid_assignee",
            &format!("invalid issue assignee: {echoed}"),
            next,
        )?;
    }
    assert_json_error(
        &[
            "logbrew",
            "issues",
            "--assignee",
            "me",
            "--assignee",
            "none",
            "--json",
        ],
        "duplicate_flag",
        "duplicate flag: --assignee",
        "use --assignee once",
    )?;
    assert!(
        parse_command(["logbrew", "logs", "--assignee", "me"]).is_err(),
        "issues only"
    );
    Ok(())
}
