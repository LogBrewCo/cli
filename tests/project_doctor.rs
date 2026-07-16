//! Read-only project doctor contract tests.

use logbrew_cli::{CliEnvironment, execute_command, parse_command};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const UPPER_PROJECT_ID: &str = "123E4567-E89B-12D3-A456-426614174000";
const OTHER_PROJECT_ID: &str = "223e4567-e89b-12d3-a456-426614174000";
const TOKEN: &str = "hostile-secret-token";

#[tokio::test]
async fn ready_json_uses_the_canonical_doctor_read_then_a_log_visibility_probe()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_doctor(&server, 200, doctor_body("ready", "active")).await;
    mount_logs(&server, 200, serde_json::json!([])).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(
        body,
        serde_json::json!({
            "status": "ready",
            "checks": [
                {"check":"api","status":"reachable","next":"validate persisted auth"},
                {"check":"auth","status":"valid","next":"inspect the selected project"},
                {"check":"project","status":"usable","next":"inspect project readiness"},
                {"check":"ingest_key","status":"active","next":"inspect setup acknowledgement"},
                {"check":"setup","status":"operational","next":"inspect telemetry state"},
                {"check":"telemetry","status":"seen","next":"inspect recent telemetry"},
                {"check":"logs","status":"empty","next":"inspect another telemetry stream"}
            ],
            "next": "inspect recent project logs, issues, actions, releases, or traces"
        })
    );
    assert_private_values_absent(text.as_str(), &server);

    let requests = requests(&server).await?;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].url.path(),
        format!("/api/projects/{PROJECT_ID}/doctor")
    );
    assert_eq!(requests[0].url.query(), None);
    assert!(requests[0].body.is_empty());
    assert_eq!(requests[1].url.path(), "/api/logs");
    let query = requests[1]
        .url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        query,
        vec![
            ("project_id".to_owned(), PROJECT_ID.to_owned()),
            ("limit".to_owned(), "1".to_owned())
        ]
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
    let mut response = doctor_body("ready", "active");
    response["next"] = serde_json::json!("hostile-secret-next");
    response["last_signal"] = serde_json::json!({
        "kind":"log",
        "id":"hostile-secret-id",
        "message":"hostile-secret-message",
        "occurred_at":"2026-07-16T08:00:00Z"
    });
    mount_doctor(&server, 200, response).await;
    mount_logs(
        &server,
        200,
        serde_json::json!([{
            "message":"hostile-secret-log",
            "service_name":"hostile-secret-service"
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
         [ok] Ingest key: active\n\
         [ok] Setup: operational\n\
         [ok] Telemetry: seen\n\
         [ok] Logs: visible\n\
         Status: ready\n\
         Next: inspect recent project logs, issues, actions, releases, or traces\n"
    );
    assert_private_values_absent(text.as_str(), &server);
    Ok(())
}

#[tokio::test]
async fn canonical_states_drive_readiness_while_logs_only_report_visibility()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            "needs_ingest_key",
            "active",
            true,
            "missing",
            "operational",
            "seen",
            "visible",
            "create an ingest key for this project, then rerun logbrew doctor --project <project_id>",
        ),
        (
            "needs_setup",
            "created",
            false,
            "active",
            "not_started",
            "not_seen",
            "empty",
            "choose an SDK or CLI setup path for this project",
        ),
        (
            "needs_telemetry",
            "setup_started",
            false,
            "active",
            "path_selected",
            "not_seen",
            "empty",
            "send the first telemetry event for this project",
        ),
        (
            "needs_telemetry",
            "sdk_seen",
            false,
            "active",
            "acknowledged",
            "not_seen",
            "empty",
            "send the first telemetry event for this project",
        ),
        (
            "ready",
            "first_telemetry_seen",
            false,
            "active",
            "operational",
            "seen",
            "empty",
            "inspect recent project logs, issues, actions, releases, or traces",
        ),
    ];

    for (state, setup_status, logs_visible, ingest, setup, telemetry, logs, next) in cases {
        let server = MockServer::start().await;
        mount_doctor(&server, 200, doctor_body(state, setup_status)).await;
        mount_logs(
            &server,
            200,
            if logs_visible {
                serde_json::json!([{
                    "message":"hostile-secret-log",
                    "service_name":"hostile-secret-service"
                }])
            } else {
                serde_json::json!([])
            },
        )
        .await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], state, "state case {state}");
        assert_eq!(body["checks"][3]["status"], ingest, "state case {state}");
        assert_eq!(body["checks"][4]["status"], setup, "state case {state}");
        assert_eq!(body["checks"][5]["status"], telemetry, "state case {state}");
        assert_eq!(body["checks"][6]["status"], logs, "state case {state}");
        assert_eq!(body["next"], next, "state case {state}");
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 2);
    }
    Ok(())
}

