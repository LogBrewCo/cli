//! CLI API response rendering tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command, write_runtime_error};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn authenticated_read_logs_sends_bearer_token_and_prints_api_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("release", "checkout@1.2.3"))
        .and(query_param("environment", "production"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "logs": [
                {
                    "level": "warning",
                    "severity": "warning",
                    "message": "checkout failed",
                    "release": "checkout@1.2.3",
                    "environment": "production",
                    "trace_id": "trace_123"
                }
            ]
        })))
        .mount(&server)
        .await;
    let command = parse_command([
        "logbrew",
        "logs",
        "--release",
        "checkout@1.2.3",
        "--environment",
        "production",
        "--json",
    ])
    .expect("command parses");
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some("test-token".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-authenticated-read-test")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .expect("read succeeds");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["logs"][0]["level"], "warning");
    assert_eq!(body["logs"][0]["severity"], "warning");
    assert_eq!(body["logs"][0]["message"], "checkout failed");
    assert_eq!(body["logs"][0]["release"], "checkout@1.2.3");
    assert_eq!(body["logs"][0]["environment"], "production");
}

#[tokio::test]
async fn human_read_logs_prints_scan_friendly_summary() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("release", "checkout@1.2.3"))
        .and(query_param("environment", "production"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "logs": [
                {
                    "level": "warning",
                    "severity": "warning",
                    "message": "checkout failed",
                    "release": "checkout@1.2.3",
                    "environment": "production",
                    "trace_id": "trace_123"
                }
            ]
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        [
            "logbrew",
            "logs",
            "--release",
            "checkout@1.2.3",
            "--environment",
            "production",
        ],
        "human-read-logs",
    )
    .await
    .expect("read succeeds");
    assert_eq!(
        text,
        "Logs (1)\n- warning checkout failed trace=trace_123 [checkout@1.2.3 / production]\n"
    );
}

#[tokio::test]
async fn human_read_logs_summarizes_level_only_array_shape_with_canonical_label() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("release", "checkout@1.2.3"))
        .and(query_param("environment", "production"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "level": "critical",
                "severity": "critical",
                "message": "checkout failed",
                "release": "checkout@1.2.3",
                "environment": "production",
                "trace_id": "trace_123"
            }
        ])))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        [
            "logbrew",
            "logs",
            "--release",
            "checkout@1.2.3",
            "--env",
            "production",
        ],
        "human-read-logs-array",
    )
    .await
    .expect("read succeeds");
    assert_eq!(
        text,
        "Logs (1)\n- critical checkout failed trace=trace_123 [checkout@1.2.3 / production]\n"
    );
}

#[tokio::test]
async fn human_empty_logs_prints_next_step() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("release", "empty@0"))
        .and(query_param("environment", "production"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "logs": []
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        [
            "logbrew",
            "logs",
            "--release",
            "empty@0",
            "--environment",
            "production",
        ],
        "human-empty-logs",
    )
    .await
    .expect("read succeeds");
    assert_eq!(
        text,
        "Logs (0)\nNo logs found.\nNext: widen filters or check --release/--environment.\n"
    );
}

#[tokio::test]
async fn human_empty_logs_summarizes_real_api_array_shape() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("release", "empty@0"))
        .and(query_param("environment", "production"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        [
            "logbrew",
            "logs",
            "--release",
            "empty@0",
            "--environment",
            "production",
        ],
        "human-empty-logs-array",
    )
    .await
    .expect("read succeeds");
    assert_eq!(
        text,
        "Logs (0)\nNo logs found.\nNext: widen filters or check --release/--environment.\n"
    );
}

