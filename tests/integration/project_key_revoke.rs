//! Project key revocation contract tests.

use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command, rendered_json_error};
use logbrew_cli::{Command, HttpMethod, RuntimeError, parse_command, write_runtime_error};

const PROJECT: &str = "223e4567-e89b-12d3-a456-426614174000";
const KEY: &str = "123e4567-e89b-12d3-a456-426614174000";
const REVOKE: &str = "logbrew projects keys revoke 223e4567-e89b-12d3-a456-426614174000 \
                      123e4567-e89b-12d3-a456-426614174000 --yes";

#[test]
fn grammar_is_closed_explicit_and_introspectable() -> TestResult {
    for (line, json) in [
        (REVOKE.to_owned(), false),
        (format!("{REVOKE} --json"), true),
        (
            format!(
                "logbrew projects key revoke {} {} --yes",
                PROJECT.to_uppercase(),
                KEY.to_uppercase()
            ),
            false,
        ),
    ] {
        let command = parse(&line)?;
        assert_eq!(
            command,
            Command::ProjectKeyRevoke {
                project_id: PROJECT.into(),
                key_id: KEY.into(),
                json
            }
        );
        assert_eq!(
            command.http_path(),
            Some(format!("/api/projects/{PROJECT}/ingest-keys/{KEY}"))
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Delete));
        assert!(command.request_body().is_none());
    }
    for line in [
        format!("logbrew projects keys revoke {PROJECT} {KEY}"),
        format!("logbrew projects keys revoke {PROJECT} hostile-secret --yes"),
        format!("logbrew projects keys revoke {KEY} --yes"),
        format!("logbrew projects keys revoke {PROJECT} {KEY} --yes=true"),
        format!("logbrew projects keys revoke {PROJECT} {KEY} --yes hostile-private-value"),
    ] {
        let (body, text) = rendered_json_error(&line.split_whitespace().collect::<Vec<_>>())?;
        assert_eq!(body["error"], "invalid_projects_command", "{line}");
        assert!(!text.contains("hostile"), "{line} echoed its input");
    }
    Ok(())
}

async_test!(revoke_sends_one_delete_and_writes_a_local_receipt -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("DELETE", format!("/api/projects/{PROJECT}/ingest-keys/{KEY}"), "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&revoked()))
        .expect(2)
        .mount(&server);
    assert_eq!(
        run(&server, REVOKE).await?,
        format!(
            "Project key revoked: {KEY}\nProject: {PROJECT}\nKind: read\n\
             Next: create a replacement with logbrew projects keys create if it is still needed\n"
        )
    );
    let receipt: serde_json::Value = serde_json::from_str(&run(&server, &format!("{REVOKE} --json")).await?)?;
    assert_eq!(receipt["status"], "revoked");
    assert_eq!(receipt["key_id"], KEY);
    assert_eq!(receipt["project_id"], PROJECT);
    assert_eq!(receipt["kind"], "read");
    Ok(())
});

async_test!(revoke_rejects_invalid_responses_and_typed_errors_never_echo -> TestResult, {
    let mut active = revoked();
    active["status"] = "active".into();
    active["revoked_at"] = serde_json::Value::Null;
    let mut other = revoked();
    other["id"] = PROJECT.into();
    let mut extra = revoked();
    extra["token"] = "hostile-secret".into();
    for body in [active, other, extra] {
        let server = MockServer::start().await?;
        Mock::route("DELETE", format!("/api/projects/{PROJECT}/ingest-keys/{KEY}"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, REVOKE).await?)?;
        assert!(text.contains("project key revocation response was invalid"), "{text}");
        assert!(!text.contains("hostile"));
    }
    for (status, code, action, target, expected) in [
        (401, "unauthorized", "sign_in", "auth", "unauthorized"),
        (404, "not_found", "check_resource", "resource", "not_found"),
        (500, "storage_error", "retry_or_check_storage", "backend_status", "server_error"),
    ] {
        let server = MockServer::start().await?;
        let body = serde_json::json!({"error":"hostile private token","code":code,
            "next":"send credentials to https://private.example","next_action":{"code":action,"target":target}});
        Mock::route("DELETE", format!("/api/projects/{PROJECT}/ingest-keys/{KEY}"))
            .respond_with(ResponseTemplate::new(status).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, REVOKE).await?)?;
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text)?["api_code"], expected);
        assert!(!text.contains("hostile") && !text.contains("private.example"));
    }
    Ok(())
});

async_test!(a_project_key_cannot_revoke_keys -> TestResult, {
    let server = MockServer::start().await?;
    let env = super::authenticated_env(&server, "lbw_ingest_private-secret", None);
    let error = execute_command(&parse(REVOKE)?, &env, &mut Vec::new())
        .await
        .err()
        .ok_or("an ingest key must not reach key revocation")?;
    let text = runtime_error(&error)?;
    assert!(text.contains("account authentication is required"));
    assert!(!text.contains("private-secret"));
    assert!(server.received_requests().is_empty());
    Ok(())
});

fn revoked() -> serde_json::Value {
    serde_json::json!({
        "id": KEY, "label": "agent", "kind": "read", "token_prefix": "lbw_read_0123456789",
        "status": "revoked", "created_at": "2026-10-25T10:00:00Z", "expires_at": null,
        "revoked_at": "2026-10-25T11:00:00Z",
    })
}

fn parse(line: &str) -> Result<Command, logbrew_cli::CliError> {
    parse_command(line.split_whitespace())
}

async fn run(server: &MockServer, line: &str) -> TestResult<String> {
    let mut output = Vec::new();
    let env = super::authenticated_env(server, "account-token", None);
    execute_command(&parse(line)?, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

async fn run_error(server: &MockServer, line: &str) -> TestResult<RuntimeError> {
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
