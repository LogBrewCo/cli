//! Connected MCP client list and revocation contract tests.

use crate::matchers::query_param;
use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command, rendered_json_error};
use logbrew_cli::{Command, HttpMethod, RuntimeError, parse_command, write_runtime_error};

const ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT: &str = "223e4567-e89b-12d3-a456-426614174000";
const CLIENT: &str = "https://client.example.test/oauth/metadata.json";
const REVOKE: &str = "logbrew mcp revoke 123e4567-e89b-12d3-a456-426614174000 --yes";

#[test]
fn grammar_is_closed_explicit_and_introspectable() -> TestResult {
    for (line, json) in [
        ("logbrew mcp grants", false),
        ("logbrew mcp grants --json", true),
        ("logbrew --json mcp grants", true),
        ("logbrew mcp --json grants", true),
    ] {
        let command = parse(line)?;
        assert_eq!(command, Command::McpGrants { json });
        assert_eq!(
            command.http_path().as_deref(),
            Some("/api/mcp/grants?response_version=2")
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Get));
    }
    for (line, json) in [
        (REVOKE, false),
        (
            "logbrew mcp revoke 123E4567-E89B-12D3-A456-426614174000 --yes --json",
            true,
        ),
        (
            "logbrew mcp --json revoke 123e4567-e89b-12d3-a456-426614174000 --yes",
            true,
        ),
    ] {
        let command = parse(line)?;
        assert_eq!(
            command,
            Command::McpRevoke {
                grant_id: ID.into(),
                json
            }
        );
        assert_eq!(command.http_path(), Some(format!("/api/mcp/grants/{ID}")));
        assert_eq!(command.http_method(), Some(HttpMethod::Delete));
        assert!(command.request_body().is_none());
    }
    for line in [
        "logbrew mcp",
        "logbrew mcp grants extra",
        "logbrew mcp grants --json --json",
        "logbrew mcp revoke 123e4567-e89b-12d3-a456-426614174000",
        "logbrew mcp revoke hostile-secret --yes",
        "logbrew mcp revoke 123e4567-e89b-12d3-a456-426614174000 --yes=true",
        "logbrew mcp revoke --yes 123e4567-e89b-12d3-a456-426614174000",
        "logbrew mcp revoke 123e4567-e89b-12d3-a456-426614174000 --yes hostile-private-value",
    ] {
        let (body, text) = rendered_json_error(&line.split_whitespace().collect::<Vec<_>>())?;
        assert_eq!(body["error"], "invalid_mcp_command", "{line}");
        assert_eq!(
            body["next"],
            "use logbrew mcp grants or logbrew mcp revoke <grant_id> --yes, with optional --json"
        );
        assert!(!text.contains("hostile"), "{line} echoed its input");
    }
    Ok(())
}

