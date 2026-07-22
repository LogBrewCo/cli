//! Secure project bootstrap contract tests.

use logbrew_cli::{
    CliEnvironment, HelpTopic, HttpMethod, RuntimeError, execute_command, help, parse_command,
    write_cli_error, write_runtime_error,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const INGEST_ID: &str = "223e4567-e89b-12d3-a456-426614174000";
const ONE_TIME_TOKEN: &str = "lbw_ingest_one_time_private_value";

#[test]
fn parses_project_create_with_normalized_exact_request() {
    let command = parse_command([
        "logbrew",
        "projects",
        "create",
        "  Checkout API  ",
        "--runtime",
        "  rust  ",
        "--environment",
        "  production  ",
        "--ingest-key-file",
        "./private/ingest.key",
        "--json",
    ])
    .expect("project create parses");

    assert_eq!(command.http_path().as_deref(), Some("/api/projects"));
    assert_eq!(command.http_method(), Some(HttpMethod::Post));
    assert_eq!(
        command.request_body(),
        Some(serde_json::json!({
            "name": "Checkout API",
            "runtime": "rust",
            "environment": "production",
            "source": "cli"
        }))
    );
    assert!(command.wants_json());

    let global_json = parse_command([
        "logbrew",
        "--json",
        "project",
        "create",
        "Checkout API",
        "--runtime=",
        "--environment=  ",
        "--ingest-key-file=./private/ingest.key",
    ])
    .expect("global json project create parses");
    assert_eq!(
        global_json.request_body(),
        Some(serde_json::json!({
            "name": "Checkout API",
            "source": "cli"
        }))
    );
    assert!(global_json.wants_json());
}

#[test]
fn project_create_rejects_invalid_or_hostile_grammar_without_reflection() {
    let long_name = "n".repeat(121);
    let long_runtime = "r".repeat(65);
    let cases = [
        vec![
            "logbrew".to_owned(),
            "projects".to_owned(),
            "create".to_owned(),
            "   ".to_owned(),
            "--ingest-key-file=./private/key".to_owned(),
            "--json".to_owned(),
        ],
        vec![
            "logbrew".to_owned(),
            "projects".to_owned(),
            "create".to_owned(),
            long_name,
            "--ingest-key-file=./private/key".to_owned(),
            "--json".to_owned(),
        ],
        vec![
            "logbrew".to_owned(),
            "projects".to_owned(),
            "create".to_owned(),
            "Checkout".to_owned(),
            "--runtime".to_owned(),
            long_runtime,
            "--ingest-key-file=./private/key".to_owned(),
            "--json".to_owned(),
        ],
        vec![
            "logbrew".to_owned(),
            "projects".to_owned(),
            "create".to_owned(),
            "Checkout".to_owned(),
            "--authorization=hostile-secret\ncontrol".to_owned(),
            "--ingest-key-file=./private/key".to_owned(),
            "--json".to_owned(),
        ],
        vec![
            "logbrew".to_owned(),
            "projects".to_owned(),
            "create".to_owned(),
            "--json".to_owned(),
            "--ingest-key-file=./private/key".to_owned(),
        ],
        vec![
            "logbrew".to_owned(),
            "projects".to_owned(),
            "create".to_owned(),
            "  --json".to_owned(),
            "--ingest-key-file=./private/key".to_owned(),
        ],
        vec![
            "logbrew".to_owned(),
            "projects".to_owned(),
            "create".to_owned(),
            "Checkout".to_owned(),
            "--json".to_owned(),
        ],
    ];

    for args in cases {
        let error = parse_command(args).expect_err("project create grammar fails closed");
        let mut output = Vec::new();
        write_cli_error(&error, true, &mut output).expect("error writes");
        let text = String::from_utf8(output).expect("utf8 output");
        let body: serde_json::Value = serde_json::from_str(text.as_str()).expect("valid json");

        assert_eq!(body["error"], "invalid_project_create_command");
        assert_eq!(body["message"], "invalid project create command");
        assert_eq!(
            body["next"],
            "use logbrew projects create <name> --ingest-key-file <path> with optional --runtime, --environment, --abandon-retry, and --json"
        );
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("authorization"));
        assert!(!text.contains("private/key"));
    }
}

