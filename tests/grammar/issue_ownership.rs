//! `logbrew issues owners|activity|ownership|rule` and `logbrew logs rule`.

use super::{TestResult, assert_json_error};
use logbrew_cli::{Command, HttpMethod, ReadOptions, ReadTarget, SetTarget, parse_command};

const PROJECT: &str = "123e4567-e89b-12d3-a456-426614174000";
const ISSUE: &str = "223e4567-e89b-12d3-a456-426614174001";
const LOG: &str = "423e4567-e89b-12d3-a456-426614174004";
const OWNERSHIP_NEXT: &str = "use logbrew issues owners <issue_id>, logbrew issues activity <issue_id> [--limit <1-200>] [--cursor-time <time> --cursor-id <id>], logbrew issues ownership --project <project_id>, or logbrew issues ownership set --project <project_id> [--rules <file>] [--codeowners <file>], each with optional --json";
const RULE_NEXT: &str = "use logbrew issues rule <issue_id> <severity> or logbrew logs rule <log_id> <severity> [--ignore], where severity is source, info, warning, error, or critical, with optional --json";

fn parse(line: &str) -> TestResult<Command> {
    Ok(parse_command(line.split_whitespace())?)
}

/// Writes `text` to a new file and returns its path.
fn file(name: &str, text: &[u8]) -> TestResult<String> {
    let path =
        std::env::temp_dir().join(format!("logbrew-ownership-{name}-{}", std::process::id()));
    std::fs::write(&path, text)?;
    Ok(path.to_string_lossy().into_owned())
}

#[test]
fn owner_activity_and_ownership_reads_use_their_endpoints() -> TestResult {
    let upper = ISSUE.to_ascii_uppercase();
    for (line, target, path, json) in [
        (
            format!("logbrew issues owners {upper}"),
            ReadTarget::IssueOwners(ISSUE.into()),
            format!("/api/telemetry/issues/{ISSUE}/owners?response_version=2"),
            false,
        ),
        (
            format!("logbrew --json issues activity {ISSUE}"),
            ReadTarget::IssueActivity(ISSUE.into()),
            format!("/api/telemetry/issues/{ISSUE}/activity?response_version=2"),
            true,
        ),
        (
            format!("logbrew issues ownership --project {PROJECT} --json"),
            ReadTarget::IssueOwnership(PROJECT.into()),
            format!("/api/telemetry/issues/ownership?project_id={PROJECT}"),
            true,
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(
            command,
            Command::Read {
                target,
                options: Box::default(),
                json
            },
            "{line}"
        );
        assert_eq!(command.http_path(), Some(path), "{line}");
        assert_eq!(command.http_method(), Some(HttpMethod::Get), "{line}");
        assert_eq!(command.request_body(), None, "{line}");
    }
    let command = parse(&format!(
        "logbrew issues activity {ISSUE} --limit 200 --cursor-time 2026-10-19T01:02:03.004Z --cursor-id {ISSUE}"
    ))?;
    let options = ReadOptions {
        limit: Some("200".into()),
        cursor_time: Some("2026-10-19T01:02:03.004Z".into()),
        cursor_id: Some(ISSUE.into()),
        ..ReadOptions::default()
    };
    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::IssueActivity(ISSUE.into()),
            options: Box::new(options),
            json: false
        }
    );
    assert_eq!(
        command.http_path(),
        Some(format!(
            "/api/telemetry/issues/{ISSUE}/activity?limit=200&cursor_time=2026-10-19T01%3A02%3A03.004Z&cursor_id={ISSUE}&response_version=2"
        ))
    );
    Ok(())
}

