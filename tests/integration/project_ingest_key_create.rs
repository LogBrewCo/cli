//! Secure existing-project ingest-key creation contract tests.

use super::{assert_private_file, secure_directory, set_private_file_mode};
use crate::matchers::{body_json, header};
use crate::{Mock, MockServer, Request, ResponseTemplate, execute_command, retry_then};
use logbrew_cli::{
    CliEnvironment, HelpTopic, HttpMethod, RuntimeError, help, parse_command, write_cli_error,
    write_runtime_error,
};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const INGEST_ID: &str = "223e4567-e89b-12d3-a456-426614174000";
const DEFAULT_LABEL: &str = "CLI-created SDK key";
const ONE_TIME_TOKEN: &str = "lbw_ingest_existing_project_private_value";

#[test]
fn parses_existing_project_key_create_with_normalized_exact_request() {
    let command = parse_command([
        "logbrew",
        "projects",
        "keys",
        "create",
        PROJECT_ID,
        "--label",
        "  Mobile server key  ",
        "--kind",
        "server",
        "--ingest-key-file",
        "./private/ingest.key",
        "--json",
    ])
    .expect("existing-project key create parses");

    assert_eq!(
        command.http_path().as_deref(),
        Some(format!("/api/projects/{PROJECT_ID}/ingest-keys").as_str())
    );
    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert_eq!(
        command.request_body(),
        Some(serde_json::json!({
            "label": "Mobile server key",
            "kind": "server",
            "expires_at": null,
        }))
    );
    assert!(command.wants_json());

    let defaulted = parse_command([
        "logbrew",
        "--json",
        "project",
        "keys",
        "create",
        PROJECT_ID,
        "--ingest-key-file=./private/ingest.key",
    ])
    .expect("global JSON default key create parses");
    assert_eq!(
        defaulted.request_body(),
        Some(serde_json::json!({
            "label": DEFAULT_LABEL,
            "kind": "sdk",
            "expires_at": null,
        }))
    );
    assert!(defaulted.wants_json());
}

#[test]
fn existing_project_key_create_rejects_hostile_or_ambiguous_grammar_without_reflection() {
    let invalid_project = "not-a-private-project-id";
    let hostile_label = "hostile-secret\ncontrol";
    let cases = [
        vec![
            "logbrew",
            "projects",
            "keys",
            "create",
            invalid_project,
            "--ingest-key-file=./private/key",
            "--json",
        ],
        vec![
            "logbrew",
            "projects",
            "keys",
            "create",
            PROJECT_ID,
            "--kind",
            "account",
            "--ingest-key-file=./private/key",
            "--json",
        ],
        vec![
            "logbrew",
            "projects",
            "keys",
            "create",
            PROJECT_ID,
            "--label",
            hostile_label,
            "--ingest-key-file=./private/key",
            "--json",
        ],
        vec![
            "logbrew", "projects", "keys", "create", PROJECT_ID, "--json",
        ],
        vec![
            "logbrew",
            "projects",
            "keys",
            "create",
            PROJECT_ID,
            "--ingest-key-file=./private/key",
            "--ingest-key-file=./private/other",
            "--json",
        ],
    ];

    for args in cases {
        let error = parse_command(args).expect_err("key-create grammar fails closed");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("error writes");
        let text = String::from_utf8(output).expect("UTF-8 error output");
        let body: serde_json::Value = serde_json::from_str(text.as_str()).expect("valid JSON");

        assert_eq!(body["error"], "invalid_project_ingest_key_create_command");
        assert_eq!(body["message"], "invalid project ingest key create command");
        assert_eq!(
            body["next"],
            "use logbrew projects keys create <project_id> --ingest-key-file <path> with optional --label, --kind sdk|browser|server|cli, --abandon-retry, and --json"
        );
        assert!(!text.contains(invalid_project));
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("private/key"));
    }
}

#[test]
fn projects_help_documents_existing_project_key_creation_and_safe_retry() {
    let text = help::help_text(HelpTopic::Projects);

    assert!(text.contains("logbrew projects keys create <project_id> --ingest-key-file <path>"));
    assert!(text.contains("--kind sdk|browser|server|cli"));
    assert!(text.contains("existing project without creating a duplicate project"));
    assert!(text.contains("never prints the one-time ingest key or its file path"));
    assert!(text.contains("reuses the pending retry key only for the exact same request"));
    assert!(text.contains("cannot prove owner-only file permissions fail before sending"));
}

