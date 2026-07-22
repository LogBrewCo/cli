//! Status command execution and diagnostics.

use crate::auth::{
    AuthSnapshot, inspect_auth_snapshot, send_authenticated_with_refresh,
    write_status_success_with_auth_snapshot,
};
use crate::{CliEnvironment, RuntimeError};

/// Executes the local status check.
pub(crate) async fn execute_status<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let url = format!("{}/health", env.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) => {
            return Err(status_unavailable_error(
                env,
                None,
                None,
                error.to_string(),
            )?);
        }
    };
    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(status_unavailable_error(
            env,
            Some(status.as_u16()),
            Some(body),
            format!("health returned status {}", status.as_u16()),
        )?);
    }

    let auth = validated_auth_snapshot(&client, env).await?;
    write_status_success_with_auth_snapshot(
        env,
        status.as_u16(),
        body.as_str(),
        json,
        output,
        &auth,
    )
}

/// Validates configured auth against the account route instead of trusting token presence.
async fn validated_auth_snapshot(
    client: &reqwest::Client,
    env: &CliEnvironment,
) -> Result<AuthSnapshot, RuntimeError> {
    let url = format!("{}/api/auth/account", env.base_url.trim_end_matches('/'));
    let (response, credential) =
        match send_authenticated_with_refresh(client, env, |client, credential| {
            client.get(url.as_str()).bearer_auth(credential.token())
        })
        .await
        {
            Ok(result) => result,
            Err(RuntimeError::MissingToken) => return inspect_auth_snapshot(env),
            Err(RuntimeError::Http(error)) => {
                return Err(status_unavailable_error(
                    env,
                    None,
                    None,
                    error.to_string(),
                )?);
            }
            Err(error) => return Err(error),
        };
    let status = response.status();
    let body = response.text().await?;

    if status.is_success() {
        return Ok(AuthSnapshot {
            authenticated: true,
            source: credential.source(),
            label: credential.label(),
            next: "run logbrew releases or logbrew logs --release <release> --environment <environment>",
        });
    }

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(AuthSnapshot {
            authenticated: false,
            source: "expired",
            label: "token expired",
            next: "run logbrew login",
        });
    }

    Err(status_unavailable_error(
        env,
        Some(status.as_u16()),
        Some(credential.redact_response_body(body.as_str())),
        format!("account returned status {}", status.as_u16()),
    )?)
}

/// Builds a redacted status failure error.
fn status_unavailable_error(
    env: &CliEnvironment,
    status_code: Option<u16>,
    body: Option<String>,
    message: String,
) -> Result<RuntimeError, RuntimeError> {
    let auth = inspect_auth_snapshot(env)?;
    Ok(RuntimeError::StatusUnavailable {
        api_url: env.base_url.clone(),
        status_code,
        body,
        authenticated: auth.authenticated,
        auth_source: auth.source,
        auth_label: auth.label,
        message,
    })
}