#[test]
fn projects_help_documents_secure_bootstrap_and_retry() {
    let text = help::help_text(HelpTopic::Projects);

    assert!(
        text.contains(
            "logbrew projects create <name> --ingest-key-file <path> [--runtime <runtime>]"
        )
    );
    assert!(text.contains("--environment <environment>"));
    assert!(text.contains("--abandon-retry"));
    assert!(text.contains("never prints the one-time ingest key or its file path"));
    assert!(text.contains("reuses the pending retry key only for the exact same request"));
    assert!(text.contains("cannot prove owner-only file permissions fail before sending"));
}

#[cfg(unix)]
#[tokio::test]
async fn project_create_posts_exact_request_then_persists_before_safe_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .and(header("authorization", "Bearer account-token"))
        .and(header("content-type", "application/json"))
        .and(body_json(serde_json::json!({
            "name": "Checkout API",
            "runtime": "rust",
            "environment": "production",
            "source": "cli"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_response()))
        .mount(&server)
        .await;
    let fixture = Fixture::new("success")?;
    let args = fixture.args("Checkout API", false);
    let command = parse_command(args)?;
    let env = fixture.env(&server);
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output)
        .await
        .map_err(|error| format!("project create failed before request: {error:?}"))?;

    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["status"], "created");
    assert_eq!(body["project"]["id"], PROJECT_ID);
    assert_eq!(body["project"]["setup_status"], "created");
    assert_eq!(body["setup"]["status"], "created");
    assert_eq!(body["ingest_key"]["id"], INGEST_ID);
    assert_eq!(body["ingest_key"]["kind"], "cli");
    assert_eq!(body["checks"][2]["status"], "stored");
    assert_eq!(body["next"], "run logbrew doctor --project <project_id>");
    assert!(!text.contains(ONE_TIME_TOKEN));
    assert!(!text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert!(!text.contains(server.uri().as_str()));
    assert_eq!(
        std::fs::read_to_string(fixture.key_file.as_path())?,
        ONE_TIME_TOKEN
    );
    assert_private_file(fixture.key_file.as_path())?;
    assert!(!fixture.retry_state().exists());

    let requests = received_requests(&server).await?;
    let retry_key = request_retry_key(&requests[0])?;
    assert!((1..=128).contains(&retry_key.len()));
    assert!(retry_key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn exact_retry_reuses_persisted_body_and_idempotency_key()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let responder = FailThenSucceed::new(success_response());
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .respond_with(responder)
        .mount(&server)
        .await;
    let fixture = Fixture::new("exact-retry")?;
    let args = fixture.args("Checkout API", false);
    let command = parse_command(args.clone())?;
    let env = fixture.env(&server);

    let first_error = execute_command(&command, &env, &mut Vec::new())
        .await
        .expect_err("first attempt remains retryable");
    assert!(
        matches!(first_error, RuntimeError::Api { status: 500, .. }),
        "unexpected first error: {first_error:?}"
    );
    assert!(fixture.retry_state().exists());
    assert_private_file(fixture.retry_state().as_path())?;

    let retry = parse_command(args)?;
    let mut output = Vec::new();
    execute_command(&retry, &env, &mut output).await?;

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body, requests[1].body);
    assert_eq!(
        request_retry_key(&requests[0])?,
        request_retry_key(&requests[1])?
    );
    assert_eq!(
        std::fs::read_to_string(fixture.key_file.as_path())?,
        ONE_TIME_TOKEN
    );
    assert!(!fixture.retry_state().exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn changed_retry_fails_closed_until_explicit_abandonment()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let responder = FailThenSucceed::new(success_response_with_name("Changed API"));
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .respond_with(responder)
        .mount(&server)
        .await;
    let fixture = Fixture::new("changed-retry")?;
    let env = fixture.env(&server);
    let original = parse_command(fixture.args("Checkout API", false))?;
    let _first_error = execute_command(&original, &env, &mut Vec::new())
        .await
        .expect_err("first attempt fails");

    let changed = parse_command(fixture.args("Changed API", false))?;
    let changed_error = execute_command(&changed, &env, &mut Vec::new())
        .await
        .expect_err("changed retry fails locally");
    let changed_text = changed_error.to_string();
    assert!(
        changed_text.contains("pending project creation does not match"),
        "unexpected changed-request error: {changed_error:?}"
    );
    assert!(!changed_text.contains("Changed API"));
    assert!(!changed_text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert_eq!(received_requests(&server).await?.len(), 1);

    let abandoned = parse_command(fixture.args("Changed API", true))?;
    execute_command(&abandoned, &env, &mut Vec::new()).await?;

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 2);
    assert_ne!(
        request_retry_key(&requests[0])?,
        request_retry_key(&requests[1])?
    );
    assert_ne!(requests[0].body, requests[1].body);
    assert!(!fixture.retry_state().exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn retry_state_path_is_rejected_as_an_ingest_key_destination()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("reserved-retry-target")?;
    let private_dir = fixture.home.join(".logbrew");
    std::fs::create_dir(private_dir.as_path())?;
    secure_directory(private_dir.as_path())?;
    let retry_state = fixture.retry_state();
    let mut args = fixture.args("Checkout API", false);
    let target_index = args
        .iter()
        .position(|argument| argument == "--ingest-key-file")
        .expect("ingest key flag")
        + 1;
    args[target_index] = retry_state.to_string_lossy().into_owned();

    let command = parse_command(args)?;
    let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("retry-state destination fails locally");

    assert_eq!(error.to_string(), "ingest key destination is not private");
    assert!(!retry_state.exists());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn reserved_state_aliases_do_not_abandon_a_pending_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
            "error": "Internal error",
            "code": "internal_error",
            "next": "retry later",
            "next_action": {"code": "retry", "target": "request"}
        })))
        .mount(&server)
        .await;
    let fixture = Fixture::new("reserved-state-aliases")?;
    let env = fixture.env(&server);
    let original = parse_command(fixture.args("Checkout API", false))?;
    let _first_error = execute_command(&original, &env, &mut Vec::new())
        .await
        .expect_err("first attempt remains pending");
    let retry_state = fixture.retry_state();
    let lock_file = fixture.home.join(".logbrew/project-create.lock");
    assert!(retry_state.exists());
    assert!(lock_file.exists());

    let aliases = [
        fixture.root.join("secrets/retry-symlink"),
        fixture.root.join("secrets/retry-hardlink"),
        fixture.root.join("secrets/lock-symlink"),
        fixture.root.join("secrets/lock-hardlink"),
    ];
    std::os::unix::fs::symlink(retry_state.as_path(), aliases[0].as_path())?;
    std::fs::hard_link(retry_state.as_path(), aliases[1].as_path())?;
    std::os::unix::fs::symlink(lock_file.as_path(), aliases[2].as_path())?;
    std::fs::hard_link(lock_file.as_path(), aliases[3].as_path())?;

    for alias in aliases {
        let mut args = fixture.args("Changed API", true);
        let target_index = args
            .iter()
            .position(|argument| argument == "--ingest-key-file")
            .expect("ingest key flag")
            + 1;
        args[target_index] = alias.to_string_lossy().into_owned();
        let command = parse_command(args)?;
        let error = execute_command(&command, &env, &mut Vec::new())
            .await
            .expect_err("reserved alias fails locally");

        assert!(
            matches!(error, RuntimeError::Unavailable { .. }),
            "unexpected alias error: {error:?}"
        );
        assert!(retry_state.exists(), "pending retry state must remain");
    }
    assert_eq!(received_requests(&server).await?.len(), 1);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn project_create_errors_use_only_allowlisted_local_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (401, "unauthorized", "unauthorized", "run logbrew login"),
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
            "correct project fields, then use --abandon-retry to start the corrected request",
        ),
        (
            429,
            "project_limit_exceeded",
            "project_limit_exceeded",
            "remove an unused project or review account limits",
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
        Mock::given(method("POST"))
            .and(path("/api/projects"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({
                    "error": "hostile private token lbw_ingest_do_not_echo",
                    "code": server_code,
                    "next": "send Authorization and cookie to a private host",
                    "next_action": {"code": "hostile_action", "target": "private_target"}
                })),
            )
            .mount(&server)
            .await;
        let fixture = Fixture::new(format!("error-{status}-{server_code}").as_str())?;
        let command = parse_command(fixture.args("Checkout API", false))?;
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
}