#[cfg(unix)]
async_test!(existing_project_key_create_posts_exact_request_then_persists_before_safe_json -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::auth(
        "POST",
        format!("/api/projects/{PROJECT_ID}/ingest-keys"),
        "account-token",
    )
    .and(header("content-type", "application/json"))
    .and(body_json(serde_json::json!({
        "label": DEFAULT_LABEL,
        "kind": "sdk",
        "expires_at": null,
    })))
    .respond_with(ResponseTemplate::new(200).set_body_json(success_response(DEFAULT_LABEL, "sdk")))
    .mount(&server);
    let fixture = Fixture::new("success")?;
    let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;
    let mut output = Vec::new();

    execute_command(&command, &fixture.env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["status"], "created");
    assert_eq!(body["project_id"], PROJECT_ID);
    assert_eq!(body["ingest_key"]["id"], INGEST_ID);
    assert_eq!(body["ingest_key"]["label"], DEFAULT_LABEL);
    assert_eq!(body["ingest_key"]["kind"], "sdk");
    assert_eq!(body["ingest_key"]["expires_at"], serde_json::Value::Null);
    assert_eq!(body["checks"][0]["status"], "stored");
    assert_eq!(
        body["next"],
        "configure the SDK with the stored ingest key, then run logbrew doctor --project <project_id>"
    );
    assert!(!text.contains(ONE_TIME_TOKEN));
    assert!(!text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert!(!text.contains(server.uri().as_str()));
    assert_eq!(
        std::fs::read_to_string(fixture.key_file.as_path())?,
        ONE_TIME_TOKEN
    );
    assert_private_file(fixture.key_file.as_path())?;
    assert!(!fixture.retry_state().exists());

    let requests = server.received_requests();
    let retry_key = request_retry_key(&requests[0])?;
    assert!((1..=128).contains(&retry_key.len()));
    assert!(retry_key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)));
    Ok(())
});

#[cfg(unix)]
async_test!(built_binary_creates_existing_project_key_over_loopback_without_secret_output -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::auth(
        "POST",
        format!("/api/projects/{PROJECT_ID}/ingest-keys"),
        "account-token",
    )
    .and(body_json(serde_json::json!({
        "label": DEFAULT_LABEL,
        "kind": "sdk",
        "expires_at": null,
    })))
    .respond_with(ResponseTemplate::new(200).set_body_json(success_response(DEFAULT_LABEL, "sdk")))
    .mount(&server);
    let fixture = Fixture::new("built-binary")?;
    let mut command = super::cli_command(&server);
    let _command = command
        .env("HOME", fixture.home.as_path())
        .current_dir(fixture.root.as_path())
        .args([
            "projects",
            "keys",
            "create",
            PROJECT_ID,
            "--ingest-key-file",
            fixture.key_file.to_string_lossy().as_ref(),
            "--json",
        ]);
    let process = super::run_cli_command(command).await?;

    super::assert_cli_success(&process);
    let text = String::from_utf8(process.stdout)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["status"], "created");
    assert_eq!(body["project_id"], PROJECT_ID);
    assert!(!text.contains(ONE_TIME_TOKEN));
    assert!(!text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert!(!text.contains(server.uri().as_str()));
    assert_eq!(
        std::fs::read_to_string(fixture.key_file.as_path())?,
        ONE_TIME_TOKEN
    );
    assert_private_file(fixture.key_file.as_path())?;
    Ok(())
});

#[cfg(unix)]
async_test!(existing_project_key_exact_retry_reuses_body_and_idempotency_key -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .respond_with(retry_then(success_response(DEFAULT_LABEL, "sdk")))
        .mount(&server);
    let fixture = Fixture::new("exact-retry")?;
    let args = fixture.args(DEFAULT_LABEL, "sdk", false, true);
    let command = parse_command(args.clone())?;

    let first_error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("first attempt remains retryable");
    assert!(
        matches!(first_error, RuntimeError::Api { status: 500, .. }),
        "unexpected first error: {first_error:?}"
    );
    assert!(fixture.retry_state().exists());
    assert_private_file(fixture.retry_state().as_path())?;

    let retry = parse_command(args)?;
    execute_command(&retry, &fixture.env(&server), &mut Vec::new()).await?;

    let requests = server.received_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body, requests[1].body);
    assert_eq!(
        request_retry_key(&requests[0])?,
        request_retry_key(&requests[1])?
    );
    assert!(!fixture.retry_state().exists());
    Ok(())
});

