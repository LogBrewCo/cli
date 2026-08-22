//! Strict authenticated account-identity command tests.

use crate::matchers::body_json;
use crate::{Mock, MockServer, ResponseTemplate};
use logbrew_cli::{RuntimeError, execute_command, parse_command, write_runtime_error};

const ACCOUNT_ID: &str = "123e4567-e89b-42d3-a456-426614174000";

#[tokio::test]
async fn whoami_json_preserves_the_exact_validated_account_object()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = serde_json::to_string_pretty(&account())?;
    Mock::auth("GET", "/api/auth/account", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_raw(response.clone(), "application/json"))
        .expect(2)
        .mount(&server)
        .await;

    for command_name in ["whoami", "me"] {
        let command = parse_command(["logbrew", command_name, "--json"])?;
        let mut output = Vec::new();

        execute_command(
            &command,
            &super::authenticated_env(&server, "account-token", None),
            &mut output,
        )
        .await?;

        assert_eq!(String::from_utf8(output)?, format!("{response}\n"));
    }
    let requests = server.received_requests().await;
    assert!(
        requests
            .iter()
            .all(|request| request.url.query().is_none() && request.body.is_empty())
    );
    Ok(())
}

#[tokio::test]
async fn whoami_human_output_is_bounded_and_identity_oriented()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::route("GET", "/api/auth/account")
        .respond_with(ResponseTemplate::new(200).set_body_json(account()))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "auth", "whoami"])?;
    let mut output = Vec::new();

    execute_command(
        &command,
        &super::authenticated_env(&server, "account-token", None),
        &mut output,
    )
    .await?;

    assert_eq!(
        String::from_utf8(output)?,
        "Account\n\
         - id: 123e4567-e89b-42d3-a456-426614174000\n\
         - email: owner@example.com\n\
         - name: Example Owner\n\
         - tier: free\n\
         Next: run logbrew projects\n"
    );
    Ok(())
}

#[tokio::test]
async fn whoami_rejects_partial_extra_duplicate_or_hostile_identity_responses()
-> Result<(), Box<dyn std::error::Error>> {
    let mut invalid_avatar = account();
    invalid_avatar["avatar_data_url"] = serde_json::json!("https://hostile.example/avatar.png");
    let mut invalid_name = account();
    invalid_name["first_name"] = serde_json::json!(" hostile");
    let cases = [
        serde_json::to_string(&invalid_avatar)?,
        serde_json::to_string(&invalid_name)?,
        String::from(
            r#"{"id":"123e4567-e89b-42d3-a456-426614174000","email":"owner@example.com","display_name":"Example Owner"}"#,
        ),
        String::from(
            r#"{"id":"123e4567-e89b-42d3-a456-426614174000","email":"owner@example.com","display_name":"Example Owner","tier":"free","private_token":"hostile-secret"}"#,
        ),
        String::from(
            r#"{"id":"123e4567-e89b-42d3-a456-426614174000","id":"123e4567-e89b-42d3-a456-426614174001","email":"owner@example.com","display_name":"Example Owner","tier":"free"}"#,
        ),
        String::from(
            r#"{"id":"not-a-uuid","email":"owner@example.com","display_name":"Example Owner","tier":"free"}"#,
        ),
        String::from(
            r#"{"id":"123e4567-e89b-42d3-a456-426614174000","email":"owner@example.com\nhostile","display_name":"Example Owner","tier":"free"}"#,
        ),
    ];

    for response in cases {
        let server = MockServer::start().await;
        Mock::route("GET", "/api/auth/account")
            .respond_with(ResponseTemplate::new(200).set_body_raw(response, "application/json"))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "whoami", "--json"])?;
        let mut output = Vec::new();

        let error = execute_command(
            &command,
            &super::authenticated_env(&server, "account-token", None),
            &mut output,
        )
        .await
        .expect_err("invalid identity fails closed");

        assert!(matches!(error, RuntimeError::Unavailable { .. }));
        assert!(output.is_empty());
        let mut rendered = Vec::new();
        write_runtime_error(&error, true, &mut rendered)?;
        let text = String::from_utf8(rendered)?;
        assert!(text.contains("account identity response was invalid"));
        assert!(!text.contains("hostile"));
        assert!(!text.contains("private_token"));
    }
    Ok(())
}