#[tokio::test]
async fn human_read_actions_and_issues_summarize_pivot_context() {
    struct ContextCase {
        route: &'static str,
        query: &'static [(&'static str, &'static str)],
        body: serde_json::Value,
        args: &'static [&'static str],
        home: &'static str,
        expected: &'static str,
    }

    let cases = [
        ContextCase {
            route: "/api/telemetry/actions",
            query: &[
                ("release", "checkout@1.2.3"),
                ("environment", "production"),
                ("name", "checkout_failed"),
            ],
            body: serde_json::json!([
                {
                    "name": "checkout_failed",
                    "severity": "warning",
                    "distinct_id": "user_123",
                    "trace_id": "trace_123",
                    "release": "checkout@1.2.3",
                    "environment": "production"
                }
            ]),
            args: &[
                "logbrew",
                "actions",
                "--release",
                "checkout@1.2.3",
                "--environment",
                "production",
                "--name",
                "checkout_failed",
            ],
            home: "human-read-actions-context",
            expected: "Actions (1)\n- checkout_failed warning user=user_123 trace=trace_123 \
                       [checkout@1.2.3 / production]\n",
        },
        ContextCase {
            route: "/api/telemetry/issues",
            query: &[
                ("release", "checkout@1.2.3"),
                ("environment", "production"),
                ("status", "unresolved"),
            ],
            body: serde_json::json!([
                {
                    "id": "issue_123",
                    "status": "unresolved",
                    "severity": "error",
                    "title": "PaymentError",
                    "occurrence_count": 2,
                    "trace_id": "trace_123",
                    "release": "checkout@1.2.3",
                    "environment": "production"
                }
            ]),
            args: &[
                "logbrew",
                "issues",
                "--release",
                "checkout@1.2.3",
                "--environment",
                "production",
                "--status",
                "unresolved",
            ],
            home: "human-read-issues-context",
            expected: "Issues (1)\n- issue_123 unresolved error PaymentError occurrences=2 \
                       trace=trace_123 [checkout@1.2.3 / production]\n",
        },
    ];

    for case in cases {
        let server = MockServer::start().await;
        mount_authenticated_json(
            &server,
            "GET",
            case.route,
            case.query.iter().copied(),
            case.body,
        )
        .await;
        let text = successful_human_output(&server, case.args.iter().copied(), case.home)
            .await
            .expect("read succeeds");

        assert_eq!(text, case.expected);
    }
}

#[tokio::test]
async fn human_read_releases_prints_all_telemetry_counts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/releases"))
        .and(query_param("environment", "production"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "releases": [
                {
                    "release": "checkout@1.2.3",
                    "environment": "production",
                    "log_count": 1,
                    "issue_count": 1,
                    "trace_span_count": 1,
                    "action_count": 1
                }
            ]
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        ["logbrew", "releases", "--environment", "production"],
        "human-read-releases",
    )
    .await
    .expect("read succeeds");
    assert_eq!(
        text,
        "Releases (1)\n- checkout@1.2.3 production logs=1 issues=1 spans=1 actions=1\n"
    );
}

#[tokio::test]
async fn human_explain_trace_prints_scan_friendly_summary() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/traces/trace_123"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "trace": {
                "trace_id": "trace_123",
                "release": "checkout@1.2.3",
                "environment": "production",
                "spans": [{"name": "checkout"}]
            }
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        ["logbrew", "explain", "trace", "trace_123"],
        "human-explain-trace",
    )
    .await
    .expect("explain succeeds");
    assert_eq!(
        text,
        "Trace trace_123 spans=1 [checkout@1.2.3 / production]\n- checkout\n"
    );
}

#[tokio::test]
async fn human_read_trace_summarizes_real_api_array_shape() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/traces/trace_123"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "trace_id": "trace_123",
                "name": "checkout",
                "release": "checkout@1.2.3",
                "environment": "production"
            }
        ])))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        ["logbrew", "trace", "trace_123"],
        "human-trace-array",
    )
    .await
    .expect("trace succeeds");
    assert_eq!(
        text,
        "Trace trace_123 spans=1 [checkout@1.2.3 / production]\n- checkout\n"
    );
}