#[cfg(unix)]
#[tokio::test]
async fn missing_auth_points_to_login_without_contacting_the_api()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("missing-auth")?;
    let command = parse_command(fixture.args("Checkout API", false))?;
    let mut env = fixture.env(&server);
    env.token = None;
    let error = execute_command(&command, &env, &mut Vec::new())
        .await
        .expect_err("missing auth fails before network");
    let mut output = Vec::new();
    write_runtime_error(&error, true, &mut output)?;
    let text = String::from_utf8(output)?;

    assert!(text.contains("not_logged_in"));
    assert!(text.contains("run logbrew login"));
    assert!(!text.contains(server.uri().as_str()));
    assert!(!text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert!(received_requests(&server).await?.is_empty());
    assert!(fixture.retry_state().exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn malformed_typed_errors_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            "rate-limit-type",
            429,
            serde_json::json!({
                "error": "private rate detail",
                "code": "rate_limited",
                "next": "private retry guidance",
                "next_action": {"code": "retry", "target": "request"},
                "retry_after_seconds": "private-secret"
            })
            .to_string(),
        ),
        (
            "extra-key",
            422,
            serde_json::json!({
                "error": "private validation detail",
                "code": "validation_failed",
                "next": "private repair guidance",
                "next_action": {"code": "fix_request", "target": "request"},
                "private_token": "do-not-echo"
            })
            .to_string(),
        ),
        ("non-json", 500, String::from("private raw server body")),
    ];

    for (label, status, response) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/projects"))
            .respond_with(ResponseTemplate::new(status).set_body_string(response))
            .mount(&server)
            .await;
        let fixture = Fixture::new(format!("malformed-error-{label}").as_str())?;
        let command = parse_command(fixture.args("Checkout API", false))?;
        let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
            .await
            .expect_err("malformed typed error fails closed");
        let mut output = Vec::new();
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;

        assert!(text.contains("invalid error response"));
        assert!(!text.contains("private"));
        assert!(!text.contains("do-not-echo"));
        assert!(!text.contains(server.uri().as_str()));
        assert!(!fixture.key_file.exists());
        assert!(fixture.retry_state().exists());
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn malformed_or_hostile_success_never_writes_or_echoes_token()
-> Result<(), Box<dyn std::error::Error>> {
    let mut extra = success_response();
    drop(
        extra
            .as_object_mut()
            .ok_or("success fixture must be object")?
            .insert(String::from("unexpected"), serde_json::json!("private")),
    );
    let mut malformed_token = success_response();
    malformed_token["ingest"]["token"] = serde_json::json!("secret with spaces");
    let mut mismatched_project = success_response();
    mismatched_project["setup"]["project_id"] =
        serde_json::json!("323e4567-e89b-12d3-a456-426614174000");

    for (label, response) in [
        ("extra", extra),
        ("token", malformed_token),
        ("project", mismatched_project),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
        let fixture = Fixture::new(format!("malformed-{label}").as_str())?;
        let command = parse_command(fixture.args("Checkout API", false))?;
        let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
            .await
            .expect_err("malformed success fails closed");
        let mut output = Vec::new();
        write_runtime_error(&error, true, &mut output)?;
        let text = String::from_utf8(output)?;

        assert!(text.contains("project creation returned an invalid response"));
        assert!(!text.contains(ONE_TIME_TOKEN));
        assert!(!text.contains("secret with spaces"));
        assert!(!text.contains("unexpected"));
        assert!(!fixture.key_file.exists());
        assert!(fixture.retry_state().exists());
        assert!(!std::fs::read_to_string(fixture.retry_state())?.contains(ONE_TIME_TOKEN));
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn unsafe_or_existing_key_destinations_fail_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let existing = Fixture::new("existing-target")?;
    std::fs::write(existing.key_file.as_path(), "existing-private-value")?;
    set_private_file_mode(existing.key_file.as_path())?;
    let command = parse_command(existing.args("Checkout API", false))?;
    let error = execute_command(&command, &existing.env(&server), &mut Vec::new())
        .await
        .expect_err("existing destination is not overwritten");
    assert!(error.to_string().contains("destination already exists"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt as _, symlink};
        let weak = Fixture::new("weak-parent")?;
        std::fs::set_permissions(
            weak.key_file.parent().ok_or("missing parent")?,
            std::fs::Permissions::from_mode(0o755),
        )?;
        let command = parse_command(weak.args("Checkout API", false))?;
        let error = execute_command(&command, &weak.env(&server), &mut Vec::new())
            .await
            .expect_err("weak parent fails");
        assert!(error.to_string().contains("destination is not private"));

        let linked = Fixture::new("symlink-target")?;
        let outside = linked.root.join("outside");
        std::fs::write(outside.as_path(), "outside-value")?;
        symlink(outside.as_path(), linked.key_file.as_path())?;
        let command = parse_command(linked.args("Checkout API", false))?;
        let error = execute_command(&command, &linked.env(&server), &mut Vec::new())
            .await
            .expect_err("symlink target fails");
        assert!(error.to_string().contains("destination already exists"));
    }

    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn exact_retry_can_confirm_an_already_persisted_matching_token()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .respond_with(FailThenSucceed::new(success_response()))
        .mount(&server)
        .await;
    let fixture = Fixture::new("confirm-existing")?;
    let args = fixture.args("Checkout API", false);
    let command = parse_command(args.clone())?;
    let _error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("first response is ambiguous");
    std::fs::write(fixture.key_file.as_path(), ONE_TIME_TOKEN)?;
    set_private_file_mode(fixture.key_file.as_path())?;

    let retry = parse_command(args)?;
    execute_command(&retry, &fixture.env(&server), &mut Vec::new()).await?;

    assert_eq!(
        std::fs::read_to_string(fixture.key_file.as_path())?,
        ONE_TIME_TOKEN
    );
    assert!(!fixture.retry_state().exists());
    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        request_retry_key(&requests[0])?,
        request_retry_key(&requests[1])?
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_project_create_serializes_to_one_persisted_key()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_response()))
        .mount(&server)
        .await;
    let fixture = Fixture::new("concurrent")?;
    let command = parse_command(fixture.args("Checkout API", false))?;
    let env = fixture.env(&server);
    let first_command = command.clone();
    let first_env = env.clone();
    let second_command = command;
    let second_env = env;

    let first =
        tokio::spawn(
            async move { execute_command(&first_command, &first_env, &mut Vec::new()).await },
        );
    let second = tokio::spawn(async move {
        execute_command(&second_command, &second_env, &mut Vec::new()).await
    });
    let (first, second) = tokio::join!(first, second);
    let first = first?;
    let second = second?;

    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(
        std::fs::read_to_string(fixture.key_file.as_path())?,
        ONE_TIME_TOKEN
    );
    assert_private_file(fixture.key_file.as_path())?;
    assert_eq!(received_requests(&server).await?.len(), 1);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn human_project_create_is_bounded_and_path_free() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_response()))
        .mount(&server)
        .await;
    let fixture = Fixture::new("human")?;
    let mut args = fixture.args("Checkout API", false);
    args.retain(|value| value != "--json");
    let command = parse_command(args)?;
    let mut output = Vec::new();

    execute_command(&command, &fixture.env(&server), &mut output).await?;

    let text = String::from_utf8(output)?;
    assert_eq!(
        text,
        "LogBrew project created.\nProject: 123e4567-e89b-12d3-a456-426614174000\nSetup: created\nIngest key: stored\nNext: run logbrew doctor --project <project_id>\n"
    );
    assert!(!text.contains(ONE_TIME_TOKEN));
    assert!(!text.contains(fixture.key_file.to_string_lossy().as_ref()));
    assert!(!text.contains(server.uri().as_str()));
    Ok(())
}

