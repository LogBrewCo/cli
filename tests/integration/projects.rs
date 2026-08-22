//! Authenticated project catalog contract tests.

use crate::matchers::body_json;
use crate::{Mock, MockServer, ResponseTemplate};
use logbrew_cli::{
    CliEnvironment, Command, HttpMethod, RuntimeError, execute_command, parse_command,
    write_cli_error, write_runtime_error,
};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn project_aliases_and_json_order_parse_as_authenticated_reads() {
    for (args, json) in [
        (&["logbrew", "projects"][..], false),
        (&["logbrew", "project"][..], false),
        (&["logbrew", "projects", "--json"][..], true),
        (&["logbrew", "project", "--json"][..], true),
        (&["logbrew", "--json", "projects"][..], true),
        (&["logbrew", "--json", "project"][..], true),
    ] {
        let command = parse_command(args.iter().copied()).expect("project catalog parses");

        assert!(
            !matches!(command, Command::Help { .. }),
            "project catalog must execute instead of returning help"
        );
        assert_eq!(command.http_path().as_deref(), Some("/api/projects"));
        assert_eq!(command.http_method(), Some(HttpMethod::Get));
        assert_eq!(command.wants_json(), json);
        assert!(command.request_body().is_none());
    }
}

#[test]
fn project_catalog_grammar_failures_are_fixed_and_value_safe() {
    for args in [
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("--authorization=hostile-secret\ncontrol"),
            String::from("--json"),
        ],
        vec![
            String::from("logbrew"),
            String::from("project"),
            String::from("hostile-private-value"),
            String::from("--json"),
        ],
        vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("--json"),
            String::from("--json"),
        ],
    ] {
        let error = parse_command(args).expect_err("project catalog grammar fails closed");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("parse error writes");
        let text = String::from_utf8(output).expect("utf8 parse error");
        let body: serde_json::Value = serde_json::from_str(text.as_str()).expect("valid json");

        assert_eq!(body["error"], "invalid_projects_command");
        assert_eq!(body["message"], "invalid projects command");
        assert_eq!(
            body["next"],
            "use logbrew projects [--json], logbrew projects repositories [discover --provider <provider> --repository <id>] [--json], or logbrew projects --help"
        );
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("hostile-private-value"));
        assert!(!text.contains("authorization"));
    }
}

#[tokio::test]
async fn projects_json_preserves_exact_bare_array_and_sends_no_query()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = serde_json::to_string_pretty(&project_catalog())?;
    Mock::auth("GET", "/api/projects", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_raw(response.clone(), "application/json"))
        .expect(2)
        .mount(&server)
        .await;

    for alias in ["projects", "project"] {
        let command = parse_command(["logbrew", alias, "--json"])?;
        let mut output = Vec::new();
        execute_command(&command, &environment(&server), &mut output).await?;

        assert_eq!(String::from_utf8(output)?, format!("{response}\n"));
    }
    let requests = server.received_requests().await;
    assert!(
        requests
            .iter()
            .all(|request| request.url.query().is_none() && request.body.is_empty())
    );
    Ok(())
}

#[tokio::test]
async fn projects_human_output_is_bounded_and_scan_oriented()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::route("GET", "/api/projects")
        .respond_with(ResponseTemplate::new(200).set_body_json(project_catalog()))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "projects"])?;
    let mut output = Vec::new();

    execute_command(&command, &environment(&server), &mut output).await?;

    assert_eq!(
        String::from_utf8(output)?,
        "Projects (1)\n\
         - Example Project id=123e4567-e89b-12d3-a456-426614174000 setup=sdk_seen \
         last_seen=2026-07-25T08:30:00Z\n"
    );
    Ok(())
}