async_test!(list_renders_validated_grants -> TestResult, {
    let server = MockServer::start().await?;
    let mut named = grant();
    named["client_name"] = "Claude Code".into();
    let mut unnamed = grant();
    unnamed["client_name"] = serde_json::Value::Null;
    let body = grants(&[named, unnamed, grant()]);
    Mock::auth("GET", "/api/mcp/grants", "account-token")
        .and(query_param("response_version", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(2)
        .mount(&server);
    assert_eq!(run(&server, "logbrew mcp grants --json").await?, format!("{body}\n"));
    let row = |client: &str| {
        format!(
            "- id={ID} client={client} projects=1 \
             last_used=2026-10-15T10:00:00.000Z expires=2026-11-14T10:00:00.000Z\n"
        )
    };
    assert_eq!(
        run(&server, "logbrew mcp grants").await?,
        format!(
            "MCP grants (3)\n{}{}{}Next: run logbrew mcp revoke <grant_id> --yes\n",
            row("Claude Code"),
            row(CLIENT),
            row(CLIENT)
        )
    );
    let empty = MockServer::start().await?;
    Mock::auth("GET", "/api/mcp/grants", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&grants(&[])))
        .mount(&empty);
    assert_eq!(
        run(&empty, "logbrew mcp grants").await?,
        "MCP grants (0)\nNo connected MCP clients.\n"
    );
    Ok(())
});

async_test!(list_rejects_invalid_and_hostile_responses -> TestResult, {
    let mut extra = grant();
    extra["private_token"] = "hostile-secret".into();
    let mut control = grant();
    control["client_id"] = "hostile\u{1b}[31m".into();
    let mut project = grant();
    project["project_ids"] = serde_json::json!(["hostile-secret"]);
    let names = [
        serde_json::json!("hostile\u{1b}[31m"),
        serde_json::json!("hostile\u{202e}name"),
        serde_json::json!("hostile".repeat(15)),
        serde_json::json!(" "),
        serde_json::json!(["hostile"]),
    ];
    let mut responses = vec![
        grants(&[extra]),
        grants(&[control]),
        grants(&[project]),
        serde_json::json!([grant()]),
    ];
    for name in names {
        let mut row = grant();
        row["client_name"] = name;
        responses.push(grants(&[row]));
    }
    for response in responses {
        let server = MockServer::start().await?;
        Mock::auth("GET", "/api/mcp/grants", "account-token")
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .mount(&server);
        let text = runtime_error(&run_error(&server, "logbrew mcp grants").await?)?;
        assert!(text.contains("MCP grant response was invalid"));
        assert!(!text.contains("hostile"));
    }
    Ok(())
});

async_test!(revoke_sends_one_delete_and_writes_a_local_receipt -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("DELETE", format!("/api/mcp/grants/{ID}"), "account-token")
        .respond_with(ResponseTemplate::new(204))
        .expect(2)
        .mount(&server);
    assert_eq!(
        run(&server, REVOKE).await?,
        format!(
            "MCP grant revoked: {ID}\nThe client must sign in again to reconnect.\n\
             Next: run logbrew mcp grants\n"
        )
    );
    assert_eq!(
        run(&server, &format!("{REVOKE} --json")).await?,
        format!(
            "{{\"grant_id\":\"{ID}\",\"next\":\"run logbrew mcp grants --json\",\"ok\":true,\
             \"status\":\"revoked\"}}\n"
        )
    );
    Ok(())
});

async_test!(revoke_errors_are_typed_and_never_echo_backend_text -> TestResult, {
    for (status, code, action, target, expected) in [
        (401, "unauthorized", "sign_in", "auth", "unauthorized"),
        (404, "not_found", "check_resource", "resource", "not_found"),
        (500, "storage_error", "retry_or_check_storage", "backend_status", "server_error"),
    ] {
        let server = MockServer::start().await?;
        let body = serde_json::json!({"error":"hostile private token","code":code,
            "next":"send credentials to https://private.example","next_action":{"code":action,"target":target}});
        Mock::route("DELETE", format!("/api/mcp/grants/{ID}"))
            .respond_with(ResponseTemplate::new(status).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, REVOKE).await?)?;
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text)?["api_code"], expected);
        assert!(!text.contains("hostile") && !text.contains("private.example"));
    }
    for response in [
        ResponseTemplate::new(200),
        ResponseTemplate::new(307).insert_header("Location", "/private-target"),
        ResponseTemplate::new(404).set_body_string("hostile-secret"),
    ] {
        let server = MockServer::start().await?;
        Mock::route("DELETE", format!("/api/mcp/grants/{ID}")).respond_with(response).mount(&server);
        let text = runtime_error(&run_error(&server, REVOKE).await?)?;
        assert!(text.contains("MCP grant") && !text.contains("hostile"));
    }
    Ok(())
});

async_test!(an_ingest_key_cannot_list_or_revoke -> TestResult, {
    let server = MockServer::start().await?;
    for line in ["logbrew mcp grants", REVOKE] {
        let env = super::authenticated_env(&server, "lbw_ingest_private-secret", None);
        let error = execute_command(&parse(line)?, &env, &mut Vec::new())
            .await
            .err()
            .ok_or("an ingest key must not reach the grant routes")?;
        let text = runtime_error(&error)?;
        assert!(text.contains("account authentication is required"));
        assert!(!text.contains("private-secret"));
    }
    assert!(server.received_requests().is_empty());
    Ok(())
});

fn grant() -> serde_json::Value {
    serde_json::json!({
        "id": ID, "client_id": CLIENT, "project_ids": [PROJECT],
        "created_at": "2026-10-01T10:00:00.000Z", "last_used_at": "2026-10-15T10:00:00.000Z",
        "expires_at": "2026-11-14T10:00:00.000Z",
    })
}

fn grants(rows: &[serde_json::Value]) -> serde_json::Value {
    serde_json::json!({ "grants": rows })
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
