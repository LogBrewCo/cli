//! Connected MCP clients: list the account's grants and revoke one.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes these private-module helpers"
)]

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::http::display_safe;
use crate::ids::is_uuid;
use crate::{CliEnvironment, CliError, RuntimeError};

/// Maximum accepted grant list response body.
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Requested list response version; version 2 adds `client_name`.
const LIST_RESPONSE_VERSION: u8 = 2;
/// Maximum grants shown in human output.
const HUMAN_ROW_LIMIT: usize = 100;

/// Exact grant list response.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantList {
    /// Active grants, most recently used first.
    grants: Vec<Grant>,
}

/// Exact grant row.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    /// Grant UUID used to revoke it.
    id: String,
    /// Client identifier (its metadata document URL).
    client_id: String,
    /// Approved client display name; `null` for grants approved before names were stored.
    client_name: Option<String>,
    /// Approved project UUIDs.
    project_ids: Vec<String>,
    /// Approval time.
    created_at: String,
    /// Latest token issue.
    last_used_at: String,
    /// End of the grant unless the client refreshes it.
    expires_at: String,
}

impl Grant {
    /// Whether every field holds the documented value shape.
    fn valid(&self) -> bool {
        is_uuid(self.id.as_str())
            && self.client_id.len() <= 2048
            && display_safe(self.client_id.as_str(), 2048)
            && !self.client_id.trim().is_empty()
            && self
                .client_name
                .as_deref()
                .is_none_or(|name| crate::http::nonempty_display_safe(name, 100))
            && self.project_ids.len() <= 50
            && self.project_ids.iter().all(|id| is_uuid(id.as_str()))
            && [&self.created_at, &self.last_used_at, &self.expires_at]
                .into_iter()
                .all(|time| crate::time::is_rfc3339_utc(time.as_str()))
    }
}

/// Lists the account's connected MCP clients.
pub(crate) async fn execute_list<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let (status, body, credential) = send(env, None).await?;
    if status != 200 {
        return Err(api_error(status, body.as_str(), &credential));
    }
    let list = serde_json::from_str::<GrantList>(body.as_str()).map_err(|_| invalid())?;
    if !list.grants.iter().all(Grant::valid) {
        return Err(invalid());
    }
    if json {
        writeln!(output, "{body}")?;
        return Ok(());
    }
    writeln!(output, "MCP grants ({})", list.grants.len())?;
    if list.grants.is_empty() {
        writeln!(output, "No connected MCP clients.")?;
        return Ok(());
    }
    for grant in list.grants.iter().take(HUMAN_ROW_LIMIT) {
        writeln!(
            output,
            "- id={} client={} projects={} last_used={} expires={}",
            grant.id,
            grant
                .client_name
                .as_deref()
                .unwrap_or(grant.client_id.as_str()),
            grant.project_ids.len(),
            grant.last_used_at,
            grant.expires_at
        )?;
    }
    if list.grants.len() > HUMAN_ROW_LIMIT {
        let omitted = list.grants.len() - HUMAN_ROW_LIMIT;
        writeln!(output, "{omitted} additional grants omitted.")?;
    }
    writeln!(output, "Next: run logbrew mcp revoke <grant_id> --yes")?;
    Ok(())
}

/// Revokes one grant: the client loses access at its next request.
pub(crate) async fn execute_revoke<W: std::io::Write>(
    env: &CliEnvironment,
    grant_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !is_uuid(grant_id) {
        return Err(CliError::InvalidMcpCommand.into());
    }
    let grant_id = grant_id.to_ascii_lowercase();
    let (status, body, credential) = send(env, Some(grant_id.as_str())).await?;
    if status != 204 || !body.is_empty() {
        return Err(api_error(status, body.as_str(), &credential));
    }
    if json {
        let receipt = serde_json::json!({
            "ok": true,
            "grant_id": grant_id,
            "status": "revoked",
            "next": "run logbrew mcp grants --json",
        });
        writeln!(output, "{receipt}")?;
    } else {
        writeln!(output, "MCP grant revoked: {grant_id}")?;
        writeln!(output, "The client must sign in again to reconnect.")?;
        writeln!(output, "Next: run logbrew mcp grants")?;
    }
    Ok(())
}

/// Sends the list request, or the revocation of `grant_id`, with account authentication.
async fn send(
    env: &CliEnvironment,
    grant_id: Option<&str>,
) -> Result<(u16, String, AuthCredential), RuntimeError> {
    let origin = crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport)?;
    let client = crate::http::api_client().map_err(|_| transport())?;
    let url = grant_id.map_or_else(
        || format!("{origin}/api/mcp/grants?response_version={LIST_RESPONSE_VERSION}"),
        |id| format!("{origin}/api/mcp/grants/{id}"),
    );
    let (response, credential) =
        send_account_authenticated_with_refresh(&client, env, |client, credential| {
            let request = if grant_id.is_some() {
                client.delete(url.as_str())
            } else {
                client.get(url.as_str())
            };
            request.bearer_auth(credential.token())
        })
        .await
        .map_err(|error| error.auth_or(transport()))?;
    let status = response.status().as_u16();
    let body = crate::http::bounded_body(response, RESPONSE_LIMIT)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => invalid(),
            crate::http::BodyError::Transport => transport(),
        })?;
    Ok((status, body, credential))
}

/// Validates a standard error envelope, then replaces all server text.
fn api_error(status: u16, body: &str, credential: &AuthCredential) -> RuntimeError {
    let Some(shape) = crate::http::ErrorShape::parse(body, status, 512, |text, limit| {
        text.len() <= limit && crate::http::nonempty_display_safe(text, usize::MAX)
    }) else {
        return invalid();
    };
    let not_found = status == 404 && shape.action() == ("not_found", "check_resource", "resource");
    if !not_found && !shape.matches_read_status(status) {
        return invalid();
    }
    let body = shape.rate_limit_body("mcp grants").unwrap_or_else(|| {
        let (code, next) = match status {
            401 => ("unauthorized", "run logbrew login"),
            404 => (
                "not_found",
                "run logbrew mcp grants and use a listed grant id",
            ),
            405 => ("method_not_allowed", "update logbrew and retry"),
            _ => ("server_error", "retry the same command later"),
        };
        serde_json::json!({"error": "MCP grant request failed", "code": code, "next": next})
            .to_string()
    });
    RuntimeError::Api {
        status,
        body,
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

/// Fixed recovery for a malformed response.
const fn invalid() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "MCP grant response was invalid",
        next: "retry logbrew mcp grants; if it repeats, report the public response contract",
    }
}

/// Fixed recovery for a failed request, without a URL.
const fn transport() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "MCP grant request could not be completed",
        next: "check network connectivity and retry the same command",
    }
}