#[cfg(unix)]
async_test!(existing_project_key_retry_state_is_isolated_from_project_creation -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_response(DEFAULT_LABEL, "sdk")),
        )
        .mount(&server);
    let fixture = Fixture::new("isolated-retry")?;
    let private_dir = fixture.home.join(".logbrew");
    std::fs::create_dir(private_dir.as_path())?;
    secure_directory(private_dir.as_path())?;
    let project_retry = private_dir.join("project-create-retry.json");
    let project_retry_body = serde_json::json!({
        "version": 1,
        "origin": server.uri(),
        "request_body": "{\"name\":\"pending\"}",
        "retry_key": "pending-project-retry",
        "ingest_key_file": fixture.root.join("secrets/project.key"),
    })
    .to_string();
    std::fs::write(project_retry.as_path(), project_retry_body.as_bytes())?;
    set_private_file_mode(project_retry.as_path())?;
    let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;

    execute_command(&command, &fixture.env(&server), &mut Vec::new()).await?;

    assert_eq!(
        std::fs::read_to_string(project_retry.as_path())?,
        project_retry_body
    );
    assert!(!fixture.retry_state().exists());
    assert_eq!(
        std::fs::read_to_string(fixture.key_file.as_path())?,
        ONE_TIME_TOKEN
    );
    Ok(())
});

#[cfg(unix)]
async_test!(changed_existing_project_key_retry_requires_explicit_abandonment -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .respond_with(retry_then(success_response("Replacement SDK key", "sdk")))
        .mount(&server);
    let fixture = Fixture::new("changed-retry")?;
    let original = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;
    let _first_error = execute_command(&original, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("first attempt remains pending");

    let changed = parse_command(fixture.args("Replacement SDK key", "sdk", false, true))?;
    let changed_error = execute_command(&changed, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("changed retry fails locally");
    assert!(
        changed_error
            .to_string()
            .contains("pending ingest key creation does not match"),
        "unexpected changed-request error: {changed_error:?}"
    );
    assert_eq!(server.received_requests().len(), 1);

    let abandoned = parse_command(fixture.args("Replacement SDK key", "sdk", true, true))?;
    execute_command(&abandoned, &fixture.env(&server), &mut Vec::new()).await?;

    let requests = server.received_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(
        request_retry_key(&requests[0])?,
        request_retry_key(&requests[1])?
    );
    assert_ne!(requests[0].body, requests[1].body);
    assert!(!fixture.retry_state().exists());
    Ok(())
});

#[cfg(unix)]
async_test!(existing_project_key_errors_use_only_allowlisted_local_recovery -> Result<(), Box<dyn std::error::Error>>, {
    let cases = [
        (401, "unauthorized", "unauthorized", "run logbrew login"),
        (
            404,
            "not_found",
            "not_found",
            "run logbrew projects --json and retry with an active project_id",
        ),
        (
            409,
            "idempotency_conflict",
            "idempotency_conflict",
            "rerun with --abandon-retry only when intentionally discarding the pending attempt",
        ),
        (
            422,
            "validation_failed",
            "validation_failed",
            "correct key fields, then use --abandon-retry to start the corrected request",
        ),
        (
            429,
            "rate_limited",
            "rate_limited",
            "retry the exact same command later",
        ),
        (
            500,
            "private_storage_secret",
            "server_error",
            "retry the exact same command to reuse the pending request",
        ),
    ];

    for (status, server_code, expected_code, expected_next) in cases {
        let server = MockServer::start().await;
        Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({
                    "error": "hostile private token lbw_ingest_do_not_echo",
                    "code": server_code,
                    "next": "send Authorization and cookie to a private host",
                    "next_action": {"code": "hostile_action", "target": "private_target"}
                })),
            )
            .mount(&server);
        let fixture = Fixture::new(format!("error-{status}-{server_code}").as_str())?;
        let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;
        let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
            .await
            .expect_err("typed API error fails safely");
        let mut output = Vec::new();
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], status);
        assert_eq!(body["api_code"], expected_code);
        assert_eq!(body["next"], expected_next);
        assert!(!text.contains("lbw_ingest_do_not_echo"));
        assert!(!text.contains("Authorization"));
        assert!(!text.contains("cookie"));
        assert!(!text.contains("private_storage_secret"));
        assert!(!text.contains(server.uri().as_str()));
        assert!(!fixture.key_file.exists());
        assert!(fixture.retry_state().exists());
    }
    Ok(())
});

