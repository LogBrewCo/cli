//! Account-owned project lifecycle contract tests.

use crate::matchers::{body_json, header};
use crate::{Mock, MockServer, ResponseTemplate};
use logbrew_cli::{
    Command, HttpMethod, RuntimeError, execute_command, parse_command, write_cli_error,
    write_runtime_error,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const ARCHIVE: &str = "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000 --yes --json";
const DELETE: &str = "logbrew projects delete 123e4567-e89b-12d3-a456-426614174000 --confirm 123e4567-e89b-12d3-a456-426614174000";
const DELETE_JSON: &str = "logbrew projects delete 123e4567-e89b-12d3-a456-426614174000 --confirm 123e4567-e89b-12d3-a456-426614174000 --json";
const RECEIPT: &str = r#"{"ticket_id":"sup_11111111111141118111111111111111","status":"open","created_at":"2026-08-15T12:00:00.000Z","next":"Support ticket created. LogBrew will review the attached diagnostics.","next_action":{"code":"review_ticket","target":"support_ticket"}}"#;

#[test]
fn lifecycle_grammar_is_closed_explicit_and_introspectable() -> TestResult {
    for (line, json) in [
        (
            "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000 --yes",
            false,
        ),
        (
            "logbrew project archive 123e4567-e89b-12d3-a456-426614174000 --yes --json",
            true,
        ),
        (
            "logbrew --json projects archive 123e4567-e89b-12d3-a456-426614174000 --yes",
            true,
        ),
        (
            "logbrew projects --json archive 123e4567-e89b-12d3-a456-426614174000 --yes",
            true,
        ),
        (
            "logbrew projects archive --json 123e4567-e89b-12d3-a456-426614174000 --yes",
            true,
        ),
        (
            "logbrew projects archive 123E4567-E89B-12D3-A456-426614174000 --yes",
            false,
        ),
    ] {
        let command = parse(line)?;
        assert_eq!(
            command,
            Command::ProjectArchive {
                project_id: ID.into(),
                json
            }
        );
        assert_eq!(command.http_path(), Some(format!("/api/projects/{ID}")));
        assert_eq!(command.http_method(), Some(HttpMethod::Delete));
        assert!(command.request_body().is_none());
    }
    for line in [
        DELETE,
        DELETE_JSON,
        "logbrew projects --json delete 123e4567-e89b-12d3-a456-426614174000 --confirm 123e4567-e89b-12d3-a456-426614174000",
    ] {
        let command = parse(line)?;
        assert_eq!(
            command,
            Command::ProjectDeletion {
                project_id: ID.into(),
                json: command.wants_json()
            }
        );
        assert_eq!(command.http_path().as_deref(), Some("/api/support/tickets"));
        assert_eq!(command.http_method(), Some(HttpMethod::Post));
        assert_eq!(command.request_body(), Some(deletion_body()));
    }
    assert_eq!(
        parse(
            "logbrew projects delete 123E4567-E89B-12D3-A456-426614174000 --confirm=123e4567-e89b-12d3-a456-426614174000"
        )?,
        Command::ProjectDeletion {
            project_id: ID.into(),
            json: false
        }
    );

    for (line, deletion) in [
        (
            "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000",
            false,
        ),
        ("logbrew projects archive hostile-secret --yes", false),
        (
            "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000 --yes --yes",
            false,
        ),
        (
            "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000 --yes --json --json",
            false,
        ),
        (
            "logbrew projects archive --yes 123e4567-e89b-12d3-a456-426614174000",
            false,
        ),
        (
            "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000 --yes=true",
            false,
        ),
        (
            "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000 --yes --authorization=hostile-secret",
            false,
        ),
        (
            "logbrew projects archive 123e4567-e89b-12d3-a456-426614174000 --yes hostile-private-value",
            false,
        ),
        (
            "logbrew projects delete 123e4567-e89b-12d3-a456-426614174000 --confirm hostile-secret",
            true,
        ),
        (
            "logbrew projects delete 123e4567-e89b-12d3-a456-426614174000 --confirm 123e4567-e89b-12d3-a456-426614174000 --json --json",
            true,
        ),
    ] {
        let error = parse(line).expect_err("invalid lifecycle grammar fails closed");
        let body = cli_error(&error)?;
        let prefix = if deletion { "deletion" } else { "archive" };
        assert_eq!(body["error"], format!("invalid_project_{prefix}_command"));
        assert_eq!(body["message"], format!("invalid project {prefix} command"));
        let next = if deletion {
            "use logbrew projects delete <project_id> --confirm <project_id> with optional --json"
        } else {
            "use logbrew projects archive <project_id> --yes with optional --json"
        };
        assert_eq!(body["next"], next);
        let text = body.to_string();
        assert!(
            !["hostile-secret", "hostile-private-value", "authorization"]
                .iter()
                .any(|value| text.contains(value))
        );
    }
    Ok(())
}

#[tokio::test]
async fn lifecycle_revalidates_public_values_and_rejects_ingest_keys() -> TestResult {
    let server = MockServer::start().await;
    for command in [
        Command::ProjectArchive {
            project_id: "../hostile-secret?authorization=private".into(),
            json: true,
        },
        Command::ProjectDeletion {
            project_id: "../hostile-secret?authorization=private".into(),
            json: true,
        },
    ] {
        let error = execute_command(
            &command,
            &super::authenticated_env(&server, "account-token", None),
            &mut Vec::new(),
        )
        .await
        .expect_err("invalid public value fails locally");
        let text = runtime_error(&error)?;
        assert!(!text.contains("hostile-secret") && !text.contains("authorization"));
    }
    for line in [ARCHIVE, DELETE_JSON] {
        let error = execute_command(
            &parse(line)?,
            &super::authenticated_env(&server, "lbw_ingest_private-secret", None),
            &mut Vec::new(),
        )
        .await
        .expect_err("ingest key cannot mutate lifecycle");
        let text = runtime_error(&error)?;
        assert!(text.contains("account authentication is required"));
        assert!(text.contains(if line == ARCHIVE {
            "retry the project archive command"
        } else {
            "retry the project deletion command"
        }));
        assert!(!text.contains("private-secret"));
    }
    assert!(requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn archive_and_delete_send_exact_requests_and_local_receipts() -> TestResult {
    let server = MockServer::start().await;
    Mock::auth("DELETE", format!("/api/projects/{ID}"), "account-token")
        .respond_with(ResponseTemplate::new(204))
        .expect(2)
        .mount(&server)
        .await;
    mount_delete(
        &server,
        ResponseTemplate::new(200).set_body_raw(RECEIPT, "application/json"),
        2,
    )
    .await;

    for (line, expected) in [
        (
            ARCHIVE.trim_end_matches(" --json"),
            format!(
                "Project archived: {ID}\nProject ingest keys: disabled\nNext: run logbrew projects\n"
            ),
        ),
        (
            ARCHIVE,
            format!(
                "{{\"ok\":true,\"project_id\":\"{ID}\",\"status\":\"archived\",\"next\":\"run logbrew projects --json\"}}\n"
            ),
        ),
        (
            DELETE,
            format!(
                "Project deletion accepted: {ID}\nProject status: inactive\nPermanent deletion: scheduled automatically\nNext: run logbrew projects\n"
            ),
        ),
        (
            DELETE_JSON,
            format!(
                "{{\"ok\":true,\"project_id\":\"{ID}\",\"project_active\":false,\"status\":\"deletion_scheduled\",\"next\":\"run logbrew projects --json\"}}\n"
            ),
        ),
    ] {
        assert_eq!(run(&server, line).await?, expected);
    }

    for request in requests(&server).await? {
        assert!(request.url.query().is_none());
        if request.url.path() == "/api/support/tickets" {
            assert_eq!(
                request
                    .headers
                    .get("idempotency-key")
                    .and_then(|v| v.to_str().ok()),
                Some(ID)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&request.body)?,
                deletion_body()
            );
        } else {
            assert!(request.body.is_empty());
        }
    }
    Ok(())
}

#[tokio::test]
async fn built_binary_deletes_over_loopback_with_a_local_receipt() -> TestResult {
    let server = MockServer::start().await;
    mount_delete(
        &server,
        ResponseTemplate::new(200).set_body_raw(RECEIPT, "application/json"),
        1,
    )
    .await;
    let home = super::isolated_home("logbrew-project-delete", "binary")?;
    let mut command = super::cli_command(&server);
    let _command =
        command
            .env("HOME", home)
            .args(["projects", "delete", ID, "--confirm", ID, "--json"]);
    let process = super::run_cli_command(command).await?;

    super::assert_cli_success(&process);
    let text = String::from_utf8(process.stdout)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text)?,
        serde_json::json!({"ok":true,"project_id":ID,"project_active":false,"status":"deletion_scheduled","next":"run logbrew projects --json"})
    );
    assert!(!text.contains("sup_") && !text.contains(server.uri().as_str()));
    Ok(())
}

