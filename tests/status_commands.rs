//! CLI status command output tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command, write_runtime_error};
use wiremock::matchers::{method, path};
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
}

#[tokio::test]
async fn status_json_reports_env_auth_without_exposing_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
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
    assert!(!body.to_string().contains("fixture-token"));
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

#[tokio::test]
async fn status_human_authenticated_output_points_to_first_read_without_leaking_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
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
            "LogBrew API reachable.\nAPI: {}\nAuth: logged in (env token)\nNext: run logbrew \
             releases or logbrew logs --release <release> --environment <environment>\n",
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