#[cfg(unix)]
async_test!(malformed_existing_project_key_error_fails_closed_without_reflection -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "error": "private validation detail",
            "code": "validation_failed",
            "next": "private repair guidance",
            "next_action": {"code": "fix_request", "target": "request"},
            "private_token": "do-not-echo"
        })))
        .mount(&server);
    let fixture = Fixture::new("malformed-error")?;
    let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;
    let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("malformed typed error fails closed");
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("ingest key creation returned an invalid error response"));
    assert!(!text.contains("private"));
    assert!(!text.contains("do-not-echo"));
    assert!(!text.contains(server.uri().as_str()));
    assert!(!fixture.key_file.exists());
    assert!(fixture.retry_state().exists());
    Ok(())
});

#[cfg(unix)]
async_test!(malformed_existing_project_key_success_never_writes_or_echoes_token -> Result<(), Box<dyn std::error::Error>>, {
    let mut extra = success_response(DEFAULT_LABEL, "sdk");
    extra["private_detail"] = serde_json::json!("do-not-echo");
    let mut token = success_response(DEFAULT_LABEL, "sdk");
    token["token"] = serde_json::json!("secret with spaces");
    let mut label = success_response(DEFAULT_LABEL, "sdk");
    label["label"] = serde_json::json!("Different private key");
    let mut kind = success_response(DEFAULT_LABEL, "sdk");
    kind["kind"] = serde_json::json!("server");
    let mut id = success_response(DEFAULT_LABEL, "sdk");
    id["id"] = serde_json::json!("not-a-public-uuid");
    let mut action = success_response(DEFAULT_LABEL, "sdk");
    action["next_action"] =
        serde_json::json!({"code": "private_action", "target": "private_target"});
    let mut expiry = success_response(DEFAULT_LABEL, "sdk");
    expiry["expires_at"] = serde_json::json!(123);
    let mut missing = success_response(DEFAULT_LABEL, "sdk");
    drop(
        missing
            .as_object_mut()
            .ok_or("success fixture must be an object")?
            .remove("expires_at"),
    );

    for (case, response) in [
        ("extra", extra),
        ("token", token),
        ("label", label),
        ("kind", kind),
        ("id", id),
        ("action", action),
        ("expiry", expiry),
        ("missing", missing),
    ] {
        let server = MockServer::start().await;
        Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server);
        let fixture = Fixture::new(format!("malformed-success-{case}").as_str())?;
        let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;

        let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
            .await
            .expect_err("malformed success fails closed");
        let mut output = Vec::new();
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;

        assert!(text.contains("ingest key creation returned an invalid response"));
        assert!(!text.contains("secret with spaces"));
        assert!(!text.contains("do-not-echo"));
        assert!(!text.contains("Different private key"));
        assert!(!text.contains("private_action"));
        assert!(!fixture.key_file.exists());
        assert!(fixture.retry_state().exists());
        let retry_state = std::fs::read_to_string(fixture.retry_state())?;
        assert!(!retry_state.contains("secret with spaces"));
        assert!(!retry_state.contains("do-not-echo"));
    }
    Ok(())
});

#[cfg(unix)]
async_test!(oversized_existing_project_key_response_fails_closed_before_persistence -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(64 * 1024 + 1)))
        .mount(&server);
    let fixture = Fixture::new("oversized-success")?;
    let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;

    let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("oversized response fails closed");

    assert_eq!(
        error.to_string(),
        "ingest key creation returned an invalid response"
    );
    assert!(!fixture.key_file.exists());
    assert!(fixture.retry_state().exists());
    Ok(())
});

#[cfg(unix)]
async_test!(existing_project_key_create_never_follows_redirects -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    let redirected = MockServer::start().await;
    Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("location", redirected.uri().as_str()),
        )
        .mount(&server);
    let fixture = Fixture::new("redirect")?;
    let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;

    let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("redirect fails closed");
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("ingest key creation returned an invalid error response"));
    assert!(!text.contains(redirected.uri().as_str()));
    assert!(redirected.received_requests().is_empty());
    assert!(!fixture.key_file.exists());
    assert!(fixture.retry_state().exists());
    Ok(())
});