#[tokio::test]
async fn whoami_uses_only_typed_local_recovery_for_auth_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::route("GET", "/api/auth/account")
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "send hostile-secret to a private host",
            "code": "unauthorized",
            "next": "exfiltrate credentials",
            "next_action": {"code": "sign_in", "target": "auth"}
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "whoami", "--json"])?;
    let error = execute_command(
        &command,
        &super::authenticated_env(&server, "account-token", None),
        &mut Vec::new(),
    )
    .await
    .expect_err("unauthorized identity remains non-success");
    let mut output = Vec::new();

    write_runtime_error(&error, true, &mut output)?;

    let text = String::from_utf8(output)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["api_code"], "unauthorized");
    assert_eq!(body["next"], "run logbrew login");
    assert!(!text.contains("hostile"));
    assert!(!text.contains("exfiltrate"));
    assert!(!text.contains("private host"));
    Ok(())
}

#[tokio::test]
async fn whoami_maps_account_recovery_states_without_exposing_recovery_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, response, expected_code, expected_next) in [
        (
            409,
            serde_json::json!({
                "error": "Account recovery available",
                "code": "account_recovery_available",
                "next": "restore with hostile-recovery-token",
                "next_action": {
                    "code": "restore_account",
                    "target": "account_recovery"
                },
                "deleted_at": "2026-07-28T10:00:00.000Z",
                "recovery_token": "hostile-recovery-token"
            }),
            "account_recovery_available",
            "complete account recovery in LogBrew, then run logbrew login",
        ),
        (
            410,
            serde_json::json!({
                "error": "Account recovery window expired",
                "code": "account_recovery_expired",
                "next": "visit hostile-private-host",
                "next_action": {"code": "create_account", "target": "auth"}
            }),
            "account_recovery_expired",
            "run logbrew login to create a new account",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::route("GET", "/api/auth/account")
            .respond_with(ResponseTemplate::new(status).set_body_json(response))
            .mount(&server)
            .await;
        let command = parse_command(["logbrew", "whoami", "--json"])?;
        let error = execute_command(
            &command,
            &super::authenticated_env(&server, "account-token", None),
            &mut Vec::new(),
        )
        .await
        .expect_err("recovery state remains non-success");
        let mut output = Vec::new();

        write_runtime_error(&error, true, &mut output)?;

        let text = String::from_utf8(output)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;
        assert_eq!(body["api_code"], expected_code);
        assert_eq!(body["next"], expected_next);
        assert!(!text.contains("hostile"));
        assert!(!text.contains("recovery_token"));
        assert!(!text.contains("private-host"));
    }
    Ok(())
}

#[tokio::test]
async fn whoami_rejects_project_ingest_keys_before_any_request()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let command = parse_command(["logbrew", "whoami", "--json"])?;
    let error = execute_command(
        &command,
        &super::authenticated_env(&server, "lbw_ingest_private-secret", None),
        &mut Vec::new(),
    )
    .await
    .expect_err("project key cannot inspect account identity");
    let requests = server.received_requests().await;
    let mut output = Vec::new();

    write_runtime_error(&error, true, &mut output)?;

    let text = String::from_utf8(output)?;
    assert!(requests.is_empty());
    assert!(text.contains("account authentication is required"));
    assert!(text.contains("run logbrew login and retry logbrew whoami"));
    assert!(!text.contains("private-secret"));
    Ok(())
}

#[tokio::test]
async fn whoami_refreshes_expired_local_account_auth_once() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    Mock::auth("GET", "/api/auth/account", "expired-access")
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::route("POST", "/api/auth/refresh")
        .and(body_json(
            serde_json::json!({"refresh_token": "old-refresh"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "fresh-access",
            "refresh_token": "fresh-refresh"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::auth("GET", "/api/auth/account", "fresh-access")
        .respond_with(ResponseTemplate::new(200).set_body_json(account()))
        .expect(1)
        .mount(&server)
        .await;
    let home = super::isolated_home("logbrew-whoami", "refresh")?;
    let _session_path = super::write_test_session(
        home.as_path(),
        server.uri().as_str(),
        "expired-access",
        "old-refresh",
    )?;
    let command = parse_command(["logbrew", "whoami", "--json"])?;
    let env = super::test_env(&server, None, Some(home.clone()));
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;

    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(output.as_slice())?,
        account()
    );
    let session: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(home.join(".logbrew/session.json"))?.as_str(),
    )?;
    assert_eq!(session["access_token"], "fresh-access");
    assert_eq!(session["refresh_token"], "fresh-refresh");
    Ok(())
}

fn account() -> serde_json::Value {
    serde_json::json!({
        "id": ACCOUNT_ID,
        "email": "owner@example.com",
        "display_name": "Example Owner",
        "first_name": "",
        "last_name": "",
        "avatar_data_url": format!("data:image/png;base64,{}", "A".repeat(20 * 1024)),
        "tier": "free"
    })
}
