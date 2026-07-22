//! Refresh-family revocation and local logout rendering.

use super::{normalized_api_base, store};
use crate::{CliEnvironment, RuntimeError};

/// Stable server-side outcome reported without response details.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerSessionOutcome {
    Revoked,
    Inactive,
    NotApplicable,
    Unknown,
}

impl ServerSessionOutcome {
    const fn key(self) -> &'static str {
        match self {
            Self::Revoked => "revoked",
            Self::Inactive => "inactive",
            Self::NotApplicable => "not_applicable",
            Self::Unknown => "unknown",
        }
    }

    const fn human_label(self) -> Option<&'static str> {
        match self {
            Self::Revoked => Some("revoked"),
            Self::Inactive => Some("already inactive"),
            Self::Unknown => Some("not confirmed"),
            Self::NotApplicable => None,
        }
    }
}

/// Revokes a refresh-backed session when possible, then always clears local credentials.
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let env_token_active = env
        .token
        .as_ref()
        .is_some_and(|token| !token.trim().is_empty());
    let (removed, server_session) = logout_local_session(env).await?;
    write_result(env_token_active, removed, server_session, json, output)
}

/// Holds the credential lock across revocation so a newer login cannot be deleted.
async fn logout_local_session(
    env: &CliEnvironment,
) -> Result<(bool, ServerSessionOutcome), RuntimeError> {
    let Some(home) = env.home.clone() else {
        return Ok((false, ServerSessionOutcome::NotApplicable));
    };
    let lock = tokio::task::spawn_blocking(move || {
        store::CredentialStoreLock::exclusive_if_present(home.as_path())
    })
    .await
    .map_err(|_| RuntimeError::Unavailable {
        message: "local authentication lock failed",
        next: "retry logbrew logout",
    })??;
    let Some(lock) = lock else {
        return Ok((false, ServerSessionOutcome::NotApplicable));
    };

    let server_session = match lock.has_refresh_backed_session() {
        Ok(false) => ServerSessionOutcome::NotApplicable,
        Ok(true) => match normalized_api_base(env.base_url.as_str()) {
            Ok(origin) => match lock.read_credentials(origin.as_str()) {
                Ok(Some(credentials)) => {
                    revoke_refresh_family(origin.as_str(), credentials.refresh_token.as_str()).await
                }
                Ok(None) | Err(_) => ServerSessionOutcome::Unknown,
            },
            Err(_) => ServerSessionOutcome::Unknown,
        },
        Err(_) => ServerSessionOutcome::Unknown,
    };
    let removed = lock.remove_credentials()?;
    Ok((removed, server_session))
}

/// Calls the public logout endpoint without attaching an access-token bearer.
async fn revoke_refresh_family(base_url: &str, refresh_token: &str) -> ServerSessionOutcome {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return ServerSessionOutcome::Unknown;
    };
    let url = format!("{}/api/auth/logout", base_url.trim_end_matches('/'));
    let Ok(response) = client
        .post(url)
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
    else {
        return ServerSessionOutcome::Unknown;
    };
    match response.status().as_u16() {
        200 => match response.json::<serde_json::Value>().await {
            Ok(value) if is_exact_logout_response(&value) => ServerSessionOutcome::Revoked,
            Ok(_) | Err(_) => ServerSessionOutcome::Unknown,
        },
        401 => match response.json::<serde_json::Value>().await {
            Ok(value) if is_typed_unauthorized_response(&value) => ServerSessionOutcome::Inactive,
            Ok(_) | Err(_) => ServerSessionOutcome::Unknown,
        },
        _ => ServerSessionOutcome::Unknown,
    }
}

/// Treats only the public typed auth code as evidence that the family is inactive.
fn is_typed_unauthorized_response(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|response| response.get("code"))
        .and_then(serde_json::Value::as_str)
        == Some("unauthorized")
}

/// Accepts only the deployed token-free logout success surface.
fn is_exact_logout_response(value: &serde_json::Value) -> bool {
    let Some(response) = value.as_object().filter(|response| response.len() == 2) else {
        return false;
    };
    let Some(next_action) = response
        .get("next_action")
        .and_then(serde_json::Value::as_object)
        .filter(|next_action| next_action.len() == 2)
    else {
        return false;
    };
    response.get("revoked").and_then(serde_json::Value::as_bool) == Some(true)
        && next_action.get("code").and_then(serde_json::Value::as_str)
            == Some("clear_local_session")
        && next_action
            .get("target")
            .and_then(serde_json::Value::as_str)
            == Some("local_credentials")
}

/// Writes stable local state plus a redacted server-session classification.
fn write_result<W: std::io::Write>(
    env_token_active: bool,
    removed: bool,
    server_session: ServerSessionOutcome,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let auth_source = if env_token_active {
        "env"
    } else if removed {
        "token_file"
    } else {
        "missing"
    };
    let next = logout_next_step(env_token_active, removed, server_session);

    if json {
        let response = serde_json::json!({
            "ok": true,
            "removed": removed,
            "auth_source": auth_source,
            "env_token_active": env_token_active,
            "server_session": server_session.key(),
            "next": next,
        });
        writeln!(output, "{response}")?;
    } else if env_token_active {
        if removed {
            writeln!(output, "Local LogBrew token removed.")?;
        } else {
            writeln!(output, "No local LogBrew token found.")?;
        }
        if let Some(label) = server_session.human_label() {
            writeln!(output, "Server session: {label}")?;
        }
        writeln!(output, "Auth: env token still active")?;
        writeln!(output, "Next: {next}")?;
    } else if removed {
        writeln!(output, "Logged out of LogBrew.")?;
        writeln!(output, "Removed: local token")?;
        if let Some(label) = server_session.human_label() {
            writeln!(output, "Server session: {label}")?;
        }
        writeln!(output, "Next: {next}")?;
    } else {
        writeln!(output, "No local LogBrew token found.")?;
        writeln!(output, "Next: {next}")?;
    }
    Ok(())
}

/// Returns recovery that is honest when server revocation could not be confirmed.
const fn logout_next_step(
    env_token_active: bool,
    removed: bool,
    server_session: ServerSessionOutcome,
) -> &'static str {
    if env_token_active && matches!(server_session, ServerSessionOutcome::Unknown) {
        "unset LOGBREW_TOKEN, run logbrew login, then use logbrew support create --help if revocation must be confirmed"
    } else if env_token_active {
        "unset LOGBREW_TOKEN to fully log out"
    } else if removed && matches!(server_session, ServerSessionOutcome::Unknown) {
        "run logbrew login; then use logbrew support create --help if revocation must be confirmed"
    } else if removed {
        "run logbrew login to authenticate again"
    } else {
        "run logbrew login to authenticate"
    }
}
