//! Project key list, rename, archived list, and restore.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes these private-module helpers"
)]

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::project_key_revoke::KeyMetadata;
use crate::{CliEnvironment, RuntimeError};

/// Maximum accepted response body.
const RESPONSE_LIMIT: usize = 256 * 1024;
/// Maximum keys shown in human output.
const HUMAN_ROW_LIMIT: usize = 100;

/// Exact rename response.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Renamed {
    /// Renamed project UUID.
    id: String,
    /// Name after the change.
    name: String,
}

/// HTTP request a project command sends.
enum Request<'a> {
    /// Read a resource.
    Get,
    /// Change fields of a resource.
    Patch(&'a serde_json::Value),
    /// Run an action without a body.
    Post,
}

/// Archived project row; extra fields are refused.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedProject {
    /// Project UUID.
    id: String,
    /// Display name.
    name: String,
    /// Telemetry service name.
    service_name: String,
    /// Creation time.
    created_at: String,
    /// Archive time.
    archived_at: String,
    /// Fixed restore action.
    next_action: serde_json::Value,
}

impl ArchivedProject {
    /// Whether every field is display-safe and well formed.
    fn valid(&self) -> bool {
        crate::ids::is_uuid(&self.id)
            && crate::http::nonempty_display_safe(&self.name, 480)
            && crate::http::nonempty_display_safe(&self.service_name, 480)
            && crate::time::is_rfc3339_utc(&self.created_at)
            && crate::time::is_rfc3339_utc(&self.archived_at)
            && self.next_action
                == serde_json::json!({"code": "restore_project", "target": "project_restore"})
    }
}

/// Newest-first archived project page.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedPage {
    /// Archived projects, newest first.
    projects: Vec<ArchivedProject>,
    /// Present when older archived projects exist.
    next_cursor: Option<serde_json::Value>,
}

/// Lists one project's keys without secret material.
pub(crate) async fn execute_key_list<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let path = format!("/api/projects/{project_id}/ingest-keys");
    let (status, body, credential) = send(env, path.as_str(), Request::Get).await?;
    if status != 200 {
        return Err(api_error(status, body.as_str(), "keys", &credential));
    }
    let keys = serde_json::from_str::<Vec<KeyMetadata>>(body.as_str()).map_err(|_| invalid())?;
    if !keys.iter().all(KeyMetadata::valid) {
        return Err(invalid());
    }
    if json {
        writeln!(output, "{body}")?;
        return Ok(());
    }
    writeln!(output, "Project keys ({})", keys.len())?;
    for key in keys.iter().take(HUMAN_ROW_LIMIT) {
        writeln!(
            output,
            "- id={} kind={} status={} prefix={} label={} created={}",
            key.id, key.kind, key.status, key.token_prefix, key.label, key.created_at
        )?;
    }
    if keys.len() > HUMAN_ROW_LIMIT {
        writeln!(
            output,
            "{} additional keys omitted.",
            keys.len() - HUMAN_ROW_LIMIT
        )?;
    }
    writeln!(
        output,
        "Next: run logbrew projects keys revoke {project_id} <key_id> --yes to revoke one"
    )?;
    Ok(())
}

/// Renames one project the account owns.
pub(crate) async fn execute_rename<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    name: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let path = format!("/api/projects/{project_id}");
    let request = serde_json::json!({ "name": name });
    let (status, body, credential) = send(env, path.as_str(), Request::Patch(&request)).await?;
    if status != 200 {
        return Err(api_error(status, body.as_str(), "rename", &credential));
    }
    let renamed = serde_json::from_str::<Renamed>(body.as_str()).map_err(|_| invalid())?;
    if renamed.id != project_id || !crate::http::nonempty_display_safe(&renamed.name, 480) {
        return Err(invalid());
    }
    if json {
        let receipt = serde_json::json!({
            "ok": true,
            "project_id": renamed.id,
            "name": renamed.name,
            "next": "run logbrew projects --json",
        });
        writeln!(output, "{receipt}")?;
    } else {
        writeln!(output, "Project renamed: {}", renamed.id)?;
        writeln!(output, "Name: {}", renamed.name)?;
        writeln!(output, "Next: run logbrew projects")?;
    }
    Ok(())
}

