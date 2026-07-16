//! Local CLI authentication helpers and status rendering.

use crate::{CliEnvironment, RuntimeError};

mod login;
mod logout;
mod session;
mod store;

use session::AuthSource;

/// Next step after status confirms both API reachability and local auth.
const AUTHENTICATED_STATUS_NEXT: &str =
    "run logbrew releases or logbrew logs --release <release> --environment <environment>";

/// Canonicalizes the API base used to bind persisted credentials to one origin.
fn normalized_api_base(base_url: &str) -> Result<String, RuntimeError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| invalid_api_url())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_api_url());
    }
    let normalized_path = url.path().trim_end_matches('/').to_owned();
    url.set_path(normalized_path.as_str());
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// Returns a stable error for an API base that cannot safely own credentials.
const fn invalid_api_url() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "the configured API URL is invalid",
        next: "set LOGBREW_API_URL to an http or https API base and retry",
    }
}
/// Redacted local authentication status for CLI diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuthSnapshot {
    /// Whether a local credential is configured.
    pub(crate) authenticated: bool,
    /// Stable machine-readable auth source key.
    pub(crate) source: &'static str,
    /// Concise human auth label.
    pub(crate) label: &'static str,
    /// Suggested next step for the current auth state.
    pub(crate) next: &'static str,
}

/// Bearer token plus redacted source metadata for API calls.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AuthCredential {
    /// Bearer token value. Never render this in user output.
    token: String,
    /// Stable machine-readable auth source key.
    source: &'static str,
    /// Concise human auth label.
    label: &'static str,
    /// Whether this credential may use the persisted refresh token.
    refreshable: bool,
}

impl AuthCredential {
    /// Returns the bearer value for request construction only.
    pub(crate) const fn token(&self) -> &str {
        self.token.as_str()
    }

    /// Returns the stable redacted source key.
    pub(crate) const fn source(&self) -> &'static str {
        self.source
    }

    /// Returns the concise redacted source label.
    pub(crate) const fn label(&self) -> &'static str {
        self.label
    }

    /// Redacts the exact bearer token if an upstream error echoes it.
    pub(crate) fn redact_response_body(&self, body: &str) -> String {
        body.replace(self.token.as_str(), "[redacted]")
    }
}

impl std::fmt::Debug for AuthCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthCredential")
            .field("token", &"[redacted]")
            .field("source", &self.source)
            .field("label", &self.label)
            .field("refreshable", &self.refreshable)
            .finish()
    }
}

/// Executes interactive login or a non-mutating handoff mode.
pub(crate) async fn execute_login<W: std::io::Write>(
    env: &CliEnvironment,
    should_open_browser: bool,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    login::execute_login(env, should_open_browser, json, output).await
}

/// Revokes the stored server session when possible, then clears local credentials.
pub(crate) async fn execute_logout<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    logout::execute(env, json, output).await
}

/// Sends one authenticated request and retries once after a local 401 refresh.
pub(crate) async fn send_authenticated_with_refresh<F>(
    client: &reqwest::Client,
    env: &CliEnvironment,
    build_request: F,
) -> Result<(reqwest::Response, AuthCredential), RuntimeError>
where
    F: Fn(&reqwest::Client, &AuthCredential) -> reqwest::RequestBuilder,
{
    session::send_authenticated_with_refresh(client, env, build_request).await
}

/// Writes the successful `status` command response with already validated auth metadata.
pub(crate) fn write_status_success_with_auth_snapshot<W: std::io::Write>(
    env: &CliEnvironment,
    status_code: u16,
    body: &str,
    json: bool,
    output: &mut W,
    auth_status: &AuthSnapshot,
) -> Result<(), RuntimeError> {
    if json {
        let mut response = serde_json::json!({
            "ok": true,
            "status": "reachable",
            "status_code": status_code,
            "body": body,
            "api_url": env.base_url,
            "authenticated": auth_status.authenticated,
            "auth_source": auth_status.source,
            "next": auth_status.next,
        });
        if auth_status.authenticated {
            if let Some(object) = response.as_object_mut() {
                drop(object.insert("agent_use".to_owned(), authenticated_agent_use()));
            }
        }
        writeln!(output, "{response}")?;
    } else {
        writeln!(output, "LogBrew API reachable.")?;
        writeln!(output, "API: {}", env.base_url)?;
        writeln!(output, "Auth: {}", auth_status.label)?;
        if auth_status.authenticated {
            write_authenticated_agent_use_prompt(output)?;
        }
        writeln!(output, "Next: {}", auth_status.next)?;
    }
    Ok(())
}