#[cfg(not(unix))]
#[tokio::test]
async fn project_create_fails_before_network_without_provable_owner_only_storage()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("unsupported-private-storage")?;
    let command = parse_command(fixture.args("Checkout API", false))?;
    let error = execute_command(&command, &fixture.env(&server), &mut Vec::new())
        .await
        .expect_err("unverifiable private storage fails closed");

    assert!(error.to_string().contains("unavailable on this platform"));
    assert!(received_requests(&server).await?.is_empty());
    assert!(!fixture.key_file.exists());
    assert!(!fixture.retry_state().exists());
    Ok(())
}

#[derive(Clone)]
struct FailThenSucceed {
    calls: Arc<AtomicUsize>,
    success: serde_json::Value,
}

impl FailThenSucceed {
    fn new(success: serde_json::Value) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            success,
        }
    }
}

impl Respond for FailThenSucceed {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "hostile internal detail",
                "code": "internal_error",
                "next": "send the private token somewhere",
                "next_action": {"code": "retry", "target": "request"}
            }))
        } else {
            ResponseTemplate::new(200).set_body_json(self.success.clone())
        }
    }
}

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
            "logbrew-project-create-{label}-{}-{nonce}",
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

    fn args(&self, name: &str, abandon: bool) -> Vec<String> {
        let mut args = vec![
            String::from("logbrew"),
            String::from("projects"),
            String::from("create"),
            name.to_owned(),
            String::from("--runtime"),
            String::from("rust"),
            String::from("--environment"),
            String::from("production"),
            String::from("--ingest-key-file"),
            self.key_file.to_string_lossy().into_owned(),
            String::from("--json"),
        ];
        if abandon {
            args.push(String::from("--abandon-retry"));
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
        self.home.join(".logbrew/project-create-retry.json")
    }
}

