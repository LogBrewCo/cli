//! Safe authenticated project archival contract tests.

use logbrew_cli::{
    CliEnvironment, Command, HttpMethod, RuntimeError, execute_command, parse_command,
    write_cli_error, write_runtime_error,
};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn project_archive_requires_explicit_confirmation_and_normalizes_json_order() {
    for (args, json) in [
        (
            &["logbrew", "projects", "archive", PROJECT_ID, "--yes"][..],
            false,
        ),
        (
            &[
                "logbrew", "project", "archive", PROJECT_ID, "--yes", "--json",
            ][..],
            true,
        ),
        (
            &[
                "logbrew", "--json", "projects", "archive", PROJECT_ID, "--yes",
            ][..],
            true,
        ),
        (
            &[
                "logbrew", "projects", "--json", "archive", PROJECT_ID, "--yes",
            ][..],
            true,
        ),
        (
            &[
                "logbrew", "projects", "archive", "--json", PROJECT_ID, "--yes",
            ][..],
            true,
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("project archive parses");

        assert_eq!(
            command,
            Command::ProjectArchive {
                project_id: PROJECT_ID.to_owned(),
                json,
            }
        );
        assert_eq!(
            command.http_path().as_deref(),
            Some("/api/projects/123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Delete));
        assert_eq!(command.wants_json(), json);
        assert!(command.request_body().is_none());
    }

    assert_eq!(
        parse_command([
            "logbrew",
            "projects",
            "archive",
            "123E4567-E89B-12D3-A456-426614174000",
            "--yes",
        ])
        .expect("uppercase UUID normalizes"),
        Command::ProjectArchive {
            project_id: PROJECT_ID.to_owned(),
            json: false,
        }
    );
}

#[test]
fn project_archive_grammar_fails_closed_without_reflecting_values() {
    for args in [
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from(PROJECT_ID),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from("not-a-uuid-hostile-secret"),
            String::from("--yes"),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from(PROJECT_ID),
            String::from("--yes"),
            String::from("--yes"),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from(PROJECT_ID),
            String::from("--yes"),
            String::from("--json"),
            String::from("--json"),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from("--yes"),
            String::from(PROJECT_ID),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from(PROJECT_ID),
            String::from("--yes=true"),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from(PROJECT_ID),
            String::from("--yes"),
            String::from("--authorization=hostile-secret\ncontrol"),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("archive"),
            String::from(PROJECT_ID),
            String::from("--yes"),
            String::from("hostile-private-value"),
        ],
    ] {
        let error = parse_command(args).expect_err("archive grammar fails closed");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("parse error writes");
        let text = String::from_utf8(output).expect("utf8 parse error");
        let body: serde_json::Value = serde_json::from_str(text.as_str()).expect("valid json");

        assert_eq!(body["error"], "invalid_project_archive_command");
        assert_eq!(body["message"], "invalid project archive command");
        assert_eq!(
            body["next"],
            "use logbrew projects archive <project_id> --yes with optional --json"
        );
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("hostile-private-value"));
        assert!(!text.contains("authorization"));
    }
}

#[tokio::test]
async fn project_archive_revalidates_public_command_values_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let command = Command::ProjectArchive {
        project_id: String::from("../hostile-secret?authorization=private"),
        json: true,
    };
    let error = execute_command(
        &command,
        &environment(&server, "account-token"),
        &mut Vec::new(),
    )
    .await
    .expect_err("constructed invalid archive command fails locally");
    let requests = server
        .received_requests()
        .await
        .ok_or("requests disabled")?;
    let mut output = Vec::new();

    write_runtime_error(&error, true, &mut output)?;

    let text = String::from_utf8(output)?;
    assert!(requests.is_empty());
    assert!(text.contains("invalid project archive command"));
    assert!(!text.contains("hostile-secret"));
    assert!(!text.contains("authorization"));
    Ok(())
}