#[tokio::test]
async fn success_contracts_reject_wrong_status_redirects_and_hostile_bodies() -> TestResult {
    for response in [
        ResponseTemplate::new(200),
        ResponseTemplate::new(201),
        ResponseTemplate::new(307).insert_header("Location", "/private-target"),
        ResponseTemplate::new(404).set_body_string(r#"{"error":"hostile-secret","code":"not_found","next":"hostile-private-path","next_action":{"code":"check_resource","target":"resource"},"private_token":"hostile-secret"}"#),
        ResponseTemplate::new(404).set_body_string(r#"{"error":"hostile-secret","code":"not_found","next":"hostile-private-path","next_action":{"code":"retry","target":"request"}}"#),
        ResponseTemplate::new(404).set_body_string(r#"{"error":"hostile-secret\ncontrol","code":"not_found","next":"hostile-private-path","next_action":{"code":"check_resource","target":"resource"}}"#),
        ResponseTemplate::new(404).set_body_string("hostile-secret".repeat(70_000)),
    ] {
        let server = MockServer::start().await;
        Mock::route("DELETE", format!("/api/projects/{ID}"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let error = run_error(&server, ARCHIVE).await;
        assert!(matches!(error, RuntimeError::Unavailable { .. }));
        let text = runtime_error(&error)?;
        assert!(text.contains("project archive response was invalid"));
        assert_private_text_absent(&text, &server);
        assert_eq!(requests(&server).await?.len(), 1);
    }

    for response in [
        ResponseTemplate::new(201).set_body_raw(RECEIPT, "application/json"),
        ResponseTemplate::new(307).insert_header("Location", "/private-target"),
        ResponseTemplate::new(200).set_body_string("not-json-hostile-secret"),
        ResponseTemplate::new(200).set_body_json(invalid_receipt()),
        ResponseTemplate::new(200).set_body_string("x".repeat(70_000)),
    ] {
        let server = MockServer::start().await;
        mount_delete(&server, response, 1).await;
        let error = run_error(&server, DELETE_JSON).await;
        let text = runtime_error(&error)?;
        assert!(matches!(
            error,
            RuntimeError::Unavailable { .. } | RuntimeError::Api { .. }
        ));
        assert!(text.contains("project deletion") && !text.contains("hostile-secret"));
        assert_eq!(requests(&server).await?.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn errors_are_typed_local_and_never_echo_backend_text() -> TestResult {
    for (status, code, action, target, expected) in [
        (401, "unauthorized", "sign_in", "auth", "unauthorized"),
        (403, "forbidden", "request_access", "auth", "forbidden"),
        (404, "not_found", "check_resource", "resource", "not_found"),
        (
            405,
            "method_not_allowed",
            "use_supported_method",
            "api_method",
            "method_not_allowed",
        ),
        (
            500,
            "storage_error",
            "retry_or_check_storage",
            "backend_status",
            "server_error",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::route("DELETE", format!("/api/projects/{ID}"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(archive_error(code, action, target)),
            )
            .mount(&server)
            .await;
        let text = runtime_error(&run_error(&server, ARCHIVE).await)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text)?["api_code"],
            expected
        );
        assert_private_text_absent(&text, &server);
    }
    for status in [401, 403, 404, 409, 422, 500] {
        let server = MockServer::start().await;
        mount_delete(
            &server,
            ResponseTemplate::new(status).set_body_string("hostile-secret https://private.example"),
            1,
        )
        .await;
        let text = runtime_error(&run_error(&server, DELETE_JSON).await)?;
        assert!(serde_json::from_str::<serde_json::Value>(&text)?["api_code"].is_string());
        assert_private_text_absent(&text, &server);
    }
    Ok(())
}

#[tokio::test]
async fn lifecycle_mutations_refresh_account_auth_once() -> TestResult {
    for (line, verb, endpoint, success, marker) in [
        (
            ARCHIVE,
            "DELETE",
            format!("/api/projects/{ID}"),
            ResponseTemplate::new(204),
            "\"status\":\"archived\"",
        ),
        (
            DELETE_JSON,
            "POST",
            String::from("/api/support/tickets"),
            ResponseTemplate::new(200).set_body_raw(RECEIPT, "application/json"),
            "deletion_scheduled",
        ),
    ] {
        let server = MockServer::start().await;
        for (token, response) in [
            ("expired-access", ResponseTemplate::new(401)),
            ("fresh-access", success),
        ] {
            Mock::auth(verb, endpoint.as_str(), token)
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::route("POST", "/api/auth/refresh")
            .and(body_json(
                serde_json::json!({"refresh_token": "old-refresh"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"access_token":"fresh-access","refresh_token":"fresh-refresh"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let home = super::isolated_home(
            "logbrew-project-delete",
            if verb == "DELETE" {
                "archive-refresh"
            } else {
                "delete-refresh"
            },
        )?;
        let _session_path = super::write_test_session(
            home.as_path(),
            server.uri().as_str(),
            "expired-access",
            "old-refresh",
        )?;
        let env = super::test_env(&server, None, Some(home.clone()));
        let mut output = Vec::new();

        execute_command(&parse(line)?, &env, &mut output).await?;

        assert!(String::from_utf8(output)?.contains(marker));
        let session: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
            home.join(".logbrew/session.json"),
        )?)?;
        assert_eq!(session["access_token"], "fresh-access");
        assert_eq!(session["refresh_token"], "fresh-refresh");
    }
    Ok(())
}

fn parse(line: &str) -> Result<Command, logbrew_cli::CliError> {
    parse_command(line.split_whitespace())
}

fn deletion_body() -> serde_json::Value {
    serde_json::json!({"source":"cli","category":"project_deletion","project_id":ID,"title":"Permanent project deletion request","description":"Permanent project deletion requested from LogBrew CLI."})
}

fn invalid_receipt() -> serde_json::Value {
    let mut value: serde_json::Value = serde_json::from_str(RECEIPT).expect("fixture parses");
    value["private_token"] = "hostile-secret".into();
    value
}

fn archive_error(code: &str, action: &str, target: &str) -> serde_json::Value {
    serde_json::json!({"error":"hostile private token lbw_ingest_do_not_echo","code":code,"next":"send credentials to https://private.example/path","next_action":{"code":action,"target":target}})
}

async fn run(server: &MockServer, line: &str) -> TestResult<String> {
    let mut output = Vec::new();
    execute_command(
        &parse(line)?,
        &super::authenticated_env(server, "account-token", None),
        &mut output,
    )
    .await?;
    Ok(String::from_utf8(output)?)
}

async fn run_error(server: &MockServer, line: &str) -> RuntimeError {
    execute_command(
        &parse(line).expect("command parses"),
        &super::authenticated_env(server, "account-token", None),
        &mut Vec::new(),
    )
    .await
    .expect_err("request fails")
}

async fn mount_delete(server: &MockServer, response: ResponseTemplate, expected: u64) {
    Mock::auth("POST", "/api/support/tickets", "account-token")
        .and(header("idempotency-key", ID))
        .and(body_json(deletion_body()))
        .respond_with(response)
        .expect(expected)
        .mount(server)
        .await;
}

async fn requests(server: &MockServer) -> Result<Vec<crate::Request>, &'static str> {
    Ok(server.received_requests().await)
}

fn cli_error(error: &logbrew_cli::CliError) -> TestResult<serde_json::Value> {
    let mut output = Vec::new();
    write_cli_error(error, true, &mut output)?;
    Ok(serde_json::from_slice(&output)?)
}

fn runtime_error(error: &RuntimeError) -> TestResult<String> {
    let mut output = Vec::new();
    write_runtime_error(error, true, &mut output)?;
    Ok(String::from_utf8(output)?)
}

fn assert_private_text_absent(text: &str, server: &MockServer) {
    for private in [
        "hostile",
        "credentials",
        "private.example",
        "lbw_ingest",
        server.uri().as_str(),
    ] {
        assert!(!text.contains(private));
    }
}