#[tokio::test]
async fn auth_rejection_is_typed_and_stops_before_the_log_probe()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_doctor(&server, 401, unauthorized_error()).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "auth_invalid");
    assert_eq!(body["checks"][0]["status"], "reachable");
    assert_eq!(body["checks"][1]["status"], "invalid");
    assert_eq!(body["checks"][2]["status"], "not_checked");
    assert_eq!(body["next"], "run logbrew login");
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 1);
    Ok(())
}

#[tokio::test]
async fn missing_local_auth_is_typed_without_a_network_request()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let home = unique_home(&server, "missing-auth");

    let text = run_with_home(&server, true, home).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "auth_invalid");
    assert_eq!(body["checks"][0]["status"], "not_checked");
    assert_eq!(body["checks"][1]["status"], "missing");
    assert_eq!(body["next"], "run logbrew login");
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 0);
    Ok(())
}

#[tokio::test]
async fn rejected_persisted_auth_does_not_refresh_or_rewrite_the_session()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/projects/{PROJECT_ID}/doctor")))
        .and(header("authorization", "Bearer local-access"))
        .respond_with(ResponseTemplate::new(401).set_body_json(unauthorized_error()))
        .expect(1)
        .mount(&server)
        .await;
    let home = local_auth_home(&server)?;
    let session_path = home.join(".logbrew/session.json");
    let original_session = std::fs::read(session_path.as_path())?;

    let text = run_with_home(&server, true, home).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "auth_invalid");
    assert_private_values_absent(text.as_str(), &server);
    let requests = requests(&server).await?;
    assert_eq!(requests.len(), 1);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    assert_eq!(std::fs::read(session_path)?, original_session);
    Ok(())
}

#[tokio::test]
async fn owner_safe_project_missing_is_typed_and_stops_before_logs()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_doctor(&server, 404, project_not_found_error()).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "project_missing");
    assert_eq!(body["checks"][0]["status"], "reachable");
    assert_eq!(body["checks"][1]["status"], "valid");
    assert_eq!(body["checks"][2]["status"], "missing");
    assert_eq!(
        body["next"],
        "use a project_id returned by logbrew projects"
    );
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 1);
    Ok(())
}

#[tokio::test]
async fn uppercase_uuid_input_binds_to_the_canonical_lowercase_response()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/projects/{UPPER_PROJECT_ID}/doctor")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(doctor_body("ready", "active")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("project_id", UPPER_PROJECT_ID))
        .and(query_param("limit", "1"))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let text = run_project(&server, UPPER_PROJECT_ID, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "ready");
    assert_private_values_absent(text.as_str(), &server);
    assert!(!text.contains(UPPER_PROJECT_ID));
    assert_eq!(request_count(&server).await?, 2);
    Ok(())
}