#[tokio::test]
async fn project_archive_sends_one_body_free_delete_and_writes_stable_success()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/api/projects/{PROJECT_ID}")))
        .and(header("authorization", "Bearer account-token"))
        .respond_with(ResponseTemplate::new(204))
        .expect(2)
        .mount(&server)
        .await;

    let human = parse_command(["logbrew", "projects", "archive", PROJECT_ID, "--yes"])?;
    let mut human_output = Vec::new();
    execute_command(
        &human,
        &environment(&server, "account-token"),
        &mut human_output,
    )
    .await?;
    assert_eq!(
        String::from_utf8(human_output)?,
        "Project archived: 123e4567-e89b-12d3-a456-426614174000\n\
         Project ingest keys: disabled\n\
         Next: run logbrew projects\n"
    );

    let json = parse_command([
        "logbrew", "projects", "archive", PROJECT_ID, "--yes", "--json",
    ])?;
    let mut json_output = Vec::new();
    execute_command(
        &json,
        &environment(&server, "account-token"),
        &mut json_output,
    )
    .await?;
    assert_eq!(
        String::from_utf8(json_output)?,
        "{\"ok\":true,\"project_id\":\"123e4567-e89b-12d3-a456-426614174000\",\
         \"status\":\"archived\",\"next\":\"run logbrew projects --json\"}\n"
    );

    let requests = server
        .received_requests()
        .await
        .ok_or("requests disabled")?;
    assert!(
        requests
            .iter()
            .all(|request| request.url.query().is_none() && request.body.is_empty())
    );
    Ok(())
}

#[tokio::test]
async fn project_archive_rejects_malformed_and_oversized_error_envelopes_without_leaks()
-> Result<(), Box<dyn std::error::Error>> {
    let malformed = [
        serde_json::json!({
            "error": "hostile-secret",
            "code": "not_found",
            "next": "hostile-private-path",
            "next_action": {"code": "check_resource", "target": "resource"},
            "private_token": "hostile-secret"
        })
        .to_string(),
        serde_json::json!({
            "error": "hostile-secret",
            "code": "not_found",
            "next": "hostile-private-path",
            "next_action": {"code": "retry", "target": "request"}
        })
        .to_string(),
        serde_json::json!({
            "error": "hostile-secret\ncontrol",
            "code": "not_found",
            "next": "hostile-private-path",
            "next_action": {"code": "check_resource", "target": "resource"}
        })
        .to_string(),
        format!(
            "{{\"error\":\"{}\",\"code\":\"not_found\",\"next\":\"retry\",\
             \"next_action\":{{\"code\":\"check_resource\",\"target\":\"resource\"}}}}",
            "hostile-secret".repeat(6_000)
        ),
    ];

    for body in malformed {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/projects/{PROJECT_ID}")))
            .respond_with(ResponseTemplate::new(404).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;
        let command = parse_command([
            "logbrew", "projects", "archive", PROJECT_ID, "--yes", "--json",
        ])?;
        let error = execute_command(
            &command,
            &environment(&server, "account-token"),
            &mut Vec::new(),
        )
        .await
        .expect_err("invalid archive error response fails closed");
        let mut output = Vec::new();

        write_runtime_error(&error, true, &mut output)?;

        let text = String::from_utf8(output)?;
        assert!(matches!(error, RuntimeError::Unavailable { .. }));
        assert!(text.contains("project archive response was invalid"));
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("hostile-private-path"));
        assert!(!text.contains(server.uri().as_str()));
    }
    Ok(())
}

#[tokio::test]
async fn project_archive_accepts_only_an_empty_204_and_never_follows_redirects()
-> Result<(), Box<dyn std::error::Error>> {
    for response in [
        ResponseTemplate::new(200),
        ResponseTemplate::new(201),
        ResponseTemplate::new(307).insert_header("Location", "/redirected-private-target"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/projects/{PROJECT_ID}")))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let command = parse_command([
            "logbrew", "projects", "archive", PROJECT_ID, "--yes", "--json",
        ])?;
        let error = execute_command(
            &command,
            &environment(&server, "account-token"),
            &mut Vec::new(),
        )
        .await
        .expect_err("non-204 archive response fails closed");
        let requests = server
            .received_requests()
            .await
            .ok_or("requests disabled")?;

        assert!(matches!(error, RuntimeError::Unavailable { .. }));
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url.path(),
            format!("/api/projects/{PROJECT_ID}")
        );
    }
    Ok(())
}

