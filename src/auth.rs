//! Local CLI authentication helpers and status rendering.

use crate::{CliEnvironment, RuntimeError};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;

/// Next step after status confirms both API reachability and local auth.
const AUTHENTICATED_STATUS_NEXT: &str =
    "run logbrew releases or logbrew logs --release <release> --environment <environment>";
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthCredential {
    /// Bearer token value. Never render this in user output.
    pub(crate) token: String,
    /// Stable machine-readable auth source key.
    pub(crate) source: &'static str,
    /// Concise human auth label.
    pub(crate) label: &'static str,
}

/// Resolves the bearer token and redacted source from env or local file.
pub(crate) fn resolve_credential(env: &CliEnvironment) -> Result<AuthCredential, RuntimeError> {
    if let Some(token) = env.token.as_ref().filter(|token| !token.trim().is_empty()) {
        return Ok(AuthCredential {
            token: token.clone(),
            source: AuthSource::Env.key(),
            label: AuthSource::Env.human_label(),
        });
    }

    let Some(home) = &env.home else {
        return Err(RuntimeError::MissingToken);
    };
    Ok(AuthCredential {
        token: read_token_from_home(home)?,
        source: AuthSource::TokenFile.key(),
        label: AuthSource::TokenFile.human_label(),
    })
}

/// Persists a local CLI token below the user's home directory.
pub(crate) fn persist_token_to_home(
    home: Option<&std::path::Path>,
    token: &str,
) -> Result<(), RuntimeError> {
    let Some(home) = home else {
        return Err(RuntimeError::Unavailable {
            message: "could not save login without a home directory",
            next: "set HOME or use LOGBREW_TOKEN",
        });
    };
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Unavailable {
            message: "login response did not include a usable token",
            next: "retry logbrew login",
        });
    }

    let config_dir = home.join(".logbrew");
    std::fs::create_dir_all(config_dir.as_path())?;
    let token_path = config_dir.join("token");
    let mut options = std::fs::OpenOptions::new();
    let options = options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    let options = options.mode(0o600);
    std::io::Write::write_all(&mut options.open(token_path)?, trimmed.as_bytes())?;
    Ok(())
}

/// Opens a URL in the user's default browser.
pub(crate) fn open_browser(url: &str) -> bool {
    let command = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("rundll32", vec!["url.dll,FileProtocolHandler", url])
    } else {
        ("xdg-open", vec![url])
    };

    std::process::Command::new(command.0)
        .args(command.1)
        .status()
        .is_ok_and(|status| status.success())
}

/// Writes the successful `status` command response.
pub(crate) fn write_status_success<W: std::io::Write>(
    env: &CliEnvironment,
    status_code: u16,
    body: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let auth_status = inspect_auth_snapshot(env)?;
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

/// Removes the local CLI token and writes a redacted logout result.
pub(crate) fn write_logout_result<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let env_token_active = env
        .token
        .as_ref()
        .is_some_and(|token| !token.trim().is_empty());
    let removed = remove_token_from_home(env.home.as_deref())?;
    let auth_source = if env_token_active {
        "env"
    } else if removed {
        "token_file"
    } else {
        "missing"
    };
    let next = logout_next_step(env_token_active, removed);

    if json {
        let response = serde_json::json!({
            "ok": true,
            "removed": removed,
            "auth_source": auth_source,
            "env_token_active": env_token_active,
            "next": next,
        });
        writeln!(output, "{response}")?;
    } else if env_token_active {
        if removed {
            writeln!(output, "Local LogBrew token removed.")?;
        } else {
            writeln!(output, "No local LogBrew token found.")?;
        }
        writeln!(output, "Auth: env token still active")?;
        writeln!(output, "Next: {next}")?;
    } else if removed {
        writeln!(output, "Logged out of LogBrew.")?;
        writeln!(output, "Removed: local token")?;
        writeln!(output, "Next: {next}")?;
    } else {
        writeln!(output, "No local LogBrew token found.")?;
        writeln!(output, "Next: {next}")?;
    }
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
    match read_token_from_home(home) {
        Ok(_) => Ok(AuthStatus::Configured(AuthSource::TokenFile)),
        Err(RuntimeError::MissingToken) => Ok(AuthStatus::Missing),
        Err(error) => Err(error),
    }
}

/// Removes a persisted token if it exists.
fn remove_token_from_home(home: Option<&std::path::Path>) -> Result<bool, RuntimeError> {
    let Some(home) = home else {
        return Ok(false);
    };
    let token_path = home.join(".logbrew").join("token");
    match std::fs::remove_file(token_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(RuntimeError::Io(error)),
    }
}

/// Returns the next logout step without exposing credential material.
const fn logout_next_step(env_token_active: bool, removed: bool) -> &'static str {
    if env_token_active {
        "unset LOGBREW_TOKEN to fully log out"
    } else if removed {
        "run logbrew login to authenticate again"
    } else {
        "run logbrew login to authenticate"
    }
}

/// Reads a persisted CLI token from a home directory.
fn read_token_from_home(home: &std::path::Path) -> Result<String, RuntimeError> {
    let token_path = home.join(".logbrew").join("token");
    let token = match std::fs::read_to_string(token_path) {
        Ok(token) => token,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(RuntimeError::MissingToken);
        }
        Err(error) => return Err(RuntimeError::Io(error)),
    };
    let trimmed = token.trim();
    if trimmed.is_empty() {
        Err(RuntimeError::MissingToken)
    } else {
        Ok(trimmed.to_owned())
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

/// Source of a configured local credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthSource {
    /// `LOGBREW_TOKEN` process environment variable.
    Env,
    /// Persisted token file below the user home directory.
    TokenFile,
}

impl AuthSource {
    /// Returns the stable JSON key for this source.
    const fn key(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::TokenFile => "token_file",
        }
    }

    /// Returns a concise human status label for this source.
    const fn human_label(self) -> &'static str {
        match self {
            Self::Env => "logged in (env token)",
            Self::TokenFile => "logged in (local token)",
        }
    }
}
