//! Authenticated project catalog contract tests.

use crate::matchers::body_json;
use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command, rendered_json_error};
use logbrew_cli::{
    CliEnvironment, Command, HttpMethod, RuntimeError, parse_command, write_runtime_error,
};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn project_aliases_and_json_order_parse_as_authenticated_reads() -> TestResult {
    for (args, json) in [
        (&["logbrew", "projects"][..], false),
        (&["logbrew", "project"][..], false),
        (&["logbrew", "projects", "--json"][..], true),
        (&["logbrew", "project", "--json"][..], true),
        (&["logbrew", "--json", "projects"][..], true),
        (&["logbrew", "--json", "project"][..], true),
    ] {
        let command = parse_command(args.iter().copied())?;

        assert!(
            !matches!(command, Command::Help { .. }),
            "project catalog must execute instead of returning help"
        );
        assert_eq!(command.http_path().as_deref(), Some("/api/projects"));
        assert_eq!(command.http_method(), Some(HttpMethod::Get));
        assert_eq!(command.wants_json(), json);
        assert!(command.request_body().is_none());
    }
    Ok(())
}

#[test]
fn project_catalog_grammar_failures_are_fixed_and_value_safe() -> TestResult {
    for args in [
        [
            "logbrew",
            "projects",
            "--authorization=hostile-secret\ncontrol",
            "--json",
        ],
        ["logbrew", "project", "hostile-private-value", "--json"],
        ["logbrew", "projects", "--json", "--json"],
    ] {
        let (body, text) = rendered_json_error(&args)?;

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
    Ok(())
}

async_test!(projects_json_preserves_exact_bare_array_and_sends_no_query -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await?;
    let response = serde_json::to_string_pretty(&project_catalog())?;
    Mock::auth("GET", "/api/projects", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_raw(response.clone(), "application/json"))
        .expect(2)
        .mount(&server);

    for alias in ["projects", "project"] {
        let command = parse_command(["logbrew", alias, "--json"])?;
        let mut output = Vec::new();
        execute_command(&command, &environment(&server), &mut output).await?;

        assert_eq!(String::from_utf8(output)?, format!("{response}\n"));
    }
    let requests = server.received_requests();
    assert!(
        requests
            .iter()
            .all(|request| request.url.query().is_none() && request.body.is_empty())
    );
    Ok(())
});

async_test!(projects_human_output_is_bounded_and_scan_oriented -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/api/projects")
        .respond_with(ResponseTemplate::new(200).set_body_json(&project_catalog()))
        .mount(&server);
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
});

async_test!(projects_rejects_envelopes_partial_rows_and_hostile_text -> Result<(), Box<dyn std::error::Error>>, {
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
        let server = MockServer::start().await?;
        Mock::route("GET", "/api/projects")
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .mount(&server);
        let command = parse_command(["logbrew", "projects", "--json"])?;
        let mut data = Vec::new();
        let error = execute_command(&command, &environment(&server), &mut data)
            .await
            .err()
            .ok_or("malformed project catalog must fail closed")?;
        assert!(data.is_empty(), "invalid catalog case {index} leaked output");
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
});

async_test!(projects_refreshes_local_account_auth_once -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/projects", "expired-access")
        .respond_with(ResponseTemplate::new(401).set_body_json(&serde_json::json!({
            "error": "Invalid or expired token",
            "code": "unauthorized",
            "next": "sign in again",
            "next_action": {"code": "sign_in", "target": "auth"}
        })))
        .expect(1)
        .mount(&server);
    Mock::route("POST", "/api/auth/refresh")
        .and(body_json(
            serde_json::json!({"refresh_token": "old-refresh"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "access_token": "fresh-access",
            "refresh_token": "fresh-refresh"
        })))
        .expect(1)
        .mount(&server);
    Mock::auth("GET", "/api/projects", "fresh-access")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!([])))
        .expect(1)
        .mount(&server);
    let home = super::isolated_home("logbrew-projects", "refresh")?;
    let session_path = super::write_test_session(
        home.as_path(),
        server.uri().as_str(),
        "expired-access",
        "old-refresh",
    )?;
    let command = parse_command(["logbrew", "projects", "--json"])?;
    let env = super::test_env(&server, None, Some(home));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    assert_eq!(String::from_utf8(output)?, "[]\n");
    let session: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(session_path)?)?;
    assert_eq!(session["access_token"], "fresh-access");
    assert_eq!(session["refresh_token"], "fresh-refresh");
    assert_eq!(session["origin"], server.uri());
    Ok(())
});

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

async_test!(projects_with_a_read_key_names_its_project_and_points_to_reads -> TestResult, {
    const READ_KEY: &str = "lbw_read_fixture-catalog-key";
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/projects", READ_KEY)
        .respond_with(ResponseTemplate::new(403).set_body_json(&serde_json::json!({
            "code": "forbidden", "error": "Forbidden"
        })))
        .mount(&server);
    Mock::auth("GET", "/api/auth/read-key", READ_KEY)
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "kind": "read", "project_id": PROJECT_ID, "project_name": "checkout-web"
        })))
        .mount(&server);
    let env = super::authenticated_env(&server, READ_KEY, None);

    for json in [true, false] {
        let args: &[&str] = if json { &["logbrew", "projects", "--json"] } else { &["logbrew", "projects"] };
        let command = parse_command(args.iter().copied())?;
        let error = crate::failed_command(&command, &env).await?;
        let mut output = Vec::new();
        write_runtime_error(&error, json, &mut output)?;
        let text = String::from_utf8(output)?;
        assert!(!text.contains("response was invalid"), "{text}");
        assert!(text.contains("read key"), "{text}");
        assert!(text.contains("checkout-web") && text.contains(PROJECT_ID), "{text}");
        assert!(text.contains("logbrew issues or logbrew read traces"), "{text}");
        assert!(!text.contains(READ_KEY) && !text.contains("logged in"), "{text}");
    }
    Ok(())
});

async_test!(projects_with_an_invalid_read_key_points_to_a_new_key -> TestResult, {
    const READ_KEY: &str = "lbw_read_fixture-revoked-key";
    let server = MockServer::start().await?;
    Mock::auth("GET", "/api/auth/read-key", READ_KEY)
        .respond_with(ResponseTemplate::new(401).set_body_json(&serde_json::json!({
            "code": "unauthorized", "error": "Invalid or expired token", "next": READ_KEY,
            "next_action": {"code": "authorize_request", "target": "auth"}
        })))
        .mount(&server);
    let env = super::authenticated_env(&server, READ_KEY, None);
    let command = parse_command(["logbrew", "projects"])?;
    let error = crate::failed_command(&command, &env).await?;
    for json in [true, false] {
        let mut output = Vec::new();
        write_runtime_error(&error, json, &mut output)?;
        let text = String::from_utf8(output)?;
        assert!(text.contains("create a new read key"), "{text}");
        assert!(!text.contains("logbrew login") && !text.contains(READ_KEY), "{text}");
    }
    Ok(())
});
