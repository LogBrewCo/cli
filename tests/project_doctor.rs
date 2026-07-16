//! Read-only project doctor contract tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const ACCOUNT_ID: &str = "00000000-0000-4000-8000-000000000001";
const TOKEN: &str = "hostile-secret-token";

#[tokio::test]
async fn ready_json_uses_the_exact_bounded_read_sequence() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    mount_health(&server, 200, "ok").await;
    mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
    mount_setup(&server, 200, setup_body("active")).await;
    mount_logs(
        &server,
        200,
        serde_json::json!([{
            "id":"log_private",
            "service_name":"checkout-api",
            "message":"hostile-secret-log"
        }]),
    )
    .await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(
        body,
        serde_json::json!({
            "status": "ready",
            "checks": [
                {"check":"api","status":"reachable","next":"validate persisted auth"},
                {"check":"auth","status":"valid","next":"check the selected project"},
                {"check":"project","status":"usable","next":"inspect project setup state"},
                {"check":"setup","status":"acknowledged","next":"check recent telemetry"},
                {"check":"telemetry","status":"recent","next":"investigate recent telemetry"}
            ],
            "next": "run logbrew logs --project <project_id> --since 24h"
        })
    );
    assert_private_values_absent(text.as_str(), &server);

    let requests = server
        .received_requests()
        .await
        .ok_or("wiremock request recording is enabled")?;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].url.path(), "/health");
    assert_eq!(requests[1].url.path(), "/api/auth/account");
    assert_eq!(
        requests[2].url.path(),
        format!("/api/projects/{PROJECT_ID}/setup")
    );
    assert_eq!(requests[2].url.query(), None);
    assert_eq!(requests[3].url.path(), "/api/logs");
    assert_eq!(
        requests[3].url.query(),
        Some("project_id=123e4567-e89b-12d3-a456-426614174000&since=24h&limit=1")
    );
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    Ok(())
}

#[tokio::test]
async fn ready_human_output_is_bounded_and_value_safe() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_health(&server, 200, "ok").await;
    mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
    mount_setup(&server, 200, setup_body("active")).await;
    mount_logs(
        &server,
        200,
        serde_json::json!([{
            "service_name":"checkout-api",
            "message":"hostile-secret-log"
        }]),
    )
    .await;

    let text = run(&server, false).await?;

    assert_eq!(
        text,
        "LogBrew project doctor\n\
         [ok] API: reachable\n\
         [ok] Auth: valid\n\
         [ok] Project: usable\n\
         [ok] Setup: acknowledged\n\
         [ok] Telemetry: recent\n\
         Status: ready\n\
         Next: run logbrew logs --project <project_id> --since 24h\n"
    );
    assert_private_values_absent(text.as_str(), &server);
    Ok(())
}

#[tokio::test]
async fn auth_rejection_is_typed_and_stops_before_project_reads()
-> Result<(), Box<dyn std::error::Error>> {
    for status in [401, 403] {
        let server = MockServer::start().await;
        mount_health(&server, 200, "ok").await;
        mount_account(
            &server,
            status,
            serde_json::json!({"error":"hostile-secret-auth","host":"private-host"}),
        )
        .await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "auth_invalid");
        assert_eq!(body["checks"][0]["status"], "reachable");
        assert_eq!(body["checks"][1]["status"], "invalid");
        assert_eq!(body["checks"][2]["status"], "not_checked");
        assert_eq!(body["next"], "run logbrew login");
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 2);
    }
    Ok(())
}

#[tokio::test]
async fn missing_project_is_typed_and_does_not_probe_logs() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    mount_health(&server, 200, "ok").await;
    mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
    mount_setup(
        &server,
        404,
        serde_json::json!({"error":"hostile-secret-project","path":"/private/path"}),
    )
    .await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "project_missing");
    assert_eq!(body["checks"][2]["status"], "missing");
    assert_eq!(body["checks"][3]["status"], "not_checked");
    assert_eq!(
        body["next"],
        "use a project_id returned by logbrew projects"
    );
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 3);
    Ok(())
}

#[tokio::test]
async fn created_project_prioritizes_setup_incomplete_after_the_log_probe()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_health(&server, 200, "ok").await;
    mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
    mount_setup(&server, 200, setup_body("created")).await;
    mount_logs(&server, 200, serde_json::json!([])).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "setup_incomplete");
    assert_eq!(body["checks"][2]["status"], "usable");
    assert_eq!(body["checks"][3]["status"], "incomplete");
    assert_eq!(body["checks"][4]["status"], "empty");
    assert_eq!(
        body["next"],
        "choose an SDK or CLI setup path for this project"
    );
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 4);
    Ok(())
}

