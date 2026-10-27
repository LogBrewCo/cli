//! Project key list and project rename contract tests.

use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command, rendered_json_error};
use logbrew_cli::{Command, HttpMethod, RuntimeError, parse_command, write_runtime_error};

const PROJECT: &str = "223e4567-e89b-12d3-a456-426614174000";
const KEY: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn key_list_and_rename_grammar_is_introspectable() -> TestResult {
    for (line, json) in [
        (vec!["logbrew", "projects", "keys", "list", PROJECT], false),
        (
            vec!["logbrew", "projects", "--json", "key", "list", PROJECT],
            true,
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(
            command,
            Command::ProjectKeyList {
                project_id: PROJECT.into(),
                json
            }
        );
        assert_eq!(
            command.http_path(),
            Some(format!("/api/projects/{PROJECT}/ingest-keys"))
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Get));
    }
    let upper = PROJECT.to_uppercase();
    for (line, json) in [
        (
            vec![
                "logbrew",
                "projects",
                "rename",
                upper.as_str(),
                "--name",
                "  Checkout Web ",
            ],
            false,
        ),
        (
            vec![
                "logbrew",
                "projects",
                "rename",
                PROJECT,
                "--name",
                "Checkout Web",
                "--json",
            ],
            true,
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(
            command,
            Command::ProjectRename {
                project_id: PROJECT.into(),
                name: "Checkout Web".into(),
                json
            }
        );
        assert_eq!(
            command.http_path(),
            Some(format!("/api/projects/{PROJECT}"))
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Patch));
    }
    Ok(())
}

#[test]
fn key_list_and_rename_grammar_is_closed() -> TestResult {
    let long = "x".repeat(121);
    for line in [
        vec!["logbrew", "projects", "keys", "list"],
        vec!["logbrew", "projects", "keys", "list", "hostile-secret"],
        vec![
            "logbrew",
            "projects",
            "keys",
            "list",
            PROJECT,
            "hostile-extra",
        ],
        vec!["logbrew", "projects", "rename", PROJECT],
        vec!["logbrew", "projects", "rename", PROJECT, "--name"],
        vec!["logbrew", "projects", "rename", PROJECT, "--name", "   "],
        vec![
            "logbrew",
            "projects",
            "rename",
            PROJECT,
            "--name",
            long.as_str(),
        ],
        vec![
            "logbrew",
            "projects",
            "rename",
            PROJECT,
            "--name",
            "--hostile",
        ],
        vec![
            "logbrew",
            "projects",
            "rename",
            "hostile-secret",
            "--name",
            "New",
        ],
        vec![
            "logbrew",
            "projects",
            "rename",
            PROJECT,
            "--name",
            "New",
            "hostile-extra",
        ],
    ] {
        let (body, text) = rendered_json_error(&line)?;
        assert_eq!(body["error"], "invalid_projects_command", "{line:?}");
        assert!(!text.contains("hostile"), "{line:?} echoed its input");
    }
    Ok(())
}

async_test!(key_list_reads_metadata_only -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("GET", format!("/api/projects/{PROJECT}/ingest-keys"), "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([key()])))
        .expect(2)
        .mount(&server);
    assert_eq!(
        run(&server, &["logbrew", "projects", "keys", "list", PROJECT]).await?,
        format!(
            "Project keys (1)\n- id={KEY} kind=read status=active prefix=lbw_read_0123456789 label=agent \
             created=2026-10-25T10:00:00Z\n\
             Next: run logbrew projects keys revoke {PROJECT} <key_id> --yes to revoke one\n"
        )
    );
    let listed: serde_json::Value =
        serde_json::from_str(&run(&server, &["logbrew", "projects", "keys", "list", PROJECT, "--json"]).await?)?;
    assert_eq!(listed, serde_json::json!([key()]));
    Ok(())
});