#[tokio::test]
async fn success_contract_mismatches_fail_closed_before_logs()
-> Result<(), Box<dyn std::error::Error>> {
    let mut cases = Vec::new();

    let mut extra = doctor_body("ready", "active");
    extra["extra"] = serde_json::json!(true);
    cases.push(extra);

    let mut wrong_project = doctor_body("ready", "active");
    wrong_project["project_id"] = serde_json::json!(OTHER_PROJECT_ID);
    cases.push(wrong_project);

    let mut unknown_state = doctor_body("ready", "active");
    unknown_state["state"] = serde_json::json!("hostile-secret-state");
    cases.push(unknown_state);

    let mut mismatched_action = doctor_body("ready", "active");
    mismatched_action["next_action"] =
        serde_json::json!({"code":"create_ingest_key","target":"project_ingest_keys"});
    cases.push(mismatched_action);

    let mut mismatched_ack = doctor_body("needs_telemetry", "sdk_seen");
    mismatched_ack["setup_acknowledged"] = serde_json::json!(false);
    cases.push(mismatched_ack);

    let mut inconsistent_key = doctor_body("needs_setup", "setup_started");
    inconsistent_key["has_active_ingest_key"] = serde_json::json!(false);
    cases.push(inconsistent_key);

    let wrong_state = doctor_body("needs_setup", "setup_started");
    cases.push(wrong_state);

    let mut bad_timestamp = doctor_body("ready", "active");
    bad_timestamp["last_seen_at"] = serde_json::json!("hostile-secret-time");
    cases.push(bad_timestamp);

    let mut bad_signal = doctor_body("ready", "active");
    bad_signal["last_signal"]["extra"] = serde_json::json!("hostile-secret-field");
    cases.push(bad_signal);

    for value in cases {
        let server = MockServer::start().await;
        mount_doctor(&server, 200, value).await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "check_failed");
        assert_eq!(body["checks"][0]["status"], "reachable");
        assert_eq!(body["checks"][2]["status"], "invalid_response");
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn ready_accepts_a_null_first_telemetry_timestamp_and_uses_setup_status()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = doctor_body("ready", "active");
    response["first_telemetry_seen_at"] = serde_json::Value::Null;
    mount_doctor(&server, 200, response).await;
    mount_logs(&server, 200, serde_json::json!([])).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "ready");
    assert_eq!(body["checks"][5]["status"], "seen");
    assert_private_values_absent(text.as_str(), &server);
    Ok(())
}

#[tokio::test]
async fn display_safe_open_signal_kind_is_accepted_but_never_rendered()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let mut response = doctor_body("ready", "active");
    response["last_signal"]["kind"] = serde_json::json!("hostile-secret-public-signal");
    mount_doctor(&server, 200, response).await;
    mount_logs(&server, 200, serde_json::json!([])).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "ready");
    assert_private_values_absent(text.as_str(), &server);
    Ok(())
}

#[tokio::test]
async fn malformed_or_noncanonical_error_bodies_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            401,
            serde_json::json!({
                "error":"hostile-secret-error",
                "code":"unauthorized",
                "next":"hostile-secret-next",
                "next_action":{"code":"fix_request","target":"request"}
            }),
        ),
        (
            404,
            serde_json::json!({
                "error":"hostile-secret-error",
                "code":"not_found",
                "next":"hostile-secret-next",
                "next_action":{"code":"check_resource","target":"resource"}
            }),
        ),
        (422, serde_json::json!({"error":"hostile-secret-error"})),
    ];

    for (status, response) in cases {
        let server = MockServer::start().await;
        mount_doctor(&server, status, response).await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "check_failed");
        assert_eq!(body["checks"][2]["status"], "invalid_response");
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn unauthorized_uses_the_stable_code_and_action_without_rendering_server_text()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_doctor(
        &server,
        401,
        serde_json::json!({
            "error":"hostile-secret-auth-error",
            "code":"unauthorized",
            "next":"hostile-secret-auth-next",
            "next_action":{"code":"sign_in","target":"auth"}
        }),
    )
    .await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "auth_invalid");
    assert_eq!(body["next"], "run logbrew login");
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 1);
    Ok(())
}