#[tokio::test]
async fn rejected_persisted_auth_does_not_refresh_or_write_server_state()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_health(&server, 200, "ok").await;
    Mock::given(method("GET"))
        .and(path("/api/auth/account"))
        .and(header("authorization", "Bearer local-access"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error":"hostile-secret-auth"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let home = local_auth_home(&server)?;

    let text = run_with_home(&server, true, home).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "auth_invalid");
    assert!(!text.contains("local-access"));
    assert!(!text.contains("local-refresh"));
    let requests = server
        .received_requests()
        .await
        .ok_or("wiremock request recording is enabled")?;
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    Ok(())
}

#[tokio::test]
async fn empty_recent_log_scope_is_distinct_from_setup_progress()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_health(&server, 200, "ok").await;
    mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
    mount_setup(&server, 200, setup_body("first_telemetry_seen")).await;
    mount_logs(&server, 200, serde_json::json!([])).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "telemetry_empty");
    assert_eq!(body["checks"][3]["status"], "acknowledged");
    assert_eq!(body["checks"][4]["status"], "empty");
    assert_eq!(
        body["next"],
        "send a log or inspect a wider window with logbrew logs --project <project_id> --since 7d"
    );
    assert_private_values_absent(text.as_str(), &server);
    Ok(())
}

#[tokio::test]
async fn every_acknowledged_setup_progress_state_reaches_the_log_probe()
-> Result<(), Box<dyn std::error::Error>> {
    for status in ["setup_started", "sdk_seen"] {
        let server = MockServer::start().await;
        mount_health(&server, 200, "ok").await;
        mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
        mount_setup(&server, 200, setup_body(status)).await;
        mount_logs(&server, 200, serde_json::json!([])).await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "telemetry_empty");
        assert_eq!(body["checks"][3]["status"], "acknowledged");
        assert_eq!(request_count(&server).await?, 4);
    }
    Ok(())
}

#[tokio::test]
async fn api_failure_is_typed_without_exposing_the_host_or_body()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_health(&server, 503, "hostile-secret-maintenance private-host").await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "api_unreachable");
    assert_eq!(body["checks"][0]["status"], "unreachable");
    assert_eq!(body["checks"][1]["status"], "not_checked");
    assert_eq!(
        body["next"],
        "check network access, then retry logbrew doctor --project <project_id>"
    );
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 1);
    Ok(())
}

#[tokio::test]
async fn malformed_success_bodies_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut mismatched_action = setup_body("active");
    mismatched_action["next_action"] = serde_json::json!({
        "code":"hostile-secret-action",
        "target":"private-host"
    });
    let cases = [
        DoctorMalformedCase::Account(serde_json::json!({
            "id":"hostile-secret-account"
        })),
        DoctorMalformedCase::Setup(serde_json::json!({
            "project_id": PROJECT_ID,
            "status":"active",
            "hostile-secret-extra":true
        })),
        DoctorMalformedCase::Setup(mismatched_action),
        DoctorMalformedCase::Logs(serde_json::json!({
            "logs":[],
            "hostile-secret-extra":true
        })),
        DoctorMalformedCase::Logs(serde_json::json!([{}])),
    ];

    for case in cases {
        let server = MockServer::start().await;
        mount_health(&server, 200, "ok").await;
        match case {
            DoctorMalformedCase::Account(value) => {
                mount_account(&server, 200, value).await;
            }
            DoctorMalformedCase::Setup(value) => {
                mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
                mount_setup(&server, 200, value).await;
            }
            DoctorMalformedCase::Logs(value) => {
                mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
                mount_setup(&server, 200, setup_body("active")).await;
                mount_logs(&server, 200, value).await;
            }
        }

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "check_failed");
        assert_eq!(
            body["next"],
            "retry logbrew doctor --project <project_id>; if it repeats, report the public response contract"
        );
        assert_private_values_absent(text.as_str(), &server);
    }
    Ok(())
}

#[tokio::test]
async fn validation_and_server_failures_use_fixed_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    for status in [422, 500] {
        let server = MockServer::start().await;
        mount_health(&server, 200, "ok").await;
        mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
        mount_setup(
            &server,
            status,
            serde_json::json!({"error":"hostile-secret-upstream","authorization":"secret"}),
        )
        .await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "check_failed");
        assert_eq!(body["checks"][2]["status"], "error");
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 3);
    }
    Ok(())
}

