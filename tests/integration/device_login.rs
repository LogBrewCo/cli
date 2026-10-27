//! Device sign-in: one link and one code, polling at the server's interval, and
//! the same stored session as browser login.

use crate::{Mock, MockServer, Request, ResponseTemplate, TestResult, execute_command};
use logbrew_cli::{CliEnvironment, RuntimeError, parse_command};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Secret the CLI polls with; it must never reach output.
const DEVICE_CODE: &str = "device-secret-code";

/// Mounts the device sign-in start with a 1-second polling interval.
pub(super) fn mount_start(server: &MockServer, expires_in: u64) {
    let link = format!("{}/device", server.uri());
    Mock::route("POST", "/api/auth/device")
        .respond_with(
            ResponseTemplate::new(200).set_body_json(&serde_json::json!({
                "device_code": DEVICE_CODE,
                "user_code": "BCDF-GHJK",
                "verification_uri": link,
                "verification_uri_complete": format!("{link}?user_code=BCDF-GHJK"),
                "expires_in": expires_in,
                "interval": 1
            })),
        )
        .expect(1)
        .mount(server);
}

/// Answers the polls in order, repeating the last answer.
pub(super) fn mount_polls(server: &MockServer, answers: Vec<ResponseTemplate>) {
    let calls = AtomicUsize::new(0);
    Mock::route("POST", "/api/auth/device/token")
        .respond_with(move |_request: &Request| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            answers
                .get(call)
                .or_else(|| answers.last())
                .cloned()
                .unwrap_or_else(|| ResponseTemplate::new(500))
        })
        .mount(server);
}

/// The token response of an approved sign-in.
pub(super) fn tokens() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(&serde_json::json!({
        "access_token": "device-access",
        "token_type": "Bearer",
        "expires_in": 900,
        "refresh_token": "device-refresh",
        "user": { "id": "user-1" }
    }))
}

/// One RFC 8628 token error.
fn rejected(error: &str) -> ResponseTemplate {
    ResponseTemplate::new(400).set_body_json(&serde_json::json!({ "error": error }))
}

/// Output printed before polling starts.
pub(super) fn waiting(server: &MockServer) -> String {
    format!(
        "Open this link to sign in: {}/device?user_code=BCDF-GHJK\nCode: BCDF-GHJK\nWaiting for \
         approval...\n",
        server.uri()
    )
}

/// Checks the stored session and that no secret reached `output`.
pub(super) fn assert_signed_in(
    home: &std::path::Path,
    server: &MockServer,
    output: &str,
) -> TestResult {
    for secret in [DEVICE_CODE, "device-access", "device-refresh"] {
        assert!(!output.contains(secret), "device sign-in leaked a secret");
    }
    let saved: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(home.join(".logbrew/session.json"))?.as_str(),
    )?;
    assert_eq!(
        (
            &saved["access_token"],
            &saved["refresh_token"],
            &saved["origin"]
        ),
        (
            &serde_json::json!("device-access"),
            &serde_json::json!("device-refresh"),
            &serde_json::json!(server.uri())
        ),
        "stored like browser login"
    );
    Ok(())
}

fn env(server: &MockServer, home: &std::path::Path) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: None,
        home: Some(home.to_path_buf()),
        cwd: None,
    }
}

fn form_body(requests: &[Request], path: &str) -> TestResult<String> {
    let request = requests
        .iter()
        .find(|request| request.url.path() == path)
        .ok_or("missing request")?;
    assert_eq!(
        request
            .headers
            .get("content-type")
            .map(reqwest::header::HeaderValue::as_bytes),
        Some(&b"application/x-www-form-urlencoded"[..]),
        "form request"
    );
    Ok(String::from_utf8(request.body.clone())?)
}

async_test!(device_login_prints_one_link_and_one_code_then_stores_the_session -> TestResult, {
    for (args, label, polls) in [
        (
            &["logbrew", "login", "--device"][..],
            "device",
            vec![rejected("authorization_pending"), ResponseTemplate::new(503), tokens()],
        ),
        (
            &["logbrew", "login", "--provider", "gitlab", "--no-open"][..],
            "no-open",
            vec![tokens()],
        ),
    ] {
        let server = MockServer::start().await?;
        mount_start(&server, 60);
        mount_polls(&server, polls);
        let home = crate::isolated_home("logbrew-cli-device", label)?;
        let mut output = Vec::new();

        execute_command(&parse_command(args.iter().copied())?, &env(&server, &home), &mut output)
            .await?;

        let text = String::from_utf8(output)?;
        assert_eq!(
            text,
            format!("{}Logged in to LogBrew.\nNext: run logbrew status\n", waiting(&server)),
            "{label}"
        );
        assert_signed_in(home.as_path(), &server, text.as_str())?;
        let requests = server.received_requests();
        assert_eq!(
            form_body(&requests, "/api/auth/device")?,
            "client_id=logbrew-cli&client_name=LogBrew+CLI",
            "start request"
        );
        assert_eq!(
            form_body(&requests, "/api/auth/device/token")?,
            format!(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code&device_code=\
                 {DEVICE_CODE}&client_id=logbrew-cli"
            ),
            "poll request"
        );
        assert!(
            requests.iter().all(|request| request.url.path() != "/api/auth/providers"),
            "device sign-in lets the person pick any provider in the browser"
        );
    }
    Ok(())
});