#[tokio::test]
async fn non_json_and_oversized_doctor_bodies_fail_closed() -> Result<(), Box<dyn std::error::Error>>
{
    for raw in [
        "hostile-secret-non-json".to_owned(),
        "x".repeat(256 * 1024 + 1),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/api/projects/{PROJECT_ID}/doctor")))
            .and(header("authorization", format!("Bearer {TOKEN}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(raw))
            .expect(1)
            .mount(&server)
            .await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "check_failed");
        assert_eq!(body["checks"][2]["status"], "invalid_response");
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn typed_non_success_statuses_use_fixed_local_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    for status in [400, 403, 405, 422, 429, 500, 503] {
        let server = MockServer::start().await;
        mount_doctor(
            &server,
            status,
            standard_error(
                "request_failed",
                "hostile-secret-next",
                "hostile-secret-action",
                "request",
            ),
        )
        .await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "check_failed");
        assert_eq!(body["checks"][2]["status"], "error");
        assert_eq!(
            body["next"],
            "retry logbrew doctor --project <project_id>; if it repeats, report the public response contract"
        );
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn malformed_log_probe_fails_closed_without_overriding_canonical_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    for response in [
        serde_json::json!({"logs":[],"hostile-secret":"value"}),
        serde_json::json!(["hostile-secret-row"]),
        serde_json::json!([{}]),
        serde_json::json!([{"message":"hostile-secret-row"}]),
    ] {
        let server = MockServer::start().await;
        mount_doctor(&server, 200, doctor_body("ready", "active")).await;
        mount_logs(&server, 200, response).await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "ready");
        assert_eq!(body["checks"][5]["status"], "seen");
        assert_eq!(body["checks"][6]["status"], "invalid_response");
        assert_private_values_absent(text.as_str(), &server);
        assert_eq!(request_count(&server).await?, 2);
    }
    Ok(())
}

#[tokio::test]
async fn optional_log_auth_rejection_does_not_override_canonical_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_doctor(&server, 200, doctor_body("ready", "active")).await;
    mount_logs(&server, 401, unauthorized_error()).await;

    let text = run(&server, true).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "ready");
    assert_eq!(body["checks"][1]["status"], "valid");
    assert_eq!(body["checks"][6]["status"], "unavailable");
    assert_eq!(
        body["next"],
        "inspect recent project logs, issues, actions, releases, or traces"
    );
    assert_private_values_absent(text.as_str(), &server);
    assert_eq!(request_count(&server).await?, 2);
    Ok(())
}

#[tokio::test]
async fn log_probe_errors_are_value_safe_and_do_not_reconstruct_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    for status in [422, 429, 500] {
        let server = MockServer::start().await;
        mount_doctor(&server, 200, doctor_body("needs_telemetry", "sdk_seen")).await;
        mount_logs(
            &server,
            status,
            standard_error(
                "request_failed",
                "hostile-secret-next",
                "hostile-secret-action",
                "request",
            ),
        )
        .await;

        let text = run(&server, true).await?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["status"], "needs_telemetry");
        assert_eq!(body["checks"][6]["status"], "error");
        assert_eq!(
            body["next"],
            "send the first telemetry event for this project"
        );
        assert_private_values_absent(text.as_str(), &server);
    }
    Ok(())
}

#[tokio::test]
async fn transport_failure_is_api_unreachable_and_value_safe()
-> Result<(), Box<dyn std::error::Error>> {
    let base_url = "http://127.0.0.1:0";

    let text = run_at(base_url, true, Some(TOKEN.to_owned()), None).await?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["status"], "api_unreachable");
    assert_eq!(body["checks"][0]["status"], "unreachable");
    assert_eq!(
        body["next"],
        "check network access, then retry logbrew doctor --project <project_id>"
    );
    assert!(!text.contains(base_url));
    assert!(!text.contains(TOKEN));
    Ok(())
}

async fn mount_doctor(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/projects/{PROJECT_ID}/doctor")))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_logs(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/api/logs"))
        .and(query_param("project_id", PROJECT_ID))
        .and(query_param("limit", "1"))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

fn doctor_body(state: &str, setup_status: &str) -> serde_json::Value {
    let setup_acknowledged = matches!(setup_status, "sdk_seen" | "first_telemetry_seen" | "active");
    let has_active_ingest_key = state != "needs_ingest_key";
    let telemetry_seen = matches!(setup_status, "first_telemetry_seen" | "active");
    let (next, code, target) = match state {
        "needs_ingest_key" => (
            "create an ingest key for this project",
            "create_ingest_key",
            "project_ingest_keys",
        ),
        "needs_setup" => (
            "choose an SDK or CLI setup path for this project",
            "choose_setup_path",
            "project_setup",
        ),
        "needs_telemetry" => (
            "send the first telemetry event for this project",
            "send_first_telemetry",
            "telemetry_ingest",
        ),
        "ready" => (
            "inspect recent telemetry for this project",
            "inspect_recent_telemetry",
            "telemetry_reads",
        ),
        _ => ("hostile-secret-next", "hostile-secret-code", "request"),
    };
    serde_json::json!({
        "project_id": PROJECT_ID,
        "state": state,
        "setup_status": setup_status,
        "setup_acknowledged": setup_acknowledged,
        "has_active_ingest_key": has_active_ingest_key,
        "first_telemetry_seen_at": telemetry_seen.then_some("2026-07-16T08:00:00Z"),
        "last_seen_at": telemetry_seen.then_some("2026-07-16T08:30:00Z"),
        "last_signal": telemetry_seen.then(|| serde_json::json!({
            "kind":"log",
            "id":null,
            "message":null,
            "occurred_at":"2026-07-16T08:30:00Z"
        })),
        "next": next,
        "next_action": {"code":code,"target":target}
    })
}