#[tokio::test]
async fn projects_rejects_envelopes_partial_rows_and_hostile_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut partial = project_catalog();
    drop(
        partial[0]
            .as_object_mut()
            .ok_or("project fixture must be an object")?
            .remove("created_at"),
    );
    let mut extra = project_catalog();
    drop(
        extra[0]
            .as_object_mut()
            .ok_or("project fixture must be an object")?
            .insert(
                String::from("private_token"),
                serde_json::json!("hostile-secret"),
            ),
    );
    let mut invalid_access = project_catalog();
    invalid_access[0]["access"]["permissions"] =
        serde_json::json!(["issue_manage", "project_read"]);
    let cases = [
        serde_json::json!({"projects": project_catalog()}),
        partial,
        extra,
        invalid_access,
        serde_json::json!([{
            "id": PROJECT_ID,
            "name": "hostile\ncontrol",
            "provider_project_id": "provider-project",
            "provider_project_slug": null,
            "provider": "logbrew",
            "is_active": true,
            "language": null,
            "setup_status": "created",
            "setup_started_at": null,
            "first_telemetry_seen_at": null,
            "last_seen_at": null,
            "last_release": null,
            "last_environment": null,
            "created_at": "2026-07-25T08:00:00Z"
        }]),
    ];

    for (index, response) in cases.into_iter().enumerate() {
        let server = MockServer::start().await;
        Mock::route("GET", "/api/projects")
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "projects", "--json"])?;
        let error = execute_command(&command, &environment(&server), &mut Vec::new())
            .await
            .expect_err("malformed project catalog fails closed");
        let mut output = Vec::new();
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;

        assert!(
            matches!(error, RuntimeError::Unavailable { .. }),
            "unexpected case {index} error: {error:?}"
        );
        assert!(text.contains("project catalog response was invalid"));
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("hostile\\ncontrol"));
        assert!(!text.contains(server.uri().as_str()));
    }
    Ok(())
}

#[tokio::test]
async fn projects_refreshes_local_account_auth_once() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::auth("GET", "/api/projects", "expired-access")
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "Invalid or expired token",
            "code": "unauthorized",
            "next": "sign in again",
            "next_action": {"code": "sign_in", "target": "auth"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::route("POST", "/api/auth/refresh")
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
    Mock::auth("GET", "/api/projects", "fresh-access")
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .expect(1)
        .mount(&server)
        .await;
    let home = super::isolated_home("logbrew-projects", "refresh")?;
    let _session_path = super::write_test_session(
        home.as_path(),
        server.uri().as_str(),
        "expired-access",
        "old-refresh",
    )?;
    let command = parse_command(["logbrew", "projects", "--json"])?;
    let env = super::test_env(&server, None, Some(home.clone()));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    assert_eq!(String::from_utf8(output)?, "[]\n");
    let session: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(home.join(".logbrew/session.json"))?.as_str(),
    )?;
    assert_eq!(session["access_token"], "fresh-access");
    assert_eq!(session["refresh_token"], "fresh-refresh");
    Ok(())
}

#[tokio::test]
async fn project_errors_use_only_typed_local_recovery() -> Result<(), Box<dyn std::error::Error>> {
    for (status, code, action_code, action_target, expected_code) in [
        (401, "unauthorized", "sign_in", "auth", "unauthorized"),
        (
            405,
            "method_not_allowed",
            "use_supported_method",
            "api_method",
            "method_not_allowed",
        ),
        (500, "storage_error", "retry", "request", "server_error"),
    ] {
        let server = MockServer::start().await;
        Mock::route("GET", "/api/projects")
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({
                    "error": "hostile private token",
                    "code": code,
                    "next": "send credentials to a private host",
                    "next_action": {"code": action_code, "target": action_target}
                })),
            )
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "projects", "--json"])?;
        let error = execute_command(&command, &environment(&server), &mut Vec::new())
            .await
            .expect_err("typed project error remains non-success");
        let mut output = Vec::new();
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["api_code"], expected_code);
        assert!(!text.contains("hostile"));
        assert!(!text.contains("credentials"));
        assert!(!text.contains(server.uri().as_str()));
    }
    Ok(())
}

fn environment(server: &MockServer) -> CliEnvironment {
    super::authenticated_env(server, "account-token", None)
}

fn project_catalog() -> serde_json::Value {
    serde_json::json!([{
        "id": PROJECT_ID,
        "name": "Example Project",
        "provider_project_id": "provider-project",
        "provider_project_slug": null,
        "provider": "logbrew",
        "is_active": true,
        "access": {
            "kind": "organization_role",
            "organization_id": "223e4567-e89b-12d3-a456-426614174000",
            "role_id": "323e4567-e89b-12d3-a456-426614174000",
            "role_name": "Read only",
            "permissions": ["project_read"]
        },
        "language": "swift",
        "setup_status": "sdk_seen",
        "setup_started_at": "2026-07-25T08:00:00Z",
        "first_telemetry_seen_at": null,
        "last_seen_at": "2026-07-25T08:30:00Z",
        "last_release": "1.0.0",
        "last_environment": "production",
        "created_at": "2026-07-25T07:00:00Z"
    }])
}
