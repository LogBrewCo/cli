//! CLI status command output tests.

use crate::matchers::body_json;
use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command};
use logbrew_cli::{CliEnvironment, parse_command, write_runtime_error};

async_test!(status_json_reports_api_and_missing_auth_for_agents -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    let command = parse_command(["logbrew", "status", "--json"])?;
    let env = environment(&server, None, "json-test");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "reachable");
    assert_eq!(body["status_code"], 200);
    assert_eq!(body["body"], "ok");
    assert_eq!(body["api_url"], server.uri());
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["auth_source"], "missing");
    assert_eq!(body["next"], "run logbrew login");
    assert!(body.get("agent_use").is_none());
    Ok(())
});

async_test!(status_json_reports_env_auth_without_exposing_token -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    Mock::route("GET", "/api/auth/account")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001"
        })))
        .mount(&server);
    let command = parse_command(["logbrew", "status", "--json"])?;
    let env = environment(&server, Some("fixture-token"), "env-auth-test");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["auth_source"], "env");
    assert_eq!(
        body["next"],
        "run logbrew releases or logbrew logs --release <release> --environment <environment>"
    );
    assert_eq!(
        body["agent_use"]["prompt"],
        "How should your AI use LogBrew?"
    );
    assert_eq!(body["agent_use"]["default"], "on_request");
    assert_eq!(body["agent_use"]["options"][0]["id"], "on_request");
    assert_eq!(body["agent_use"]["options"][0]["token_use"], "lower");
    assert_eq!(body["agent_use"]["options"][0]["available"], true);
    assert_eq!(
        body["agent_use"]["options"][0]["description"],
        "Your AI runs LogBrew commands when you ask."
    );
    assert_eq!(body["agent_use"]["options"][1]["id"], "keep_watching");
    assert_eq!(body["agent_use"]["options"][1]["token_use"], "higher");
    assert_eq!(body["agent_use"]["options"][1]["available"], true);
    assert_eq!(
        body["agent_use"]["options"][1]["description"],
        "Your AI watches new events/logs until stopped."
    );
    assert_eq!(
        body["agent_use"]["options"][1]["command"],
        "logbrew watch --json"
    );
    assert_eq!(
        body["agent_use"]["options"][2]["id"],
        "watch_errors_critical"
    );
    assert_eq!(body["agent_use"]["options"][2]["token_use"], "moderate");
    assert_eq!(body["agent_use"]["options"][2]["available"], true);
    assert_eq!(
        body["agent_use"]["options"][2]["description"],
        "Your AI ignores lower-severity logs/events."
    );
    assert_eq!(
        body["agent_use"]["options"][2]["command"],
        "logbrew watch --severity error,critical --json"
    );
    assert!(!body.to_string().contains("fixture-token"));
    Ok(())
});

async_test!(status_json_reports_expired_token_as_unauthenticated -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    Mock::route("GET", "/api/auth/account")
        .respond_with(ResponseTemplate::new(401).set_body_json(&serde_json::json!({
            "code": "unauthorized",
            "error": "Invalid or expired token",
            "next_action": {
                "code": "sign_in",
                "target": "auth"
            }
        })))
        .mount(&server);
    let command = parse_command(["logbrew", "status", "--json"])?;
    let env = environment(&server, Some("expired-token"), "expired-auth-test");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "reachable");
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["auth_source"], "expired");
    assert_eq!(body["next"], "run logbrew login");
    assert!(body.get("agent_use").is_none());
    assert!(!body.to_string().contains("expired-token"));
    Ok(())
});

const READ_KEY: &str = "lbw_read_fixture-read-key";
const READ_PROJECT: &str = "2f769098-3aef-4f85-808b-c48223ec2c0b";

async_test!(status_names_the_project_a_valid_read_key_reads -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    Mock::auth("GET", "/api/auth/read-key", READ_KEY)
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "kind": "read",
            "project_id": READ_PROJECT,
            "project_name": "checkout-web"
        })))
        .expect(2)
        .mount(&server);
    let env = environment(&server, Some(READ_KEY), "read-key-test");

    let mut output = Vec::new();
    execute_command(&parse_command(["logbrew", "status", "--json"])?, &env, &mut output).await?;
    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["auth_source"], "env");
    assert_eq!(body["credential"], "read_key");
    assert_eq!(body["project_id"], READ_PROJECT);
    assert_eq!(body["project_name"], "checkout-web");
    assert_eq!(body["next"], "run logbrew issues or logbrew read traces");
    assert!(body.get("agent_use").is_none(), "a read key cannot watch");
    assert!(!text.contains(READ_KEY));

    let mut output = Vec::new();
    execute_command(&parse_command(["logbrew", "status"])?, &env, &mut output).await?;
    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        format!(
            "LogBrew API reachable.\nAPI: {}\nAuth: read key for project checkout-web \
             ({READ_PROJECT}) (env token)\nNext: run logbrew issues or logbrew read traces\n",
            server.uri()
        )
    );
    Ok(())
});

async_test!(status_sends_an_invalid_read_key_to_a_new_key_not_login -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    Mock::auth("GET", "/api/auth/read-key", READ_KEY)
        .respond_with(ResponseTemplate::new(401).set_body_json(&serde_json::json!({
            "code": "unauthorized",
            "error": "Invalid or expired token"
        })))
        .mount(&server);
    let env = environment(&server, Some(READ_KEY), "invalid-read-key-test");
    let mut output = Vec::new();

    execute_command(&parse_command(["logbrew", "status", "--json"])?, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["auth_source"], "invalid_read_key");
    assert_eq!(
        body["next"],
        "create a new read key with logbrew projects keys create <project_id> --kind read, \
         then set LOGBREW_TOKEN to it"
    );
    assert!(!text.contains(READ_KEY));
    Ok(())
});