/// Agent-readable user choice prompt for authenticated status responses.
fn authenticated_agent_use() -> serde_json::Value {
    serde_json::json!({
        "prompt": "How should your AI use LogBrew?",
        "default": "on_request",
        "options": [
            {
                "id": "on_request",
                "label": "Check only when requested",
                "token_use": "lower",
                "available": true,
                "description": "Your AI runs LogBrew commands when you ask."
            },
            {
                "id": "keep_watching",
                "label": "Keep watching this session",
                "token_use": "higher",
                "available": true,
                "description": "Your AI watches new events/logs until stopped.",
                "command": "logbrew watch --json"
            },
            {
                "id": "watch_errors_critical",
                "label": "Watch only errors and critical issues",
                "token_use": "moderate",
                "available": true,
                "description": "Your AI ignores lower-severity logs/events.",
                "command": "logbrew watch --severity error,critical --json"
            }
        ]
    })
}

/// Writes the authenticated human prompt without implying live watch is ready.
fn write_authenticated_agent_use_prompt<W: std::io::Write>(output: &mut W) -> std::io::Result<()> {
    writeln!(output, "LogBrew is connected. How should your AI use it?")?;
    writeln!(output)?;
    writeln!(output, "1. Check only when requested")?;
    writeln!(
        output,
        "   Lower token use. Your AI runs LogBrew commands when you ask."
    )?;
    writeln!(output)?;
    writeln!(output, "2. Keep watching this session")?;
    writeln!(
        output,
        "   Higher token use. Your AI watches new events/logs until stopped."
    )?;
    writeln!(output, "   Command: logbrew watch --json")?;
    writeln!(output)?;
    writeln!(output, "3. Watch only errors and critical issues")?;
    writeln!(
        output,
        "   Moderate token use. Your AI ignores lower-severity logs/events."
    )?;
    writeln!(
        output,
        "   Command: logbrew watch --severity error,critical --json"
    )?;
    writeln!(output)?;
    Ok(())
}

/// Inspects local auth and returns only redacted status metadata.
pub(crate) fn inspect_auth_snapshot(env: &CliEnvironment) -> Result<AuthSnapshot, RuntimeError> {
    let status = inspect_auth_status(env)?;
    Ok(AuthSnapshot {
        authenticated: status.is_authenticated(),
        source: status.source_key(),
        label: status.human_label(),
        next: status.next_step(),
    })
}

/// Inspects whether a local auth credential is configured without exposing it.
fn inspect_auth_status(env: &CliEnvironment) -> Result<AuthStatus, RuntimeError> {
    if env
        .token
        .as_ref()
        .is_some_and(|token| !token.trim().is_empty())
    {
        return Ok(AuthStatus::Configured(AuthSource::Env));
    }

    let Some(home) = &env.home else {
        return Ok(AuthStatus::Missing);
    };
    let origin = normalized_api_base(env.base_url.as_str())?;
    match store::read_access_token(home, origin.as_str()) {
        Ok(_) => Ok(AuthStatus::Configured(AuthSource::TokenFile)),
        Err(RuntimeError::MissingToken) => Ok(AuthStatus::Missing),
        Err(error) => Err(error),
    }
}

/// Local authentication state reported by `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthStatus {
    /// A local credential is configured.
    Configured(AuthSource),
    /// No local credential was found.
    Missing,
}

impl AuthStatus {
    /// Returns whether a local credential is configured.
    const fn is_authenticated(self) -> bool {
        matches!(self, Self::Configured(_))
    }

    /// Returns the stable JSON source key.
    const fn source_key(self) -> &'static str {
        match self {
            Self::Configured(source) => source.key(),
            Self::Missing => "missing",
        }
    }

    /// Returns a concise human status label.
    const fn human_label(self) -> &'static str {
        match self {
            Self::Configured(source) => source.human_label(),
            Self::Missing => "not logged in",
        }
    }

    /// Returns the next action for the current authentication state.
    const fn next_step(self) -> &'static str {
        match self {
            Self::Configured(_) => AUTHENTICATED_STATUS_NEXT,
            Self::Missing => "run logbrew login",
        }
    }
}
