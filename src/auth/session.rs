//! Authenticated request sessions and one-time local refresh.

use super::AuthCredential;
use crate::{CliEnvironment, RuntimeError};

/// Source of a configured local credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AuthSource {
    /// `LOGBREW_TOKEN` process environment variable.
    Env,
    /// Persisted token file below the user home directory.
    TokenFile,
}

impl AuthSource {
    /// Returns the stable JSON key for this source.
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::TokenFile => "token_file",
        }
    }

    /// Returns a concise human status label for this source.
    pub(super) const fn human_label(self) -> &'static str {
        match self {
            Self::Env => "logged in (env token)",
            Self::TokenFile => "logged in (local token)",
        }
    }
}

/// Resolves the bearer token and redacted source from env or local file.
fn resolve_credential(env: &CliEnvironment) -> Result<AuthCredential, RuntimeError> {
    if let Some(token) = env.token.as_ref().filter(|token| !token.trim().is_empty()) {
        return Ok(AuthCredential {
            token: token.clone(),
            source: AuthSource::Env.key(),
            label: AuthSource::Env.human_label(),
            refreshable: false,
        });
    }

    let Some(home) = &env.home else {
        return Err(RuntimeError::MissingToken);
    };
    let origin = super::normalized_api_base(env.base_url.as_str())?;
    Ok(local_credential(super::store::read_access_token(
        home,
        origin.as_str(),
    )?))
}

/// Sends one authenticated request and retries once after a local 401 refresh.
pub(super) async fn send_authenticated_with_refresh<F>(
    client: &reqwest::Client,
    env: &CliEnvironment,
    build_request: F,
) -> Result<(reqwest::Response, AuthCredential), RuntimeError>
where
    F: Fn(&reqwest::Client, &AuthCredential) -> reqwest::RequestBuilder,
{
    let credential = resolve_credential(env)?;
    send_with_refresh(
        client,
        env,
        credential,
        CredentialPolicy::AnyBearer,
        build_request,
    )
    .await
}

/// Sends one account-authenticated request and rejects project ingest keys locally.
pub(super) async fn send_account_authenticated_with_refresh<F>(
    client: &reqwest::Client,
    env: &CliEnvironment,
    build_request: F,
) -> Result<(reqwest::Response, AuthCredential), RuntimeError>
where
    F: Fn(&reqwest::Client, &AuthCredential) -> reqwest::RequestBuilder,
{
    let credential = resolve_credential(env)?;
    send_with_refresh(
        client,
        env,
        credential,
        CredentialPolicy::AccountOnly,
        build_request,
    )
    .await
}

/// Credential policy applied immediately before each request construction.
#[derive(Clone, Copy)]
enum CredentialPolicy {
    /// Preserve existing callers that intentionally accept any bearer shape.
    AnyBearer,
    /// Require account auth and reject project-scoped ingest keys.
    AccountOnly,
}

/// Sends with one resolved credential and performs the canonical single refresh.
async fn send_with_refresh<F>(
    client: &reqwest::Client,
    env: &CliEnvironment,
    credential: AuthCredential,
    policy: CredentialPolicy,
    build_request: F,
) -> Result<(reqwest::Response, AuthCredential), RuntimeError>
where
    F: Fn(&reqwest::Client, &AuthCredential) -> reqwest::RequestBuilder,
{
    enforce_credential_policy(&credential, policy)?;
    let response = build_request(client, &credential).send().await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        && let Some(refreshed) = refresh_local_credential(client, env, &credential).await?
    {
        enforce_credential_policy(&refreshed, policy)?;
        let response = build_request(client, &refreshed).send().await?;
        return Ok((response, refreshed));
    }
    Ok((response, credential))
}

/// Rejects a disallowed credential before it can reach request construction.
fn enforce_credential_policy(
    credential: &AuthCredential,
    policy: CredentialPolicy,
) -> Result<(), RuntimeError> {
    if matches!(policy, CredentialPolicy::AccountOnly)
        && super::token_is_project_ingest_key(Some(credential.token.as_str()))
    {
        return Err(RuntimeError::Unavailable {
            message: "account authentication is required",
            next: "run logbrew login and retry the native debug-artifact command",
        });
    }
    Ok(())
}