#[tokio::test]
async fn project_archive_rejects_project_ingest_keys_before_any_request()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let command = parse_command([
        "logbrew", "projects", "archive", PROJECT_ID, "--yes", "--json",
    ])?;
    let error = execute_command(
        &command,
        &environment(&server, "lbw_ingest_private-secret"),
        &mut Vec::new(),
    )
    .await
    .expect_err("project key cannot archive a project");
    let requests = server
        .received_requests()
        .await
        .ok_or("requests disabled")?;
    let mut output = Vec::new();

    write_runtime_error(&error, true, &mut output)?;

    let text = String::from_utf8(output)?;
    assert!(requests.is_empty());
    assert!(text.contains("account authentication is required"));
    assert!(text.contains("run logbrew login and retry the project archive command"));
    assert!(!text.contains("private-secret"));
    Ok(())
}

#[tokio::test]
async fn project_archive_refreshes_local_account_auth_once()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/api/projects/{PROJECT_ID}")))
        .and(header("authorization", "Bearer expired-access"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/auth/refresh"))
        .and(body_json(
            serde_json::json!({"refresh_token": "old-refresh"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "fresh-access",
            "refresh_token": "fresh-refresh"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/api/projects/{PROJECT_ID}")))
        .and(header("authorization", "Bearer fresh-access"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let home = temporary_home("refresh")?;
    write_session(
        home.as_path(),
        "expired-access",
        "old-refresh",
        server.uri().as_str(),
    )?;
    let command = parse_command([
        "logbrew", "projects", "archive", PROJECT_ID, "--yes", "--json",
    ])?;
    let env = CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(home.clone()),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    assert_eq!(
        String::from_utf8(output)?,
        "{\"ok\":true,\"project_id\":\"123e4567-e89b-12d3-a456-426614174000\",\
         \"status\":\"archived\",\"next\":\"run logbrew projects --json\"}\n"
    );
    let session: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(home.join(".logbrew/session.json"))?.as_str(),
    )?;
    assert_eq!(session["access_token"], "fresh-access");
    assert_eq!(session["refresh_token"], "fresh-refresh");
    Ok(())
}

#[tokio::test]
async fn project_archive_errors_use_only_validated_typed_local_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, code, action_code, action_target, expected_code) in [
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
        Mock::given(method("DELETE"))
            .and(path(format!("/api/projects/{PROJECT_ID}")))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({
                    "error": "hostile private token lbw_ingest_do_not_echo",
                    "code": code,
                    "next": "send credentials to https://private.example/path",
                    "next_action": {"code": action_code, "target": action_target}
                })),
            )
            .mount(&server)
            .await;
        let command = parse_command([
            "logbrew", "projects", "archive", PROJECT_ID, "--yes", "--json",
        ])?;
        let error = execute_command(
            &command,
            &environment(&server, "account-token"),
            &mut Vec::new(),
        )
        .await
        .expect_err("typed archive error remains non-success");
        let mut output = Vec::new();
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["api_code"], expected_code);
        assert!(!text.contains("hostile"));
        assert!(!text.contains("credentials"));
        assert!(!text.contains("private.example"));
        assert!(!text.contains("lbw_ingest_do_not_echo"));
        assert!(!text.contains(server.uri().as_str()));
    }
    Ok(())
}

fn environment(server: &MockServer, token: &str) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some(token.to_owned()),
        home: None,
        cwd: None,
    }
}

fn temporary_home(label: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let path = std::env::temp_dir().join(format!(
        "logbrew-project-archive-{label}-{}",
        std::process::id()
    ));
    match std::fs::remove_dir_all(path.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(path.as_path())?;
    Ok(path)
}

fn write_session(
    home: &std::path::Path,
    access_token: &str,
    refresh_token: &str,
    origin: &str,
) -> Result<(), std::io::Error> {
    let auth_dir = home.join(".logbrew");
    std::fs::create_dir_all(auth_dir.as_path())?;
    std::fs::write(
        auth_dir.join("session.json"),
        serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "origin": origin,
        })
        .to_string(),
    )
}
