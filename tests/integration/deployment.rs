//! Completed deployment capture contract tests.

use crate::matchers::body_json;
use crate::{
    Mock, MockServer, ResponseTemplate, TestResult, execute_command, failed_command,
    rendered_json_error, runtime_error_text,
};
use logbrew_cli::{
    Command, DeploymentRecordOptions, DeploymentStatus, HelpTopic, HttpMethod, RuntimeError, help,
    parse_command,
};
use serde_json::{Value, json};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const DEPLOYMENT_ID: &str = "ci-run-42";
const DEPLOYMENT_PATH: &str = "/api/telemetry/deployments/ci-run-42";

#[test]
fn deployment_parser_normalizes_aliases_values_and_json_position() -> TestResult {
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
        let command = parse_command(args.iter().copied())?;
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
    Ok(())
}

#[test]
fn deployment_help_is_discoverable_and_explains_idempotency() -> TestResult {
    for args in [
        &["logbrew", "deploy", DEPLOYMENT_ID, "--help"][..],
        &["logbrew", "deploy", "--help"],
        &["logbrew", "help", "deploy"],
    ] {
        assert_eq!(
            parse_command(args.iter().copied())?,
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
    assert!(topic.contains("a project artifact key in LOGBREW_TOKEN"));
    assert!(topic.contains("versioned receipt"));
    assert!(help::help_text(HelpTopic::Root).contains("logbrew deploy <deployment_id>"));
    Ok(())
}

#[test]
fn deployment_grammar_and_values_fail_closed_without_reflection() -> TestResult {
    let base = [
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
        "--json",
    ];
    let mut cases = vec![vec!["logbrew", "deploy", "hostile-private-value", "--json"]];
    for (index, value) in [
        (2, "../hostile-private-value"),
        (4, "hostile-private-value"),
        (12, "running-hostile-private-value"),
        (14, "2026-08-11T12:00:00Z"),
        (16, "2026-09-10T12:00:01Z"),
        (14, "not-a-time-hostile-private-value"),
    ] {
        let mut args = base.to_vec();
        args[index] = value;
        cases.push(args);
    }
    let mut invalid_commit = base[..base.len() - 1].to_vec();
    invalid_commit.extend(["--commit-sha", "hostile-private-value", "--json"]);
    cases.extend([
        invalid_commit,
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
    ]);

    for args in cases {
        let (value, text) = rendered_json_error(&args)?;
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
    Ok(())
}

async_test!(deployment_revalidates_constructed_values_before_network -> TestResult, {
    let server = MockServer::start().await?;
    let mut command = deployment_command(true);
    let Command::Deploy { options, .. } = &mut command else {
        return Err("deployment command expected".into());
    };
    options.deployment_id = String::from("../hostile-private-value?authorization=secret");
    let error = failed_command(
        &command,
        &super::authenticated_env(&server, "account-token", None),
    )
    .await?;
    let requests = server.received_requests();
    let text = runtime_error_text(&error)?;

    assert!(requests.is_empty());
    assert!(text.contains("invalid_deployment_command"));
    assert!(!text.contains("hostile-private-value"));
    assert!(!text.contains("authorization"));
    Ok(())
});

async_test!(deployment_sends_one_put_and_validates_human_and_json_receipts -> TestResult, {
    let server = MockServer::start().await?;
    let response = deployment_response();
    Mock::auth("PUT", DEPLOYMENT_PATH, "account-token")
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
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .expect(2)
        .mount(&server);

    let mut human_output = Vec::new();
    execute_command(
        &deployment_command(false),
        &super::authenticated_env(&server, "account-token", None),
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
        &super::authenticated_env(&server, "account-token", None),
        &mut json_output,
    )
    .await?;
    let json_receipt: Value = serde_json::from_slice(json_output.as_slice())?;
    assert_eq!(json_receipt, response);
    Ok(())
});

async_test!(deployment_rejects_malformed_or_contradictory_success_before_output -> TestResult, {
    for changed in [
        ("schema_version", json!(2)),
        ("deployment_id", json!("other-deployment")),
        ("project_id", json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")),
        ("release", json!("checkout@3.0.0")),
        ("status", json!("failed")),
        ("started_at", json!("2026-08-10T10:00:00.123456Z")),
        ("recorded_at", json!("not-a-time")),
    ] {
        let server = MockServer::start().await?;
        let mut response = deployment_response();
        response[changed.0] = changed.1;
        Mock::auth("PUT", DEPLOYMENT_PATH, "account-token")
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .expect(1)
            .mount(&server);
        let error = failed_command(
            &deployment_command(true),
            &super::authenticated_env(&server, "account-token", None),
        )
        .await?;

        assert!(matches!(
            error,
            RuntimeError::Unavailable {
                message: "deployment capture returned an invalid response",
                ..
            }
        ));
    }
    Ok(())
});

async_test!(deployment_conflict_uses_fixed_retry_without_reflecting_server_content -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("PUT", DEPLOYMENT_PATH, "account-token")
        .respond_with(ResponseTemplate::new(409).set_body_json(&json!({
            "error": "hostile-private-value\nauthorization: secret",
            "code": "hostile_code",
            "next": "exfiltrate hostile-private-value",
        })))
        .expect(1)
        .mount(&server);
    let error = failed_command(
        &deployment_command(true),
        &super::authenticated_env(&server, "account-token", None),
    )
    .await?;
    let text = runtime_error_text(&error)?;
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
});

async_test!(deployment_sends_a_project_artifact_key_as_the_bearer -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("PUT", DEPLOYMENT_PATH, "lbw_ingest_release-artifact-key")
        .respond_with(ResponseTemplate::new(200).set_body_json(&deployment_response()))
        .expect(1)
        .mount(&server);

    let mut output = Vec::new();
    execute_command(
        &deployment_command(true),
        &super::authenticated_env(&server, "lbw_ingest_release-artifact-key", None),
        &mut output,
    )
    .await?;
    let receipt: Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(receipt["deployment_id"], "ci-run-42");
    Ok(())
});

async_test!(deployment_refused_project_key_points_to_an_artifact_key -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("PUT", DEPLOYMENT_PATH, "lbw_ingest_hostile-private-value")
        .respond_with(ResponseTemplate::new(401).set_body_string("lbw_ingest_hostile-private-value"))
        .expect(1)
        .mount(&server);
    let error = failed_command(
        &deployment_command(true),
        &super::authenticated_env(&server, "lbw_ingest_hostile-private-value", None),
    )
    .await?;
    let text = runtime_error_text(&error)?;

    assert!(text.contains("set LOGBREW_TOKEN to an artifact key for this project"));
    assert!(!text.contains("hostile-private-value"));
    assert!(!text.contains("lbw_ingest_"));
    Ok(())
});

#[test]
fn deployment_built_binary_emits_non_reflecting_parse_errors() -> TestResult {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_logbrew"))
        .args(["deploy", "hostile-private-value", "--json"])
        .env_clear()
        .output()?;
    assert!(
        output.stdout.is_empty(),
        "parse failure must not write stdout"
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    let value: Value = serde_json::from_str(&stderr)?;
    assert_eq!(value["error"], "invalid_deployment_command");
    assert!(!stderr.contains("hostile-private-value"));
    Ok(())
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