#[tokio::test]
async fn human_read_issue_summarizes_real_api_object_shape_with_backend_next() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/issues/issue_123"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "issue_123",
            "status": "unresolved",
            "severity": "error",
            "title": "PaymentError",
            "message": "card declined",
            "occurrence_count": 2,
            "first_seen_at": "2026-06-02T19:00:00Z",
            "last_seen_at": "2026-06-02T20:00:00Z",
            "trace_id": "trace_123",
            "release": "checkout@1.2.3",
            "environment": "production",
            "next": "open the trace for issue_123",
            "next_action": {
                "code": "read_trace",
                "target": "trace_123"
            }
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        ["logbrew", "issue", "issue_123"],
        "human-issue-object",
    )
    .await
    .expect("issue succeeds");
    assert_eq!(
        text,
        "Issue issue_123 unresolved error trace=trace_123 [checkout@1.2.3 / production]\nTitle: \
         PaymentError\nMessage: card declined\nOccurrences: 2\nFirst seen: \
         2026-06-02T19:00:00Z\nLast seen: 2026-06-02T20:00:00Z\nNext: open the trace for \
         issue_123\n"
    );
}

#[tokio::test]
async fn human_set_issue_status_prints_confirmation() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/api/telemetry/issues/issue_123"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issue": {
                "id": "issue_123",
                "status": "resolved",
                "trace_id": "trace_123",
                "release": "checkout@1.2.3",
                "environment": "production"
            },
            "next": "read issue issue_123",
            "next_action": {
                "code": "read_issue",
                "target": "issue_123"
            }
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        ["logbrew", "set", "issue", "issue_123", "resolved"],
        "human-set-issue",
    )
    .await
    .expect("set succeeds");
    assert_eq!(
        text,
        "Issue issue_123 marked resolved trace=trace_123 [checkout@1.2.3 / production].\nNext: \
         read issue issue_123\n"
    );
}

#[tokio::test]
async fn human_set_issue_status_summarizes_real_api_object_shape() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/api/telemetry/issues/issue_123"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "issue_123",
            "status": "resolved",
            "trace_id": "trace_123",
            "release": "checkout@1.2.3",
            "environment": "production"
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        ["logbrew", "resolve", "issue_123"],
        "human-set-issue-object",
    )
    .await
    .expect("set succeeds");
    assert_eq!(
        text,
        "Issue issue_123 marked resolved trace=trace_123 [checkout@1.2.3 / production].\n"
    );
}

#[tokio::test]
async fn project_setup_seen_posts_backend_owned_setup_state()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects/proj_123/setup/seen"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(serde_json::json!({
            "runtime": "node",
            "source": "cli",
            "environment": "production"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "sdk_seen",
            "runtime": "node",
            "source": "cli",
            "environment": "production",
            "last_seen_at": "2026-06-15T20:00:00Z",
            "next": "send telemetry for this project"
        })))
        .mount(&server)
        .await;
    let command = parse_command([
        "logbrew",
        "projects",
        "setup",
        "proj_123",
        "--runtime",
        "node",
        "--source",
        "cli",
        "--environment",
        "production",
        "--json",
    ])?;
    let env = authenticated_env(&server, "project-setup-seen-json");
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["status"], "sdk_seen");
    assert_eq!(body["runtime"], "node");
    assert_eq!(body["source"], "cli");
    assert_eq!(body["environment"], "production");
    assert_eq!(body["next"], "send telemetry for this project");
    Ok(())
}

#[tokio::test]
async fn project_setup_seen_omits_source_for_ingest_key_auth()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects/proj_123/setup/seen"))
        .and(header("authorization", "Bearer lbw_ingest_test"))
        .and(body_json(serde_json::json!({ "runtime": "node" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "sdk_seen",
            "runtime": "node",
            "source": "sdk",
            "next": "send telemetry for this project"
        })))
        .mount(&server)
        .await;
    let command = parse_command([
        "logbrew",
        "projects",
        "setup",
        "proj_123",
        "--runtime",
        "node",
        "--json",
    ])?;
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some("lbw_ingest_test".to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-project-setup-ingest-key")),
        cwd: None,
    };
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["source"], "sdk");
    Ok(())
}