/// Sends one authenticated request without refreshing or persisting credentials.
pub(super) async fn send_authenticated_without_refresh<F>(
    client: &reqwest::Client,
    env: &CliEnvironment,
    build_request: F,
) -> Result<(reqwest::Response, AuthCredential), RuntimeError>
where
    F: Fn(&reqwest::Client, &AuthCredential) -> reqwest::RequestBuilder,
{
    let credential = resolve_credential(env)?;
    let response = build_request(client, &credential).send().await?;
    Ok((response, credential))
}

/// Refreshes a rejected local credential while serializing other CLI processes.
async fn refresh_local_credential(
    client: &reqwest::Client,
    env: &CliEnvironment,
    rejected: &AuthCredential,
) -> Result<Option<AuthCredential>, RuntimeError> {
    if !rejected.refreshable || super::token_is_project_ingest_key(Some(rejected.token.as_str())) {
        return Ok(None);
    }
    let Some(home) = env.home.clone() else {
        return Ok(None);
    };
    let origin = super::normalized_api_base(env.base_url.as_str())?;
    let lock = tokio::task::spawn_blocking(move || {
        super::store::CredentialStoreLock::exclusive(home.as_path())
    })
    .await
    .map_err(|_| RuntimeError::Unavailable {
        message: "local authentication lock failed",
        next: "retry the command or run logbrew login",
    })??;
    let Some(current) = lock.read_credentials(origin.as_str())? else {
        return Ok(None);
    };

    if current.access_token != rejected.token {
        return Ok(Some(local_credential(current.access_token)));
    }

    let url = format!("{}/api/auth/refresh", env.base_url.trim_end_matches('/'));
    let response = client
        .post(url)
        .json(&serde_json::json!({ "refresh_token": current.refresh_token }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let value = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| invalid_refresh_response())?;
    let access_token = required_refresh_field(&value, "access_token")?;
    let refresh_token = required_refresh_field(&value, "refresh_token")?;
    lock.persist(
        access_token.as_str(),
        refresh_token.as_str(),
        origin.as_str(),
    )?;
    Ok(Some(local_credential(access_token)))
}

/// Builds redacted source metadata for a persisted access token.
const fn local_credential(token: String) -> AuthCredential {
    AuthCredential {
        token,
        source: AuthSource::TokenFile.key(),
        label: AuthSource::TokenFile.human_label(),
        refreshable: true,
    }
}

/// Extracts one non-empty token field from a refresh response.
fn required_refresh_field(
    value: &serde_json::Value,
    field: &'static str,
) -> Result<String, RuntimeError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(invalid_refresh_response)
}

/// Returns a stable, secret-free invalid refresh response error.
const fn invalid_refresh_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "authentication refresh returned an invalid response",
        next: "run logbrew login",
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthCredential, CredentialPolicy, enforce_credential_policy};

    /// Proves the account-only boundary uses the canonical whitespace-tolerant predicate.
    #[test]
    fn account_policy_rejects_every_ingest_key_shape() {
        for token in ["lbw_ingest_private", " \t lbw_ingest_private"] {
            let credential = credential(token);
            assert!(enforce_credential_policy(&credential, CredentialPolicy::AccountOnly).is_err());
            assert!(enforce_credential_policy(&credential, CredentialPolicy::AnyBearer).is_ok());
        }
        assert!(
            enforce_credential_policy(
                &credential("account-access-token"),
                CredentialPolicy::AccountOnly,
            )
            .is_ok()
        );
    }

    /// Builds one secret-redacting test credential.
    fn credential(token: &str) -> AuthCredential {
        AuthCredential {
            token: token.to_owned(),
            source: "test",
            label: "test",
            refreshable: true,
        }
    }
}