async_test!(status_rejects_a_malformed_read_key_scope -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    Mock::auth("GET", "/api/auth/read-key", READ_KEY)
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "kind": "read",
            "project_id": "not-a-uuid",
            "project_name": READ_KEY
        })))
        .mount(&server);
    let env = environment(&server, Some(READ_KEY), "malformed-read-key-test");
    let mut output = Vec::new();

    let error = execute_command(&parse_command(["logbrew", "status", "--json"])?, &env, &mut output)
        .await
        .err()
        .ok_or("a malformed scope must fail")?;
    write_runtime_error(&error, true, &mut output)?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("\"ok\":false"), "{text}");
    assert!(!text.contains(READ_KEY), "{text}");
    Ok(())
});

async_test!(status_refreshes_expired_local_auth_before_reporting_authenticated -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    Mock::auth("GET", "/api/auth/account", "expired-local")
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server);
    Mock::route("POST", "/api/auth/refresh")
        .and(body_json(serde_json::json!({
            "refresh_token": "old-local-refresh"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "access_token": "fresh-local",
            "refresh_token": "fresh-local-refresh"
        })))
        .expect(1)
        .mount(&server);
    Mock::auth("GET", "/api/auth/account", "fresh-local")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001"
        })))
        .expect(1)
        .mount(&server);
    let home = crate::isolated_home("logbrew-cli-status", "refresh-local")?;
    let _session_path =
        crate::write_test_session(&home, &server.uri(), "expired-local", "old-local-refresh")?;
    let command = parse_command(["logbrew", "status", "--json"])?;
    let env = super::test_env(&server, None, Some(home));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["auth_source"], "token_file");
    for secret in [
        "expired-local",
        "old-local-refresh",
        "fresh-local",
        "fresh-local-refresh",
    ] {
        assert!(!text.contains(secret));
    }
    Ok(())
});

async_test!(status_human_output_includes_api_and_auth_next_step -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    let command = parse_command(["logbrew", "status"])?;
    let env = environment(&server, None, "human-test");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        format!(
            "LogBrew API reachable.\nAPI: {}\nAuth: not logged in\nNext: run logbrew login\n",
            server.uri()
        )
    );
    Ok(())
});

async_test!(
    status_human_authenticated_output_points_to_first_read_without_leaking_token -> TestResult,
    {
        let server = MockServer::start().await?;
        Mock::route("GET", "/health")
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server);
        Mock::route("GET", "/api/auth/account")
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
                "id": "00000000-0000-4000-8000-000000000001"
            })))
            .mount(&server);
        let command = parse_command(["logbrew", "status"])?;
        let env = environment(&server, Some("fixture-token"), "human-auth-test");
        let mut output = Vec::new();

        execute_command(&command, &env, &mut output).await?;

        let text = String::from_utf8(output)?;
        assert_eq!(
            text,
            format!(
                "LogBrew API reachable.\nAPI: {}\nAuth: logged in (env token)\nLogBrew is \
                 connected. How should your AI use it?\n\n1. Check only when requested\n   Lower \
                 token use. Your AI runs LogBrew commands when you ask.\n\n2. Keep watching this \
                 session\n   Higher token use. Your AI watches new events/logs until stopped.\n   \
                 Command: logbrew watch --json\n\n3. Watch only errors and critical issues\n   \
                 Moderate token use. Your AI ignores lower-severity logs/events.\n   Command: \
                 logbrew watch --severity error,critical --json\n\nNext: run logbrew releases or \
                 logbrew logs --release <release> --environment <environment>\n",
                server.uri()
            )
        );
        assert!(!text.contains("fixture-token"));
        Ok(())
    }
);

async_test!(
    status_json_reports_unreachable_api_without_exposing_token -> TestResult,
    {
        let server = MockServer::start().await?;
        Mock::route("GET", "/health")
            .respond_with(ResponseTemplate::new(503).set_body_string("maintenance"))
            .mount(&server);
        let command = parse_command(["logbrew", "status", "--json"])?;
        let env = environment(&server, Some("fixture-token"), "json-down-test");
        let mut output = Vec::new();

        let error = execute_command(&command, &env, &mut output)
            .await
            .err()
            .ok_or("status must fail")?;
        write_runtime_error(&error, command.wants_json(), &mut output)?;

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice())?;
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "status_unreachable");
        assert_eq!(body["status"], "unreachable");
        assert_eq!(body["status_code"], 503);
        assert_eq!(body["body"], "maintenance");
        assert_eq!(body["api_url"], server.uri());
        assert_eq!(body["authenticated"], true);
        assert_eq!(body["auth_source"], "env");
        assert_eq!(body["next"], "check LOGBREW_API_URL or network");
        assert!(!body.to_string().contains("fixture-token"));
        Ok(())
    }
);

async_test!(status_human_reports_unreachable_api_with_next_step -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(503).set_body_string("maintenance"))
        .mount(&server);
    let command = parse_command(["logbrew", "status"])?;
    let env = environment(&server, None, "human-down-test");
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .err()
        .ok_or("status must fail")?;
    write_runtime_error(&error, command.wants_json(), &mut output)?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        format!(
            "LogBrew API unreachable.\nAPI: {}\nAuth: not logged in\nStatus: 503\nBody: \
                 maintenance\nNext: check LOGBREW_API_URL or network\n",
            server.uri()
        )
    );
    Ok(())
});

fn environment(server: &MockServer, token: Option<&str>, name: &str) -> CliEnvironment {
    super::test_env(
        server,
        token,
        Some(std::env::temp_dir().join(format!("logbrew-status-{name}"))),
    )
}