/// Lists the newest 100 archived projects the account can restore.
pub(crate) async fn execute_archived_list<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let (status, body, credential) =
        send(env, "/api/projects/archived?limit=100", Request::Get).await?;
    if status != 200 {
        return Err(api_error(status, body.as_str(), "archived", &credential));
    }
    let page = serde_json::from_str::<ArchivedPage>(body.as_str()).map_err(|_| invalid())?;
    if page.projects.len() > HUMAN_ROW_LIMIT || !page.projects.iter().all(ArchivedProject::valid) {
        return Err(invalid());
    }
    if json {
        writeln!(output, "{body}")?;
        return Ok(());
    }
    writeln!(output, "Archived projects ({})", page.projects.len())?;
    for project in &page.projects {
        writeln!(
            output,
            "- id={} name={} archived={}",
            project.id, project.name, project.archived_at
        )?;
    }
    if page.next_cursor.is_some() {
        writeln!(
            output,
            "Older archived projects exist; this list shows the newest 100."
        )?;
    }
    writeln!(
        output,
        "Next: run logbrew projects restore <project_id> to restore one"
    )?;
    Ok(())
}

/// Restores one archived project the account owns.
pub(crate) async fn execute_restore<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let path = format!("/api/projects/{project_id}/restore");
    let (status, body, credential) = send(env, path.as_str(), Request::Post).await?;
    if status != 204 {
        return Err(api_error(status, body.as_str(), "restore", &credential));
    }
    if !body.is_empty() {
        return Err(invalid());
    }
    if json {
        let receipt = serde_json::json!({
            "ok": true,
            "project_id": project_id,
            "status": "active",
            "next": "run logbrew projects --json",
        });
        writeln!(output, "{receipt}")?;
    } else {
        writeln!(output, "Project restored: {project_id}")?;
        writeln!(output, "Next: run logbrew projects")?;
    }
    Ok(())
}

/// Sends `request` to `path` with account authentication.
async fn send(
    env: &CliEnvironment,
    path: &str,
    request: Request<'_>,
) -> Result<(u16, String, AuthCredential), RuntimeError> {
    let origin = crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport)?;
    let client = crate::http::api_client().map_err(|_| transport())?;
    let url = format!("{origin}{path}");
    let (response, credential) =
        send_account_authenticated_with_refresh(&client, env, |client, credential| {
            match request {
                Request::Get => client.get(url.as_str()),
                Request::Patch(body) => client.patch(url.as_str()).json(body),
                Request::Post => client.post(url.as_str()),
            }
            .bearer_auth(credential.token())
        })
        .await
        .map_err(|error| error.auth_or(transport()))?;
    let status = response.status().as_u16();
    let text = crate::http::bounded_body(response, RESPONSE_LIMIT)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => invalid(),
            crate::http::BodyError::Transport => transport(),
        })?;
    Ok((status, text, credential))
}

/// Validates a standard error envelope, then replaces all server text.
fn api_error(status: u16, body: &str, action: &str, credential: &AuthCredential) -> RuntimeError {
    let Some(shape) = crate::http::ErrorShape::parse(body, status, 512, |text, limit| {
        text.len() <= limit && crate::http::nonempty_display_safe(text, usize::MAX)
    }) else {
        return invalid();
    };
    let not_found = status == 404 && shape.action() == ("not_found", "check_resource", "resource");
    let refused =
        status == 422 && shape.action() == ("validation_failed", "fix_request", "request");
    if !not_found && !refused && !shape.matches_read_status(status) {
        return invalid();
    }
    let body = shape.rate_limit_body("projects").unwrap_or_else(|| {
        let (code, next) = match status {
            401 => ("unauthorized", "run logbrew login"),
            404 if action == "restore" => (
                "not_found",
                "run logbrew projects archived and use a project that this account archived",
            ),
            404 => (
                "not_found",
                "run logbrew projects and use an active project that this account owns",
            ),
            422 if action == "restore" => (
                "deletion_requested",
                "this project is scheduled for permanent deletion and cannot be restored",
            ),
            422 => ("invalid_name", "use a project name of 1 to 120 characters"),
            405 if action == "rename" => (
                "method_not_allowed",
                "this server cannot rename projects yet; retry later",
            ),
            405 => ("method_not_allowed", "update logbrew and retry"),
            _ => ("server_error", "retry the same command later"),
        };
        serde_json::json!({"error": "project request failed", "code": code, "next": next})
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
        message: "project response was invalid",
        next: "retry the same command; if it repeats, report the public response contract",
    }
}

/// Fixed recovery for a failed request, without a URL.
const fn transport() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project request could not be completed",
        next: "check network connectivity and retry the same command",
    }
}
