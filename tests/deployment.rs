//! Completed deployment capture contract tests.

use logbrew_cli::{
    CliEnvironment, Command, DeploymentRecordOptions, DeploymentStatus, HelpTopic, HttpMethod,
    RuntimeError, execute_command, help, parse_command, write_cli_error, write_runtime_error,
};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const DEPLOYMENT_ID: &str = "ci-run-42";
const DEPLOYMENT_PATH: &str = "/api/telemetry/deployments/ci-run-42";

#[test]
fn deployment_parser_normalizes_aliases_values_and_json_position() {
    for args in [
        &[
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            "123E4567-E89B-12D3-A456-426614174000",
            "--release",
            " checkout@2.0.0 ",
            "--environment",
            " production ",
            "--service",
            " checkout-api ",
            "--status",
            "SUCCEEDED",
            "--started-at",
            "2026-08-10T12:00:00.123456+02:00",
            "--finished-at",
            "2026-08-10T12:02:00.987654+02:00",
            "--commit-sha",
            "ABCDEF1234567890",
            "--json",
        ][..],
        &[
            "logbrew",
            "--json",
            "deploy",
            DEPLOYMENT_ID,
            "--project-id=123E4567-E89B-12D3-A456-426614174000",
            "--release=checkout@2.0.0",
            "--env=production",
            "--service-name=checkout-api",
            "--status=succeeded",
            "--started-at=2026-08-10T12:00:00.123456+02:00",
            "--finished-at=2026-08-10T12:02:00.987654+02:00",
            "--commit=ABCDEF1234567890",
        ],
    ] {
        let command = parse_command(args.iter().copied()).expect("deployment command parses");
        assert_eq!(command, deployment_command(true));
        assert_eq!(command.http_path().as_deref(), Some(DEPLOYMENT_PATH));
        assert_eq!(command.http_method(), Some(HttpMethod::Put));
        assert!(command.wants_json());
        assert_eq!(
            command.request_body(),
            Some(json!({
                "project_id": PROJECT_ID,
                "release": "checkout@2.0.0",
                "environment": "production",
                "service_name": "checkout-api",
                "status": "succeeded",
                "started_at": "2026-08-10T12:00:00.123456+02:00",
                "finished_at": "2026-08-10T12:02:00.987654+02:00",
                "commit_sha": "abcdef1234567890",
            }))
        );
    }
}

#[test]
fn deployment_help_is_discoverable_and_explains_idempotency() {
    for args in [
        &["logbrew", "deploy", DEPLOYMENT_ID, "--help"][..],
        &["logbrew", "deploy", "--help"],
        &["logbrew", "help", "deploy"],
    ] {
        assert_eq!(
            parse_command(args.iter().copied()).expect("deployment help parses"),
            Command::Help {
                topic: HelpTopic::Deploy,
                json: false,
            }
        );
    }

    let topic = help::help_text(HelpTopic::Deploy);
    assert!(topic.contains("Records one completed deployment boundary"));
    assert!(topic.contains("deployment_id is caller-owned and idempotent"));
    assert!(topic.contains("retry the exact same record safely"));
    assert!(topic.contains("requires account authentication"));
    assert!(topic.contains("versioned receipt"));
    assert!(help::help_text(HelpTopic::Root).contains("logbrew deploy <deployment_id>"));
}