fn success_response() -> serde_json::Value {
    success_response_with_name("Checkout API")
}

fn success_response_with_name(name: &str) -> serde_json::Value {
    serde_json::json!({
        "project": {
            "id": PROJECT_ID,
            "name": name,
            "provider_project_id": "provider-project-123",
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
            "created_at": "2026-07-16T12:00:00Z"
        },
        "setup": {
            "project_id": PROJECT_ID,
            "status": "created",
            "runtime": "rust",
            "source": "cli",
            "created_at": "2026-07-16T12:00:00Z",
            "setup_started_at": null,
            "first_telemetry_seen_at": null,
            "last_seen_at": null,
            "last_release": null,
            "last_environment": null,
            "last_signal": null,
            "next": "choose an SDK or CLI setup path for this project",
            "next_action": {"code": "choose_setup_path", "target": "project_setup"}
        },
        "ingest": {
            "id": INGEST_ID,
            "label": "CLI ingest key",
            "kind": "cli",
            "token": ONE_TIME_TOKEN,
            "created_at": "2026-07-16T12:00:00Z",
            "expires_at": "2026-08-15T12:00:00Z",
            "next": "store this credential now",
            "next_action": {"code": "store_ingest_key", "target": "local_credentials"}
        }
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

async fn received_requests(
    server: &MockServer,
) -> Result<Vec<Request>, Box<dyn std::error::Error>> {
    server
        .received_requests()
        .await
        .ok_or_else(|| "request recording is disabled".into())
}

#[cfg(unix)]
fn secure_directory(path: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn secure_directory(_path: &std::path::Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
fn assert_private_file(path: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(std::fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &std::path::Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(not(unix))]
fn assert_private_file(path: &std::path::Path) -> Result<(), std::io::Error> {
    assert!(path.is_file());
    Ok(())
}
