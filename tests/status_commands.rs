//! CLI status command output tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command, write_runtime_error};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn status_json_reports_api_and_missing_auth_for_agents() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "status", "--json"]).expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-status-json-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("status succeeds");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "reachable");
    assert_eq!(body["status_code"], 200);
    assert_eq!(body["body"], "ok");
    assert_eq!(body["api_url"], server.uri());
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["auth_source"], "missing");
    assert_eq!(body["next"], "run logbrew login");
    assert!(body.get("agent_use").is_none());
}

#[tokio::test]
async fn status_json_reports_env_auth_without_exposing_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/auth/account"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001"
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "status", "--json"]).expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some("fixture-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-status-env-auth-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("status succeeds");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
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
}

#[tokio::test]
async fn status_json_reports_expired_token_as_unauthenticated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/auth/account"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "code": "unauthorized",
            "error": "Invalid or expired token",
            "next_action": {
                "code": "sign_in",
                "target": "auth"
            }
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "status", "--json"]).expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some("expired-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-status-expired-auth-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("status succeeds");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "reachable");
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["auth_source"], "expired");
    assert_eq!(body["next"], "run logbrew login");
    assert!(body.get("agent_use").is_none());
    assert!(!body.to_string().contains("expired-token"));
}

#[tokio::test]
async fn status_refreshes_expired_local_auth_before_reporting_authenticated()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/auth/account"))
        .and(header("authorization", "Bearer expired-local"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/auth/refresh"))
        .and(body_json(serde_json::json!({
            "refresh_token": "old-local-refresh"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "fresh-local",
            "refresh_token": "fresh-local-refresh"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/auth/account"))
        .and(header("authorization", "Bearer fresh-local"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let home = status_home("refresh-local")?;
    let auth_dir = home.join(".logbrew");
    std::fs::create_dir_all(auth_dir.as_path())?;
    std::fs::write(
        auth_dir.join("session.json"),
        serde_json::json!({
            "access_token": "expired-local",
            "refresh_token": "old-local-refresh",
            "origin": server.uri(),
        })
        .to_string(),
    )?;
    let command = parse_command(["logbrew", "status", "--json"])?;
    let env = CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(home),
        cwd: None,
    };
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
}

#[tokio::test]
async fn status_human_output_includes_api_and_auth_next_step() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "status"]).expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-status-human-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("status succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        format!(
            "LogBrew API reachable.\nAPI: {}\nAuth: not logged in\nNext: run logbrew login\n",
            server.uri()
        )
    );
}

fn status_home(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let home =
        std::env::temp_dir().join(format!("logbrew-cli-status-{name}-{}", std::process::id()));
    match std::fs::remove_dir_all(home.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(home.as_path())?;
    Ok(home)
}

#[tokio::test]
async fn status_human_authenticated_output_points_to_first_read_without_leaking_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/auth/account"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001"
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "status"]).expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some("fixture-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-status-human-auth-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("status succeeds");

    let text = String::from_utf8(output).expect("utf8 output");
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
}

#[tokio::test]
async fn status_json_reports_unreachable_api_without_exposing_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(503).set_body_string("maintenance"))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "status", "--json"]).expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some("fixture-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-status-json-down-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("status fails");
    write_runtime_error(&error, command.wants_json(), &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
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
}

#[tokio::test]
async fn status_human_reports_unreachable_api_with_next_step() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(503).set_body_string("maintenance"))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "status"]).expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(std::env::temp_dir().join("logbrew-status-human-down-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("status fails");
    write_runtime_error(&error, command.wants_json(), &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        format!(
            "LogBrew API unreachable.\nAPI: {}\nAuth: not logged in\nStatus: 503\nBody: \
             maintenance\nNext: check LOGBREW_API_URL or network\n",
            server.uri()
        )
    );
}