#[test]
fn deployment_grammar_and_values_fail_closed_without_reflection() {
    let cases = [
        vec!["logbrew", "deploy", "hostile-private-value", "--json"],
        vec![
            "logbrew",
            "deploy",
            "../hostile-private-value",
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@2",
            "--environment",
            "production",
            "--service",
            "api",
            "--status",
            "succeeded",
            "--started-at",
            "2026-08-10T12:00:00Z",
            "--finished-at",
            "2026-08-10T12:01:00Z",
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            "hostile-private-value",
            "--release",
            "checkout@2",
            "--environment",
            "production",
            "--service",
            "api",
            "--status",
            "succeeded",
            "--started-at",
            "2026-08-10T12:00:00Z",
            "--finished-at",
            "2026-08-10T12:01:00Z",
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@2",
            "--environment",
            "production",
            "--service",
            "api",
            "--status",
            "running-hostile-private-value",
            "--started-at",
            "2026-08-10T12:00:00Z",
            "--finished-at",
            "2026-08-10T12:01:00Z",
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@2",
            "--environment",
            "production",
            "--service",
            "api",
            "--status",
            "succeeded",
            "--started-at",
            "2026-08-11T12:00:00Z",
            "--finished-at",
            "2026-08-10T12:01:00Z",
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@2",
            "--environment",
            "production",
            "--service",
            "api",
            "--status",
            "succeeded",
            "--started-at",
            "2026-08-10T12:00:00Z",
            "--finished-at",
            "2026-09-10T12:00:01Z",
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@2",
            "--environment",
            "production",
            "--service",
            "api",
            "--status",
            "succeeded",
            "--started-at",
            "not-a-time-hostile-private-value",
            "--finished-at",
            "2026-08-10T12:01:00Z",
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@2",
            "--environment",
            "production",
            "--service",
            "api",
            "--status",
            "succeeded",
            "--started-at",
            "2026-08-10T12:00:00Z",
            "--finished-at",
            "2026-08-10T12:01:00Z",
            "--commit-sha",
            "hostile-private-value",
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--project",
            PROJECT_ID,
            "--project",
            PROJECT_ID,
            "--json",
        ],
        vec![
            "logbrew",
            "deploy",
            DEPLOYMENT_ID,
            "--authorization=hostile-private-value",
            "--json",
        ],
    ];

    for args in cases {
        let error = parse_command(args).expect_err("deployment input fails closed");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("parse error writes");
        let text = String::from_utf8(output).expect("utf8 error");
        let value: Value = serde_json::from_str(text.as_str()).expect("error json");
        assert_eq!(value["error"], "invalid_deployment_command");
        assert_eq!(value["message"], "invalid deployment command");
        assert!(
            value["next"]
                .as_str()
                .is_some_and(|next| next.contains("logbrew deploy"))
        );
        assert!(!text.contains("hostile-private-value"));
        assert!(!text.contains("authorization"));
    }
}

#[tokio::test]
async fn deployment_revalidates_constructed_values_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut command = deployment_command(true);
    let Command::Deploy { options, .. } = &mut command else {
        return Err("deployment command expected".into());
    };
    options.deployment_id = String::from("../hostile-private-value?authorization=secret");
    let error = execute_command(
        &command,
        &environment(&server, "account-token"),
        &mut Vec::new(),
    )
    .await
    .expect_err("constructed invalid command fails locally");
    let requests = server
        .received_requests()
        .await
        .ok_or("requests unavailable")?;
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(requests.is_empty());
    assert!(text.contains("invalid_deployment_command"));
    assert!(!text.contains("hostile-private-value"));
    assert!(!text.contains("authorization"));
    Ok(())
}