#[tokio::test]
async fn human_project_setup_seen_prints_status_and_next() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects/proj_123/setup/seen"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(serde_json::json!({ "source": "cli" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "sdk_seen",
            "source": "cli",
            "last_seen_at": "2026-06-15T20:00:00Z",
            "next": "send telemetry for this project"
        })))
        .mount(&server)
        .await;
    let text = successful_human_output(
        &server,
        ["logbrew", "projects", "setup", "proj_123"],
        "human-project-setup-seen",
    )
    .await
    .expect("project setup seen succeeds");

    assert_eq!(
        text,
        "Project setup seen: sdk_seen\nLast seen: 2026-06-15T20:00:00Z\nNext: send telemetry for this project\n"
    );
}

#[tokio::test]
async fn api_auth_error_reports_token_file_source_without_leaking_token()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(header("authorization", "Bearer expired"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "ok": false,
            "error": "not_logged_in",
            "code": "unauthorized",
            "next": "run logbrew login",
            "next_action": {
                "code": "authenticate_cli",
                "target": "cli_login"
            }
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "logs", "--json"])?;
    let home = api_rendering_home("api-auth-token-file")?;
    let token_path = home.join(".logbrew").join("token");
    std::fs::create_dir_all(token_path.parent().expect("token path has parent"))?;
    std::fs::write(token_path.as_path(), "expired\n")?;
    let env = CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(home),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("401 fails");
    write_runtime_error(&error, command.wants_json(), &mut output)?;

    let text = String::from_utf8(output)?;
    assert!(!text.contains("expired"));
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "api_error");
    assert_eq!(body["status"], 401);
    assert_eq!(body["api_code"], "unauthorized");
    assert_eq!(body["api_next"], "run logbrew login");
    assert_eq!(body["api_next_action"]["code"], "authenticate_cli");
    assert_eq!(body["api_next_action"]["target"], "cli_login");
    assert_eq!(body["auth_source"], "token_file");
    assert_eq!(body["next"], "run logbrew login");
    Ok(())
}

#[tokio::test]
async fn human_api_auth_error_reports_env_source_without_leaking_token()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(header("authorization", "Bearer env-token"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "ok": false,
            "error": "forbidden"
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "logs"])?;
    let home = api_rendering_home("api-auth-env")?;
    let token_path = home.join(".logbrew").join("token");
    std::fs::create_dir_all(token_path.parent().expect("token path has parent"))?;
    std::fs::write(token_path.as_path(), "file-token\n")?;
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some("env-token".to_owned()),
        home: Some(home),
        cwd: None,
    };
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("403 fails");
    write_runtime_error(&error, command.wants_json(), &mut output)?;

    let text = String::from_utf8(output)?;
    assert!(text.contains("api returned status 403"));
    assert!(text.contains("Auth: logged in (env token)"));
    assert!(text.contains("Next: run logbrew login"));
    assert!(!text.contains("env-token"));
    assert!(!text.contains("file-token"));
    Ok(())
}

async fn successful_human_output<I>(
    server: &MockServer,
    args: I,
    home_name: &str,
) -> Result<String, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = &'static str>,
{
    let command = parse_command(args)?;
    let env = authenticated_env(server, home_name);
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    Ok(String::from_utf8(output)?)
}

async fn mount_authenticated_json<I>(
    server: &MockServer,
    http_method: &str,
    route: &str,
    query: I,
    body: serde_json::Value,
) where
    I: IntoIterator<Item = (&'static str, &'static str)>,
{
    let mut builder = Mock::given(method(http_method))
        .and(path(route))
        .and(header("authorization", "Bearer test-token"));
    for (name, value) in query {
        builder = builder.and(query_param(name, value));
    }
    builder
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

fn authenticated_env(server: &MockServer, home_name: &str) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some("test-token".to_owned()),
        home: Some(
            std::env::temp_dir().join(format!("logbrew-{home_name}-{}", std::process::id())),
        ),
        cwd: None,
    }
}

fn api_rendering_home(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let dir = std::env::temp_dir().join(format!("logbrew-cli-{name}-{}", std::process::id()));
    match std::fs::remove_dir_all(dir.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir_all(dir.as_path())?;
    Ok(dir)
}
