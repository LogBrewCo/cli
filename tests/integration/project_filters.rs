//! Project inbound filter contract tests.

use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command, rendered_json_error};
use logbrew_cli::{
    Command, HttpMethod, ProjectFiltersUpdate, RuntimeError, parse_command, write_runtime_error,
};

const PROJECT: &str = "223e4567-e89b-12d3-a456-426614174000";

#[test]
fn filters_grammar_is_introspectable() -> TestResult {
    let read = parse(&["logbrew", "projects", "filters", &PROJECT.to_uppercase()])?;
    assert_eq!(
        read,
        Command::ProjectFilters {
            project_id: PROJECT.into(),
            update: None,
            json: false
        }
    );
    assert_eq!(
        read.http_path(),
        Some(format!("/api/projects/{PROJECT}/inbound-filters"))
    );
    assert_eq!(read.http_method(), Some(HttpMethod::Get));
    assert_eq!(read.request_body(), None);

    let change = parse(&[
        "logbrew",
        "projects",
        "--json",
        "filters",
        PROJECT,
        "--message",
        "*ResizeObserver loop*",
        "--message=Load failed",
        "--no-releases",
        "--environment",
        "staging",
        "--bots",
        "on",
        "--localhost=off",
    ])?;
    assert_eq!(
        change,
        Command::ProjectFilters {
            project_id: PROJECT.into(),
            update: Some(ProjectFiltersUpdate {
                messages: Some(vec!["*ResizeObserver loop*".into(), "Load failed".into()]),
                releases: Some(Vec::new()),
                environments: Some(vec!["staging".into()]),
                localhost: Some(false),
                browser_extensions: None,
                bots: Some(true),
            }),
            json: true
        }
    );
    assert_eq!(change.http_method(), Some(HttpMethod::Put));
    assert_eq!(
        change.request_body(),
        Some(serde_json::json!({
            "messages": ["*ResizeObserver loop*", "Load failed"], "releases": [],
            "environments": ["staging"], "localhost": false, "bots": true
        }))
    );
    Ok(())
}

#[test]
fn filters_grammar_is_closed() -> TestResult {
    let long = "x".repeat(201);
    let mut many = vec!["logbrew", "projects", "filters", PROJECT];
    for _ in 0..51 {
        many.extend(["--release", "v1"]);
    }
    for line in [
        vec!["logbrew", "projects", "filters"],
        vec!["logbrew", "projects", "filters", "hostile-secret"],
        vec!["logbrew", "projects", "filters", PROJECT, "hostile-extra"],
        vec!["logbrew", "projects", "filters", PROJECT, "--message"],
        vec!["logbrew", "projects", "filters", PROJECT, "--message", "  "],
        vec![
            "logbrew",
            "projects",
            "filters",
            PROJECT,
            "--message",
            long.as_str(),
        ],
        vec![
            "logbrew", "projects", "filters", PROJECT, "--bots", "hostile",
        ],
        vec![
            "logbrew", "projects", "filters", PROJECT, "--bots", "on", "--bots", "off",
        ],
        vec![
            "logbrew",
            "projects",
            "filters",
            PROJECT,
            "--no-messages",
            "--message",
            "a",
        ],
        vec![
            "logbrew",
            "projects",
            "filters",
            PROJECT,
            "--message",
            "a",
            "--no-messages",
        ],
        vec![
            "logbrew",
            "projects",
            "filters",
            PROJECT,
            "--no-messages=hostile",
        ],
        vec!["logbrew", "projects", "filters", PROJECT, "--hostile"],
        many,
    ] {
        let (body, text) = rendered_json_error(&line)?;
        assert_eq!(body["error"], "invalid_projects_command", "{line:?}");
        assert!(!text.contains("hostile"), "{line:?} echoed its input");
    }
    Ok(())
}

async_test!(filters_read_shows_each_filter_and_dropped_counts -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("GET", format!("/api/projects/{PROJECT}/inbound-filters"), "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&filters()))
        .expect(2)
        .mount(&server);
    assert_eq!(
        run(&server, &["logbrew", "projects", "filters", PROJECT]).await?,
        format!(
            "Inbound filters for {PROJECT}\nMessages: *ResizeObserver loop*\nReleases: none\n\
             Environments: staging, dev*\nLocal machines: on\nBrowser extensions: off\nBots: on\n\
             Dropped this billing period: 12\n\
             - reason=message meter=issue_events events=12 last=2026-10-29T10:00:00Z\n\
             Next: run logbrew projects filters {PROJECT} --bots on (or --message <pattern>) to \
             change a filter\n"
        )
    );
    let read: serde_json::Value = serde_json::from_str(
        &run(&server, &["logbrew", "projects", "filters", PROJECT, "--json"]).await?,
    )?;
    assert_eq!(read, filters());
    Ok(())
});

