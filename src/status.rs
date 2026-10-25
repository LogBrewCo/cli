//! Status command execution and diagnostics.

use crate::auth::{
    AuthCredential, AuthSnapshot, ReadKeyScope, inspect_auth_snapshot,
    send_authenticated_with_refresh, token_is_project_read_key,
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
    let client = crate::http::api_client()?;

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

/// Validates configured auth against the account route, or a read key against its own
/// scope route, instead of trusting token presence.
async fn validated_auth_snapshot(
    client: &reqwest::Client,
    env: &CliEnvironment,
) -> Result<AuthSnapshot, RuntimeError> {
    let base = env.base_url.trim_end_matches('/');
    let (account_url, read_key_url) = (
        format!("{base}/api/auth/account"),
        format!("{base}/api/auth/read-key"),
    );
    let (response, credential) =
        match send_authenticated_with_refresh(client, env, |client, credential| {
            let url = if token_is_project_read_key(credential.token()) {
                &read_key_url
            } else {
                &account_url
            };
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
    let read_key = token_is_project_read_key(credential.token());

    if status.is_success() && read_key {
        let Some(scope) = read_key_scope(&credential, body.as_str()) else {
            return Err(status_unavailable_error(
                env,
                Some(status.as_u16()),
                None,
                "read key scope response was invalid".to_owned(),
            )?);
        };
        return Ok(AuthSnapshot {
            authenticated: true,
            source: credential.source(),
            label: if credential.source() == "env" {
                "(env token)"
            } else {
                "(local token)"
            },
            next: "run logbrew issues or logbrew read traces",
            read_key: Some(scope),
        });
    }

    if status.is_success() {
        return Ok(AuthSnapshot {
            authenticated: true,
            source: credential.source(),
            label: credential.label(),
            next: "run logbrew releases or logbrew logs --release <release> --environment <environment>",
            read_key: None,
        });
    }

    if read_key && status == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(AuthSnapshot {
            authenticated: false,
            source: "invalid_read_key",
            label: "read key invalid, expired, or revoked",
            next: "create a new read key with logbrew projects keys create <project_id> --kind \
                   read, then set LOGBREW_TOKEN to it",
            read_key: None,
        });
    }

    if !read_key
        && (status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN)
    {
        return Ok(AuthSnapshot {
            authenticated: false,
            source: "expired",
            label: "token expired",
            next: "run logbrew login",
            read_key: None,
        });
    }

    Err(status_unavailable_error(
        env,
        Some(status.as_u16()),
        Some(credential.redact_response_body(body.as_str())),
        format!("account returned status {}", status.as_u16()),
    )?)
}

/// Validates the read-key scope response; `None` for any other shape.
fn read_key_scope(credential: &AuthCredential, body: &str) -> Option<ReadKeyScope> {
    /// Exact `GET /api/auth/read-key` response.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Scope {
        /// Credential kind, always `read`.
        kind: String,
        /// Project UUID.
        project_id: String,
        /// Project name.
        project_name: String,
    }
    let scope = serde_json::from_str::<Scope>(body).ok()?;
    (scope.kind == "read"
        && crate::ids::is_uuid(&scope.project_id)
        && crate::http::nonempty_display_safe(&scope.project_name, 256)
        && !scope.project_name.contains(credential.token().trim()))
    .then_some(ReadKeyScope {
        project_id: scope.project_id,
        project_name: scope.project_name,
    })
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
