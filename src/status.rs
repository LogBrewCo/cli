//! Status command execution and diagnostics.

use crate::auth::{inspect_auth_snapshot, write_status_success};
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

    write_status_success(env, status.as_u16(), body.as_str(), json, output)
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