async_test!(filters_change_sends_only_the_given_fields -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("PUT", format!("/api/projects/{PROJECT}/inbound-filters"), "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&filters()))
        .expect(1)
        .mount(&server);
    let line = ["logbrew", "projects", "filters", PROJECT, "--bots", "on", "--json"];
    let changed: serde_json::Value = serde_json::from_str(&run(&server, &line).await?)?;
    assert_eq!(changed["bots"], true);
    let requests = server.received_requests();
    let sent: serde_json::Value =
        serde_json::from_slice(&requests.first().ok_or("no request")?.body)?;
    assert_eq!(sent, serde_json::json!({"bots": true}));
    Ok(())
});

async_test!(filters_reject_invalid_responses_and_typed_errors_never_echo -> TestResult, {
    let line = ["logbrew", "projects", "filters", PROJECT];
    let mut extra = filters();
    extra["token"] = "hostile-secret".into();
    let mut other = filters();
    other["project_id"] = "123e4567-e89b-12d3-a456-426614174000".into();
    let mut unsafe_pattern = filters();
    unsafe_pattern["messages"] = serde_json::json!(["hostile\u{202e}"]);
    for body in [extra, other, unsafe_pattern, serde_json::json!([])] {
        let server = MockServer::start().await?;
        Mock::route("GET", format!("/api/projects/{PROJECT}/inbound-filters"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, &line).await?)?;
        assert!(text.contains("project response was invalid"), "{text}");
        assert!(!text.contains("hostile"));
    }
    for (status, code, action, target, expected) in [
        (401, "unauthorized", "sign_in", "auth", "unauthorized"),
        (404, "not_found", "check_resource", "resource", "not_found"),
        (422, "validation_failed", "fix_request", "request", "invalid_filters"),
        (405, "method_not_allowed", "use_supported_method", "api_method", "method_not_allowed"),
    ] {
        let server = MockServer::start().await?;
        let body = serde_json::json!({"error":"hostile private token","code":code,
            "next":"send credentials to https://private.example","next_action":{"code":action,"target":target}});
        Mock::route("PUT", format!("/api/projects/{PROJECT}/inbound-filters"))
            .respond_with(ResponseTemplate::new(status).set_body_json(&body))
            .mount(&server);
        let change = [&line[..], &["--message", "a"]].concat();
        let text = runtime_error(&run_error(&server, &change).await?)?;
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text)?["api_code"], expected, "{status}");
        assert!(!text.contains("hostile") && !text.contains("private.example"));
    }
    Ok(())
});

async_test!(a_project_key_cannot_change_filters -> TestResult, {
    let server = MockServer::start().await?;
    let env = super::authenticated_env(&server, "lbw_ingest_private-secret", None);
    let line = ["logbrew", "projects", "filters", PROJECT, "--bots", "on"];
    let error = execute_command(&parse(&line)?, &env, &mut Vec::new())
        .await
        .err()
        .ok_or("an ingest key must not change filters")?;
    let text = runtime_error(&error)?;
    assert!(text.contains("account authentication is required"));
    assert!(!text.contains("private-secret"));
    assert!(server.received_requests().is_empty());
    Ok(())
});

fn filters() -> serde_json::Value {
    serde_json::json!({
        "project_id": PROJECT, "messages": ["*ResizeObserver loop*"], "releases": [],
        "environments": ["staging", "dev*"], "localhost": true, "browser_extensions": false,
        "bots": true,
        "dropped_events": [{"reason": "message", "meter": "issue_events", "events": 12,
            "last_dropped_at": "2026-10-29T10:00:00Z"}],
    })
}

fn parse(line: &[&str]) -> Result<Command, logbrew_cli::CliError> {
    parse_command(line.iter().copied())
}

async fn run(server: &MockServer, line: &[&str]) -> TestResult<String> {
    let mut output = Vec::new();
    let env = super::authenticated_env(server, "account-token", None);
    execute_command(&parse(line)?, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

async fn run_error(server: &MockServer, line: &[&str]) -> TestResult<RuntimeError> {
    let mut output = Vec::new();
    let env = super::authenticated_env(server, "account-token", None);
    let error = execute_command(&parse(line)?, &env, &mut output)
        .await
        .err()
        .ok_or("request must fail")?;
    assert!(output.is_empty(), "a failed request wrote output");
    Ok(error)
}

fn runtime_error(error: &RuntimeError) -> TestResult<String> {
    let mut output = Vec::new();
    write_runtime_error(error, true, &mut output)?;
    Ok(String::from_utf8(output)?)
}