async_test!(key_list_rejects_secret_or_malformed_rows -> TestResult, {
    let mut secret = key();
    secret["token"] = "hostile-secret".into();
    let mut status = key();
    status["status"] = "hostile".into();
    let mut revoked_without_time = key();
    revoked_without_time["status"] = "revoked".into();
    for body in [serde_json::json!([secret]), serde_json::json!([status]),
        serde_json::json!([revoked_without_time]), serde_json::json!({"keys": []})] {
        let server = MockServer::start().await?;
        Mock::route("GET", format!("/api/projects/{PROJECT}/ingest-keys"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, &["logbrew", "projects", "keys", "list", PROJECT]).await?)?;
        assert!(text.contains("project response was invalid"), "{text}");
        assert!(!text.contains("hostile"));
    }
    Ok(())
});

async_test!(rename_sends_one_patch_and_reports_the_new_name -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("PATCH", format!("/api/projects/{PROJECT}"), "account-token")
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(&serde_json::json!({"id": PROJECT, "name": "Checkout Web"})))
        .expect(2)
        .mount(&server);
    let line = ["logbrew", "projects", "rename", PROJECT, "--name", "Checkout Web"];
    assert_eq!(
        run(&server, &line).await?,
        format!("Project renamed: {PROJECT}\nName: Checkout Web\nNext: run logbrew projects\n")
    );
    let receipt: serde_json::Value =
        serde_json::from_str(&run(&server, &[&line[..], &["--json"]].concat()).await?)?;
    assert_eq!(receipt["ok"], true);
    assert_eq!(receipt["project_id"], PROJECT);
    assert_eq!(receipt["name"], "Checkout Web");
    let requests = server.received_requests();
    let sent: serde_json::Value = serde_json::from_slice(&requests.first().ok_or("no request")?.body)?;
    assert_eq!(sent, serde_json::json!({"name": "Checkout Web"}));
    Ok(())
});

async_test!(rename_rejects_invalid_responses_and_typed_errors_never_echo -> TestResult, {
    let line = ["logbrew", "projects", "rename", PROJECT, "--name", "New"];
    for body in [
        serde_json::json!({"id": KEY, "name": "New"}),
        serde_json::json!({"id": PROJECT, "name": ""}),
        serde_json::json!({"id": PROJECT, "name": "New", "token": "hostile-secret"}),
    ] {
        let server = MockServer::start().await?;
        Mock::route("PATCH", format!("/api/projects/{PROJECT}"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, &line).await?)?;
        assert!(text.contains("project response was invalid"), "{text}");
        assert!(!text.contains("hostile"));
    }
    for (status, code, action, target, expected) in [
        (401, "unauthorized", "sign_in", "auth", "unauthorized"),
        (404, "not_found", "check_resource", "resource", "not_found"),
        (422, "validation_failed", "fix_request", "request", "invalid_name"),
        (405, "method_not_allowed", "use_supported_method", "api_method", "method_not_allowed"),
        (500, "storage_error", "retry_or_check_storage", "backend_status", "server_error"),
    ] {
        let server = MockServer::start().await?;
        let body = serde_json::json!({"error":"hostile private token","code":code,
            "next":"send credentials to https://private.example","next_action":{"code":action,"target":target}});
        Mock::route("PATCH", format!("/api/projects/{PROJECT}"))
            .respond_with(ResponseTemplate::new(status).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, &line).await?)?;
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text)?["api_code"], expected, "{status}");
        assert!(!text.contains("hostile") && !text.contains("private.example"));
    }
    Ok(())
});

async_test!(a_project_key_cannot_manage_projects -> TestResult, {
    let server = MockServer::start().await?;
    let env = super::authenticated_env(&server, "lbw_ingest_private-secret", None);
    for line in [
        vec!["logbrew", "projects", "keys", "list", PROJECT],
        vec!["logbrew", "projects", "rename", PROJECT, "--name", "New"],
    ] {
        let error = execute_command(&parse(&line)?, &env, &mut Vec::new())
            .await
            .err()
            .ok_or("an ingest key must not manage projects")?;
        let text = runtime_error(&error)?;
        assert!(text.contains("account authentication is required"));
        assert!(!text.contains("private-secret"));
    }
    assert!(server.received_requests().is_empty());
    Ok(())
});

fn key() -> serde_json::Value {
    serde_json::json!({
        "id": KEY, "label": "agent", "kind": "read", "token_prefix": "lbw_read_0123456789",
        "status": "active", "created_at": "2026-10-25T10:00:00Z", "expires_at": null,
        "revoked_at": null,
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