async_test!(device_login_json_prints_the_link_and_code_then_the_result -> TestResult, {
    let server = MockServer::start().await?;
    mount_start(&server, 60);
    mount_polls(&server, vec![tokens()]);
    let home = crate::isolated_home("logbrew-cli-device", "json")?;
    let mut output = Vec::new();

    execute_command(&parse_command(["logbrew", "login", "--json"])?, &env(&server, &home), &mut output)
        .await?;

    let text = String::from_utf8(output)?;
    let lines = text
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        lines,
        [
            serde_json::json!({
                "ok": true,
                "status": "waiting_for_approval",
                "link": format!("{}/device?user_code=BCDF-GHJK", server.uri()),
                "user_code": "BCDF-GHJK",
                "expires_in": 60,
                "next": "open link in a browser, check that it shows user_code, and approve"
            }),
            serde_json::json!({ "ok": true, "status": "logged_in", "next": "run logbrew status" }),
        ],
        "one line to act on, one line with the result"
    );
    assert_signed_in(home.as_path(), &server, text.as_str())
});

async_test!(device_login_stops_on_denial_expiry_or_a_bad_answer_without_a_session -> TestResult, {
    for (label, poll, message) in [
        ("denied", rejected("access_denied"), "the sign-in was denied"),
        ("expired", rejected("expired_token"), "the sign-in code expired"),
        ("refused", rejected("invalid_grant"), "device sign-in could not be completed"),
        (
            "no-refresh",
            ResponseTemplate::new(200)
                .set_body_json(&serde_json::json!({ "access_token": "device-access" })),
            "device sign-in returned an invalid response",
        ),
    ] {
        let server = MockServer::start().await?;
        mount_start(&server, 60);
        mount_polls(&server, vec![poll]);
        let home = crate::isolated_home("logbrew-cli-device", label)?;
        let mut output = Vec::new();

        let error = execute_command(&parse_command(["logbrew", "login", "--device"])?, &env(&server, &home), &mut output)
            .await
            .err()
            .ok_or("the sign-in must fail")?;

        assert!(
            matches!(error, RuntimeError::Unavailable { message: found, .. } if found == message),
            "{label}: {error:?}"
        );
        assert_eq!(String::from_utf8(output)?, waiting(&server), "{label}");
        assert!(!home.join(".logbrew/session.json").exists(), "{label}: nothing stored");
    }
    Ok(())
});

async_test!(device_login_gives_up_when_the_code_expires_or_the_api_stops_answering -> TestResult, {
    let server = MockServer::start().await?;
    mount_start(&server, 1);
    Mock::route("POST", "/api/auth/device/token")
        .respond_with(tokens())
        .expect(0)
        .mount(&server);
    let home = crate::isolated_home("logbrew-cli-device", "deadline")?;
    let error = execute_command(&parse_command(["logbrew", "login", "--device"])?, &env(&server, &home), &mut Vec::new())
        .await
        .err()
        .ok_or("an expired code must fail")?;
    assert!(
        matches!(error, RuntimeError::Unavailable { message: "the sign-in code expired", .. }),
        "{error:?}"
    );

    let server = MockServer::start().await?;
    mount_start(&server, 60);
    mount_polls(&server, vec![ResponseTemplate::new(502)]);
    let error = execute_command(&parse_command(["logbrew", "login", "--device"])?, &env(&server, &home), &mut Vec::new())
        .await
        .err()
        .ok_or("an API that stops answering must fail")?;
    assert!(
        matches!(
            error,
            RuntimeError::Unavailable {
                message: "the LogBrew API stopped answering during device sign-in",
                ..
            }
        ),
        "{error:?}"
    );
    let polls = server
        .received_requests()
        .iter()
        .filter(|request| request.url.path() == "/api/auth/device/token")
        .count();
    assert_eq!(polls, 5, "5 unanswered polls in a row");
    assert!(!home.join(".logbrew/session.json").exists(), "nothing stored");
    Ok(())
});

async_test!(device_login_fails_closed_when_it_cannot_start_safely -> TestResult, {
    let unsafe_link = serde_json::json!({
        "device_code": DEVICE_CODE,
        "user_code": "BCDF-GHJK",
        "verification_uri": "https://user:pass@logbrew.example/device",
        "expires_in": 600
    });
    for (label, start, message) in [
        ("unavailable", ResponseTemplate::new(503), "device sign-in could not start"),
        (
            "unsafe-link",
            ResponseTemplate::new(200).set_body_json(&unsafe_link),
            "device sign-in returned an invalid response",
        ),
        (
            "oversized",
            ResponseTemplate::new(200).set_body_string("x".repeat(17 * 1024)),
            "device sign-in could not start",
        ),
    ] {
        let server = MockServer::start().await?;
        Mock::route("POST", "/api/auth/device")
            .respond_with(start)
            .expect(1)
            .mount(&server);
        let home = crate::isolated_home("logbrew-cli-device", label)?;
        let mut output = Vec::new();

        let error = execute_command(&parse_command(["logbrew", "login", "--json"])?, &env(&server, &home), &mut output)
            .await
            .err()
            .ok_or("an unsafe start must fail")?;

        assert!(
            matches!(error, RuntimeError::Unavailable { message: found, .. } if found == message),
            "{label}: {error:?}"
        );
        assert!(output.is_empty(), "{label}: nothing to act on");
        assert_eq!(server.received_requests().len(), 1, "{label}: no poll");
    }
    Ok(())
});