#[tokio::test]
async fn post_auth_forbidden_responses_fail_closed_without_relabeling_auth()
-> Result<(), Box<dyn std::error::Error>> {
    for failure in ["setup", "logs"] {
        let server = MockServer::start().await;
        mount_health(&server, 200, "ok").await;
        mount_account(&server, 200, serde_json::json!({"id": ACCOUNT_ID})).await;
        if failure == "setup" {
            mount_setup(
                &server,
                403,
                serde_json::json!({"error":"hostile-secret-forbidden"}),
            )
            .await;
        } else {
            mount_setup(&server, 200, setup_body("active")).await;
            mount_logs(
                &server,
                403,
                serde_json::json!({"error":"hostile-secret-forbidden"}),
            )
            .await;
        }

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "check_failed");
        assert_eq!(body["checks"][1]["status"], "valid");
        assert_private_values_absent(text.as_str(), &server);
    }
    Ok(())
}

#[derive(Debug)]
enum DoctorMalformedCase {
    Account(serde_json::Value),
    Setup(serde_json::Value),
    Logs(serde_json::Value),
}

async fn mount_health(server: &MockServer, status: u16, body: &str) {
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_account(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/api/auth/account"))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_setup(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/projects/{PROJECT_ID}/setup")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_logs(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

fn setup_body(status: &str) -> serde_json::Value {
    let (next, code, target, first_telemetry_seen_at, last_seen_at, last_signal) = match status {
        "created" => (
            "choose an SDK or CLI setup path for this project",
            "choose_setup_path",
            "project_setup",
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
        ),
        "first_telemetry_seen" | "active" => (
            "open the project dashboard or inspect recent telemetry",
            "review_project_dashboard",
            "project_dashboard",
            serde_json::json!("2026-07-16T08:00:00Z"),
            serde_json::json!("2026-07-16T09:00:00Z"),
            serde_json::json!({
                "kind":"issue",
                "id":null,
                "message":null,
                "occurred_at":"2026-07-16T09:00:00Z"
            }),
        ),
        _ => (
            "send the first telemetry event for this project",
            "send_first_telemetry",
            "telemetry_ingest",
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
        ),
    };
    serde_json::json!({
        "project_id": PROJECT_ID,
        "status": status,
        "runtime": "rust",
        "source": "cli",
        "created_at": "2026-07-16T07:00:00Z",
        "setup_started_at": "2026-07-16T07:30:00Z",
        "first_telemetry_seen_at": first_telemetry_seen_at,
        "last_seen_at": last_seen_at,
        "last_release": "checkout@1.2.3",
        "last_environment": "production",
        "last_signal": last_signal,
        "next": next,
        "next_action": {"code":code,"target":target}
    })
}

async fn run(server: &MockServer, json: bool) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["logbrew", "doctor", "--project", PROJECT_ID];
    if json {
        args.push("--json");
    }
    let command = parse_command(args)?;
    let env = CliEnvironment {
        base_url: server.uri(),
        token: Some(TOKEN.to_owned()),
        home: Some(std::env::temp_dir().join("logbrew-project-doctor-test")),
        cwd: None,
    };
    let mut output = Vec::new();
    execute_command(&command, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

async fn run_with_home(
    server: &MockServer,
    json: bool,
    home: std::path::PathBuf,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["logbrew", "doctor", "--project", PROJECT_ID];
    if json {
        args.push("--json");
    }
    let command = parse_command(args)?;
    let env = CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(home),
        cwd: None,
    };
    let mut output = Vec::new();
    execute_command(&command, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

fn local_auth_home(server: &MockServer) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::temp_dir().join(format!(
        "logbrew-project-doctor-local-{}-{}",
        std::process::id(),
        server.address().port()
    ));
    let auth_dir = home.join(".logbrew");
    std::fs::create_dir_all(auth_dir.as_path())?;
    std::fs::write(
        auth_dir.join("session.json"),
        serde_json::json!({
            "access_token":"local-access",
            "refresh_token":"local-refresh",
            "origin":server.uri()
        })
        .to_string(),
    )?;
    Ok(home)
}

async fn request_count(server: &MockServer) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(server
        .received_requests()
        .await
        .ok_or("wiremock request recording is enabled")?
        .len())
}

fn assert_private_values_absent(text: &str, server: &MockServer) {
    for private in [
        TOKEN,
        PROJECT_ID,
        ACCOUNT_ID,
        "hostile-secret",
        "private-host",
        "/private/path",
        server.uri().as_str(),
    ] {
        assert!(
            !text.contains(private),
            "private value appeared in doctor output"
        );
    }
}
