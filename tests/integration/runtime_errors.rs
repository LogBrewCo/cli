//! CLI runtime error rendering tests.

use logbrew_cli::{
    CliEnvironment, RuntimeError, execute_command, parse_command, write_runtime_error,
};

#[test]
fn writes_runtime_errors_as_json_for_agents() {
    let mut output = Vec::new();

    write_runtime_error(&RuntimeError::MissingToken, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "not_logged_in");
    assert_eq!(body["message"], "not logged in: run logbrew login");
    assert_eq!(body["next"], "run logbrew login");
}

#[test]
fn writes_io_errors_as_json_with_local_next_step() {
    let mut output = Vec::new();
    let error = RuntimeError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));

    write_runtime_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "io_error");
    assert_eq!(body["next"], "check local files and permissions");
}

#[tokio::test]
async fn writes_http_errors_as_json_with_network_next_step() {
    let command = parse_command(["logbrew", "logs", "--json"]).expect("command parses");
    let env = CliEnvironment {
        base_url: "http://127.0.0.1:1".to_owned(),
        token: Some("test-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-http-error-home")),
        cwd: None,
    };
    let mut command_output = Vec::new();
    let error = execute_command(&command, &env, &mut command_output)
        .await
        .expect_err("connection failure is an http error");
    let mut error_output = Vec::new();

    write_runtime_error(&error, command.wants_json(), &mut error_output).expect("error writes");

    let body: serde_json::Value =
        serde_json::from_slice(error_output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "http_error");
    assert_eq!(body["next"], "check LOGBREW_API_URL or network");
}

#[test]
fn writes_missing_token_errors_with_human_next_step() {
    let mut output = Vec::new();

    write_runtime_error(&RuntimeError::MissingToken, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "not logged in: run logbrew login\nNext: run logbrew login\n"
    );
}

#[test]
fn writes_api_auth_errors_as_json_with_login_next_step() {
    let mut output = Vec::new();
    let error = RuntimeError::Api {
        status: 401,
        body: String::from(r#"{"ok":false,"error":"not_logged_in"}"#),
        auth_source: "token_file",
        auth_label: "logged in (local token)",
    };

    write_runtime_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "api_error");
    assert_eq!(body["status"], 401);
    assert_eq!(body["auth_source"], "token_file");
    assert_eq!(body["next"], "run logbrew login");
}

#[test]
fn writes_api_auth_errors_with_human_next_step() {
    let mut output = Vec::new();
    let error = RuntimeError::Api {
        status: 403,
        body: String::from(r#"{"ok":false,"error":"forbidden"}"#),
        auth_source: "env",
        auth_label: "logged in (env token)",
    };

    write_runtime_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "api returned status 403: {\"ok\":false,\"error\":\"forbidden\"}\nAuth: logged in (env \
         token)\nNext: run logbrew login\n"
    );
}

#[test]
fn writes_api_server_errors_as_json_with_retry_next_step() {
    let mut output = Vec::new();
    let error = RuntimeError::Api {
        status: 500,
        body: String::from(r#"{"ok":false,"error":"internal"}"#),
        auth_source: "token_file",
        auth_label: "logged in (local token)",
    };

    write_runtime_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "api_error");
    assert_eq!(body["status"], 500);
    assert_eq!(body["auth_source"], "token_file");
    assert_eq!(body["next"], "check LOGBREW_API_URL or retry later");
}

#[test]
fn writes_api_rate_limit_errors_with_retry_next_step() {
    let error = RuntimeError::Api {
        status: 429,
        body: String::from(r#"{"ok":false,"error":"rate_limited"}"#),
        auth_source: "token_file",
        auth_label: "logged in (local token)",
    };
    let mut json_output = Vec::new();
    let mut text_output = Vec::new();

    write_runtime_error(&error, true, &mut json_output).expect("json error writes");
    write_runtime_error(&error, false, &mut text_output).expect("human error writes");

    let body: serde_json::Value =
        serde_json::from_slice(json_output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "api_error");
    assert_eq!(body["status"], 429);
    assert_eq!(body["auth_source"], "token_file");
    assert_eq!(body["next"], "retry later");

    let text = String::from_utf8(text_output).expect("utf8 output");
    assert_eq!(
        text,
        "api returned status 429: {\"ok\":false,\"error\":\"rate_limited\"}\nAuth: logged in \
         (local token)\nNext: retry later\n",
    );
}

#[test]
fn writes_api_not_found_errors_with_human_next_step() {
    let mut output = Vec::new();
    let error = RuntimeError::Api {
        status: 404,
        body: String::from(r#"{"ok":false,"error":"not_found"}"#),
        auth_source: "token_file",
        auth_label: "logged in (local token)",
    };

    write_runtime_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "api returned status 404: {\"ok\":false,\"error\":\"not_found\"}\nAuth: logged in (local \
         token)\nNext: check the resource id or filters\n",
    );
}

#[test]
fn writes_api_validation_errors_as_json_with_argument_next_step() {
    let mut output = Vec::new();
    let error = RuntimeError::Api {
        status: 422,
        body: String::from(r#"{"ok":false,"error":"invalid_filter"}"#),
        auth_source: "token_file",
        auth_label: "logged in (local token)",
    };

    write_runtime_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "api_error");
    assert_eq!(body["status"], 422);
    assert_eq!(body["auth_source"], "token_file");
    assert_eq!(body["next"], "check command arguments or filters");
}

#[test]
fn writes_backend_api_code_and_next_for_agents() {
    let mut output = Vec::new();
    let error = RuntimeError::Api {
        status: 422,
        body: String::from(
            r#"{"error":"release is required","code":"validation_failed","next":"provide --release <release>"}"#,
        ),
        auth_source: "token_file",
        auth_label: "logged in (local token)",
    };

    write_runtime_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "api_error");
    assert_eq!(body["api_error"], "release is required");
    assert_eq!(body["api_code"], "validation_failed");
    assert_eq!(body["api_next"], "provide --release <release>");
    assert_eq!(body["next"], "provide --release <release>");
}

#[test]
fn writes_backend_api_code_and_next_for_humans() {
    let mut output = Vec::new();
    let error = RuntimeError::Api {
        status: 404,
        body: String::from(
            r#"{"error":"issue not found","code":"not_found","next":"check the issue id"}"#,
        ),
        auth_source: "env",
        auth_label: "logged in (env token)",
    };

    write_runtime_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "api returned status 404: {\"error\":\"issue not found\",\"code\":\"not_found\",\"next\":\"check the issue id\"}\nCode: not_found\nAuth: logged in (env token)\nNext: check the issue id\n",
    );
}