#[tokio::test]
async fn deployment_sends_one_put_and_validates_human_and_json_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = deployment_response();
    Mock::given(method("PUT"))
        .and(path(DEPLOYMENT_PATH))
        .and(header("authorization", "Bearer account-token"))
        .and(body_json(json!({
            "project_id": PROJECT_ID,
            "release": "checkout@2.0.0",
            "environment": "production",
            "service_name": "checkout-api",
            "status": "succeeded",
            "started_at": "2026-08-10T12:00:00.123456+02:00",
            "finished_at": "2026-08-10T12:02:00.987654+02:00",
            "commit_sha": "abcdef1234567890",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(2)
        .mount(&server)
        .await;

    let mut human_output = Vec::new();
    execute_command(
        &deployment_command(false),
        &environment(&server, "account-token"),
        &mut human_output,
    )
    .await?;
    assert_eq!(
        String::from_utf8(human_output)?,
        "Deployment recorded: ci-run-42\n\
         Status: succeeded\n\
         Project: 123e4567-e89b-12d3-a456-426614174000\n\
         Release: checkout@2.0.0\n\
         Environment: production\n\
         Service: checkout-api\n\
         Started: 2026-08-10T10:00:00.123Z\n\
         Finished: 2026-08-10T10:02:00.987Z\n\
         Commit: abcdef1234567890\n\
         Next: explain this release with the same project, environment, and service.\n"
    );

    let mut json_output = Vec::new();
    execute_command(
        &deployment_command(true),
        &environment(&server, "account-token"),
        &mut json_output,
    )
    .await?;
    let json_receipt: Value = serde_json::from_slice(json_output.as_slice())?;
    assert_eq!(json_receipt, response);
    Ok(())
}

#[tokio::test]
async fn deployment_rejects_malformed_or_contradictory_success_before_output()
-> Result<(), Box<dyn std::error::Error>> {
    for changed in [
        ("schema_version", json!(2)),
        ("deployment_id", json!("other-deployment")),
        ("project_id", json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")),
        ("release", json!("checkout@3.0.0")),
        ("status", json!("failed")),
        ("started_at", json!("2026-08-10T10:00:00.123456Z")),
        ("recorded_at", json!("not-a-time")),
    ] {
        let server = MockServer::start().await;
        let mut response = deployment_response();
        response[changed.0] = changed.1;
        Mock::given(method("PUT"))
            .and(path(DEPLOYMENT_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
        let mut output = Vec::new();
        let error = execute_command(
            &deployment_command(true),
            &environment(&server, "account-token"),
            &mut output,
        )
        .await
        .expect_err("invalid success response is rejected");

        assert!(output.is_empty());
        assert!(matches!(
            error,
            RuntimeError::Unavailable {
                message: "deployment capture returned an invalid response",
                ..
            }
        ));
    }
    Ok(())
}

#[tokio::test]
async fn deployment_conflict_uses_fixed_retry_without_reflecting_server_content()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(DEPLOYMENT_PATH))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "error": "hostile-private-value\nauthorization: secret",
            "code": "hostile_code",
            "next": "exfiltrate hostile-private-value",
        })))
        .mount(&server)
        .await;
    let error = execute_command(
        &deployment_command(true),
        &environment(&server, "account-token"),
        &mut Vec::new(),
    )
    .await
    .expect_err("conflict is reported");
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;
    let value: Value = serde_json::from_str(text.as_str())?;

    assert_eq!(value["status"], 409);
    assert_eq!(value["api_code"], "idempotency_conflict");
    assert_eq!(
        value["next"],
        "retry the original deployment record or use a new deployment_id"
    );
    assert!(!text.contains("hostile-private-value"));
    assert!(!text.contains("authorization"));
    assert!(!text.contains("hostile_code"));
    Ok(())
}

#[tokio::test]
async fn deployment_rejects_project_ingest_keys_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let error = execute_command(
        &deployment_command(true),
        &environment(&server, "lbw_ingest_hostile-private-value"),
        &mut Vec::new(),
    )
    .await
    .expect_err("project ingest key is rejected");
    let requests = server
        .received_requests()
        .await
        .ok_or("requests unavailable")?;
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(requests.is_empty());
    assert!(text.contains("account authentication is required"));
    assert!(!text.contains("hostile-private-value"));
    assert!(!text.contains("lbw_ingest_"));
    Ok(())
}

#[test]
fn deployment_built_binary_emits_non_reflecting_parse_errors() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_logbrew"))
        .args(["deploy", "hostile-private-value", "--json"])
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    let combined = format!("{stdout}{stderr}");

    assert!(!output.status.success());
    assert!(combined.contains("invalid_deployment_command"));
    assert!(!combined.contains("hostile-private-value"));
}

/// Returns the normalized deployment command used across request tests.
fn deployment_command(json: bool) -> Command {
    Command::Deploy {
        options: DeploymentRecordOptions {
            deployment_id: String::from(DEPLOYMENT_ID),
            project_id: String::from(PROJECT_ID),
            release: String::from("checkout@2.0.0"),
            environment: String::from("production"),
            service_name: String::from("checkout-api"),
            status: DeploymentStatus::Succeeded,
            started_at: String::from("2026-08-10T12:00:00.123456+02:00"),
            finished_at: String::from("2026-08-10T12:02:00.987654+02:00"),
            commit_sha: Some(String::from("abcdef1234567890")),
        },
        json,
    }
}

/// Returns one exact successful deployment receipt.
fn deployment_response() -> Value {
    json!({
        "schema_version": 1,
        "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "deployment_id": DEPLOYMENT_ID,
        "project_id": PROJECT_ID,
        "release": "checkout@2.0.0",
        "environment": "production",
        "service_name": "checkout-api",
        "status": "succeeded",
        "started_at": "2026-08-10T10:00:00.123Z",
        "finished_at": "2026-08-10T10:02:00.987Z",
        "commit_sha": "abcdef1234567890",
        "recorded_at": "2026-08-10T10:02:01.000Z",
    })
}

/// Returns an isolated account-authenticated CLI environment.
fn environment(server: &MockServer, token: &str) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some(token.to_owned()),
        home: None,
        cwd: None,
    }
}