#[test]
fn ownership_set_sends_the_named_files() -> TestResult {
    let rules = file("rules", b"path:src/payments/** ada@example.com\n")?;
    let codeowners = file("codeowners", b"* @org/web\n")?;
    let empty = file("empty", b"")?;
    for (flags, body) in [
        (
            format!("--rules {rules}"),
            serde_json::json!({"project_id": PROJECT, "rules": "path:src/payments/** ada@example.com\n"}),
        ),
        (
            format!("--codeowners {codeowners} --rules {empty}"),
            serde_json::json!({"project_id": PROJECT, "rules": "", "codeowners": "* @org/web\n"}),
        ),
    ] {
        let line = format!("logbrew issues ownership set --project {PROJECT} {flags} --json");
        let command = parse(&line)?;
        assert!(
            matches!(
                &command,
                Command::Set {
                    target: SetTarget::IssueOwnership { .. },
                    json: true
                }
            ),
            "{line}"
        );
        assert_eq!(
            command.http_path().as_deref(),
            Some("/api/telemetry/issues/ownership")
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Put), "{line}");
        assert_eq!(command.request_body(), Some(body), "{line}");
    }
    for path in [rules, codeowners, empty] {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[test]
fn severity_rules_put_one_complete_rule() -> TestResult {
    for (line, path, body) in [
        (
            format!("logbrew issues rule {ISSUE} warning"),
            format!("/api/telemetry/issues/{ISSUE}/triage-rule"),
            serde_json::json!({"severity": "warning"}),
        ),
        (
            format!("logbrew issues rule {ISSUE} source --json"),
            format!("/api/telemetry/issues/{ISSUE}/triage-rule"),
            serde_json::json!({"severity": "source"}),
        ),
        (
            format!("logbrew logs rule {LOG} info --ignore"),
            format!("/api/logs/{LOG}/triage-rule"),
            serde_json::json!({"severity": "info", "ignored": true}),
        ),
        (
            format!("logbrew --json logs rule {LOG} critical"),
            format!("/api/logs/{LOG}/triage-rule"),
            serde_json::json!({"severity": "critical", "ignored": false}),
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(command.http_path(), Some(path), "{line}");
        assert_eq!(command.http_method(), Some(HttpMethod::Put), "{line}");
        assert_eq!(command.request_body(), Some(body), "{line}");
    }
    assert!(
        matches!(parse("logbrew logs rule checkout")?, Command::Read { .. }),
        "a log search for the word rule stays a search"
    );
    Ok(())
}

#[test]
fn malformed_commands_fail_before_any_request() -> TestResult {
    let missing = std::env::temp_dir()
        .join("logbrew-ownership-missing-file")
        .to_string_lossy()
        .into_owned();
    let large = file("large", &vec![b'#'; 128 * 1024 + 1])?;
    let invalid = file("binary", &[0xff, 0xfe])?;
    let valid = file("valid", b"* @org/web\n")?;
    for line in [
        "logbrew issues owners".to_owned(),
        "logbrew issues owners not-an-issue".to_owned(),
        format!("logbrew issues owners {ISSUE} {ISSUE}"),
        format!("logbrew issues activity {ISSUE} --limit 0"),
        format!("logbrew issues activity {ISSUE} --limit 201"),
        format!("logbrew issues activity {ISSUE} --cursor-id {ISSUE}"),
        format!("logbrew issues activity {ISSUE} --cursor-time yesterday --cursor-id {ISSUE}"),
        format!("logbrew issues activity {ISSUE} --limit 5 --limit 6"),
        format!("logbrew issues activity {ISSUE} --since 1h"),
        "logbrew issues ownership".to_owned(),
        format!("logbrew issues ownership --project {PROJECT} --json --json"),
        format!("logbrew issues ownership set --project {PROJECT}"),
        format!("logbrew issues ownership set --rules {valid}"),
        format!(
            "logbrew issues ownership set --project {PROJECT} --project {PROJECT} --rules {valid}"
        ),
    ] {
        assert_json_error(
            &line.split_whitespace().collect::<Vec<_>>(),
            "invalid_ownership_command",
            "invalid ownership command",
            OWNERSHIP_NEXT,
        )?;
    }
    for path in [&missing, &large, &invalid] {
        let line = format!("logbrew issues ownership set --project {PROJECT} --rules {path}");
        assert_json_error(
            &line.split_whitespace().collect::<Vec<_>>(),
            "invalid_ownership_file",
            "invalid ownership file",
            "pass a readable UTF-8 text file of at most 128 KiB to --rules or --codeowners",
        )?;
    }
    for line in [
        format!("logbrew issues rule {ISSUE}"),
        format!("logbrew issues rule {ISSUE} fatal"),
        format!("logbrew issues rule {ISSUE} warning --ignore"),
        format!("logbrew logs rule {LOG}"),
        format!("logbrew logs rule {LOG} WARNING"),
        format!("logbrew logs rule {LOG} info --ignore --ignore"),
        format!("logbrew logs rule {LOG} info extra"),
    ] {
        assert_json_error(
            &line.split_whitespace().collect::<Vec<_>>(),
            "invalid_severity_rule_command",
            "invalid severity rule command",
            RULE_NEXT,
        )?;
    }
    for path in [large, invalid, valid] {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[test]
fn teams_are_assignees_and_filters_and_the_inbox_reads_assignments() -> TestResult {
    const TEAM: &str = "523e4567-e89b-12d3-a456-426614174005";
    let command = parse(&format!("logbrew issues assign team:{TEAM} {ISSUE}"))?;
    assert_eq!(
        command.request_body(),
        Some(serde_json::json!({"assignee": format!("team:{TEAM}")}))
    );
    for filter in ["my_teams".to_owned(), format!("team:{TEAM}")] {
        let command = parse(&format!("logbrew issues --assignee {filter}"))?;
        assert!(
            command.http_path().is_some_and(
                |path| path.contains(&format!("assignee={}", filter.replace(':', "%3A")))
            ),
            "{filter}"
        );
    }
    for (line, path, method, body) in [
        (
            "logbrew inbox",
            "/api/telemetry/issues/notifications",
            HttpMethod::Get,
            None,
        ),
        (
            "logbrew inbox --limit 5 --json",
            "/api/telemetry/issues/notifications?limit=5",
            HttpMethod::Get,
            None,
        ),
        (
            "logbrew inbox read",
            "/api/telemetry/issues/notifications/read",
            HttpMethod::Post,
            Some(serde_json::json!({})),
        ),
    ] {
        let command = parse(line)?;
        assert_eq!(command.http_path().as_deref(), Some(path), "{line}");
        assert_eq!(command.http_method(), Some(method), "{line}");
        assert_eq!(command.request_body(), body, "{line}");
    }
    for line in [
        "logbrew inbox read extra",
        "logbrew inbox --limit 0",
        "logbrew inbox --cursor-id x",
    ] {
        assert_json_error(
            &line
                .split_whitespace()
                .chain(["--json"])
                .collect::<Vec<_>>(),
            "invalid_inbox_command",
            "invalid inbox command",
            "use logbrew inbox [--limit <1-200>] [--cursor-time <time> --cursor-id <id>] or logbrew inbox read, each with optional --json",
        )?;
    }
    Ok(())
}