fn unauthorized_error() -> serde_json::Value {
    serde_json::json!({
        "error":"Invalid or expired token",
        "code":"unauthorized",
        "next":"send Authorization: Bearer <token>, include the logbrew_session cookie, or sign in again",
        "next_action":{"code":"sign_in","target":"auth"}
    })
}

fn project_not_found_error() -> serde_json::Value {
    serde_json::json!({
        "error":"project not found",
        "code":"not_found",
        "next":"check project_id or create a project with POST /api/projects",
        "next_action":{"code":"check_resource","target":"resource"}
    })
}

fn standard_error(code: &str, next: &str, action: &str, target: &str) -> serde_json::Value {
    serde_json::json!({
        "error":"request failed",
        "code":code,
        "next":next,
        "next_action":{"code":action,"target":target}
    })
}

async fn run(server: &MockServer, json: bool) -> Result<String, Box<dyn std::error::Error>> {
    run_project_at(
        server.uri().as_str(),
        PROJECT_ID,
        json,
        Some(TOKEN.to_owned()),
        None,
    )
    .await
}

async fn run_project(
    server: &MockServer,
    project_id: &str,
    json: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    run_project_at(
        server.uri().as_str(),
        project_id,
        json,
        Some(TOKEN.to_owned()),
        None,
    )
    .await
}

async fn run_with_home(
    server: &MockServer,
    json: bool,
    home: std::path::PathBuf,
) -> Result<String, Box<dyn std::error::Error>> {
    run_project_at(server.uri().as_str(), PROJECT_ID, json, None, Some(home)).await
}

async fn run_at(
    base_url: &str,
    json: bool,
    token: Option<String>,
    home: Option<std::path::PathBuf>,
) -> Result<String, Box<dyn std::error::Error>> {
    run_project_at(base_url, PROJECT_ID, json, token, home).await
}

async fn run_project_at(
    base_url: &str,
    project_id: &str,
    json: bool,
    token: Option<String>,
    home: Option<std::path::PathBuf>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["logbrew", "doctor", "--project", project_id];
    if json {
        args.push("--json");
    }
    let command = parse_command(args)?;
    let env = CliEnvironment {
        base_url: base_url.to_owned(),
        token,
        home: Some(home.unwrap_or_else(|| unique_home_from_url(base_url, "env-token"))),
        cwd: None,
    };
    let mut output = Vec::new();
    execute_command(&command, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

fn local_auth_home(server: &MockServer) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let home = unique_home(server, "local-auth");
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

fn unique_home(server: &MockServer, label: &str) -> std::path::PathBuf {
    unique_home_from_url(server.uri().as_str(), label)
}

fn unique_home_from_url(base_url: &str, label: &str) -> std::path::PathBuf {
    let suffix = base_url
        .rsplit(':')
        .next()
        .unwrap_or("none")
        .replace('/', "-");
    std::env::temp_dir().join(format!(
        "logbrew-project-doctor-{label}-{}-{suffix}",
        std::process::id()
    ))
}

async fn requests(
    server: &MockServer,
) -> Result<Vec<wiremock::Request>, Box<dyn std::error::Error>> {
    server
        .received_requests()
        .await
        .ok_or_else(|| "wiremock request recording is enabled".into())
}

async fn request_count(server: &MockServer) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(requests(server).await?.len())
}

fn assert_private_values_absent(text: &str, server: &MockServer) {
    for private in [
        TOKEN,
        PROJECT_ID,
        UPPER_PROJECT_ID,
        OTHER_PROJECT_ID,
        "local-access",
        "local-refresh",
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