#[cfg(unix)]
async_test!(existing_or_unsafe_key_destination_fails_before_network -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    let existing = Fixture::new("existing-target")?;
    std::fs::write(existing.key_file.as_path(), "existing-private-value")?;
    set_private_file_mode(existing.key_file.as_path())?;
    let command = parse_command(existing.args(DEFAULT_LABEL, "sdk", false, true))?;
    let error = execute_command(&command, &existing.env(&server), &mut Vec::new())
        .await
        .expect_err("existing destination is not overwritten");
    assert!(error.to_string().contains("destination already exists"));

    use std::os::unix::fs::PermissionsExt as _;
    let weak = Fixture::new("weak-parent")?;
    std::fs::set_permissions(
        weak.key_file.parent().ok_or("missing parent")?,
        std::fs::Permissions::from_mode(0o755),
    )?;
    let command = parse_command(weak.args(DEFAULT_LABEL, "sdk", false, true))?;
    let error = execute_command(&command, &weak.env(&server), &mut Vec::new())
        .await
        .expect_err("weak parent fails");
    assert!(error.to_string().contains("destination is not private"));

    assert!(server.received_requests().is_empty());
    Ok(())
});

#[cfg(unix)]
async_test!(existing_project_key_missing_auth_points_to_login_without_network -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    let fixture = Fixture::new("missing-auth")?;
    let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, true))?;
    let mut env = fixture.env(&server);
    env.token = None;

    let error = execute_command(&command, &env, &mut Vec::new())
        .await
        .expect_err("missing auth fails");
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("not_logged_in"));
    assert!(text.contains("run logbrew login"));
    assert!(!text.contains(server.uri().as_str()));
    assert!(!text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert!(server.received_requests().is_empty());
    assert!(fixture.retry_state().exists());
    Ok(())
});

#[cfg(unix)]
async_test!(human_existing_project_key_success_is_bounded_and_path_free -> Result<(), Box<dyn std::error::Error>>, {
    let server = MockServer::start().await;
    Mock::route("POST", format!("/api/projects/{PROJECT_ID}/ingest-keys"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_response(DEFAULT_LABEL, "sdk")),
        )
        .mount(&server);
    let fixture = Fixture::new("human")?;
    let command = parse_command(fixture.args(DEFAULT_LABEL, "sdk", false, false))?;
    let mut output = Vec::new();

    execute_command(&command, &fixture.env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        format!(
            "LogBrew ingest key created.\nProject: {PROJECT_ID}\nKind: sdk\nIngest key: stored\nNext: configure the SDK with the stored ingest key, then run logbrew doctor --project <project_id>\n"
        )
    );
    assert!(!text.contains(ONE_TIME_TOKEN));
    assert!(!text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert!(!text.contains(server.uri().as_str()));
    Ok(())
});

struct Fixture {
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    key_file: std::path::PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "logbrew-project-ingest-key-create-{label}-{}-{nonce}",
            std::process::id()
        ));
        let home = root.join("home");
        let secrets = root.join("secrets");
        std::fs::create_dir_all(home.as_path())?;
        std::fs::create_dir_all(secrets.as_path())?;
        secure_directory(home.as_path())?;
        secure_directory(secrets.as_path())?;
        Ok(Self {
            root,
            home,
            key_file: secrets.join("ingest.key"),
        })
    }

    fn args(&self, label: &str, kind: &str, abandon: bool, json: bool) -> Vec<String> {
        let mut args = vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("keys"),
            String::from("create"),
            String::from(PROJECT_ID),
            String::from("--label"),
            label.to_owned(),
            String::from("--kind"),
            kind.to_owned(),
            String::from("--ingest-key-file"),
            self.key_file.to_string_lossy().into_owned(),
        ];
        if abandon {
            args.push(String::from("--abandon-retry"));
        }
        if json {
            args.push(String::from("--json"));
        }
        args
    }

    fn env(&self, server: &MockServer) -> CliEnvironment {
        CliEnvironment {
            base_url: server.uri(),
            token: Some(String::from("account-token")),
            home: Some(self.home.clone()),
            cwd: Some(self.root.clone()),
        }
    }

    fn retry_state(&self) -> std::path::PathBuf {
        self.home
            .join(".logbrew/project-ingest-key-create-retry.json")
    }
}

fn success_response(label: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({
        "id": INGEST_ID,
        "label": label,
        "kind": kind,
        "token": ONE_TIME_TOKEN,
        "created_at": "2026-07-31T12:00:00Z",
        "expires_at": null,
        "next": "configure this ingest key in an SDK or client",
        "next_action": {"code": "send_first_telemetry", "target": "telemetry_ingest"}
    })
}

fn request_retry_key(request: &Request) -> Result<&str, Box<dyn std::error::Error>> {
    request
        .headers
        .get("idempotency-key")
        .ok_or_else(|| -> Box<dyn std::error::Error> { "missing idempotency key".into() })?
        .to_str()
        .map_err(Into::into)
}
