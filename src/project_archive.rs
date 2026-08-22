//! Strict, explicitly confirmed project lifecycle mutations.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes these private-module helpers"
)]

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::http::nonempty_display_safe as safe_text;
use crate::ids::{is_support_ticket_id, is_uuid};
use crate::{CliEnvironment, CliError, RuntimeError};
use serde::{Deserialize, Serialize};

/// Account-owned project mutation selected by the parser.
#[derive(Clone, Copy)]
enum Operation {
    /// Remove a project from active use without hard deletion.
    Archive,
    /// Schedule permanent deletion after exact confirmation.
    Delete,
}

/// Local failure boundary used to select fixed recovery text.
enum Failure {
    /// Account authentication was absent or invalid for mutation.
    Auth,
    /// The server response violated the selected operation contract.
    Response,
    /// Request construction, origin validation, or transport failed.
    Transport,
}

/// Exact successful machine-readable lifecycle result.
#[derive(Serialize)]
struct Success<'a> {
    /// Stable success signal.
    ok: bool,
    /// Canonical affected project UUID.
    project_id: &'a str,
    /// Explicit inactive state returned only for permanent deletion.
    #[serde(skip_serializing_if = "Option::is_none")]
    project_active: Option<bool>,
    /// Stable lifecycle result.
    status: &'static str,
    /// Deterministic active-catalog recovery.
    next: &'static str,
}

/// Strict backend archive error envelope.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorEnvelope {
    /// Human backend error, validated and then discarded.
    error: String,
    /// Stable backend error code.
    code: String,
    /// Human backend recovery, validated and then discarded.
    next: String,
    /// Typed backend recovery metadata.
    next_action: Action,
}

/// Strict typed backend recovery metadata.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    /// Stable action code.
    code: String,
    /// Stable action target.
    target: String,
}

/// Strict accepted deletion receipt; all server text is discarded.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionReceipt {
    /// Public support-ticket identifier proving durable acceptance.
    ticket_id: String,
    /// Required initial ticket status.
    status: String,
    /// Required UTC creation timestamp.
    created_at: String,
    /// Bounded server guidance, validated and discarded.
    next: String,
    /// Required legacy support receipt action.
    next_action: Action,
}

/// Executes one strict account-authenticated project archive.
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    execute_lifecycle(Operation::Archive, env, project_id, json, output).await
}

/// Executes one idempotent account-authenticated permanent deletion.
pub(crate) async fn execute_deletion<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    execute_lifecycle(Operation::Delete, env, project_id, json, output).await
}

/// Executes one validated lifecycle operation through the account auth path.
async fn execute_lifecycle<W: std::io::Write>(
    operation: Operation,
    env: &CliEnvironment,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !is_uuid(project_id) {
        return Err(match operation {
            Operation::Archive => CliError::InvalidProjectArchiveCommand,
            Operation::Delete => CliError::InvalidProjectDeletionCommand,
        }
        .into());
    }
    let project_id = project_id.to_ascii_lowercase();
    let client = crate::http::api_client().map_err(|_| failure(operation, Failure::Transport))?;
    let origin = crate::http::normalized_origin(env.base_url.as_str())
        .ok_or_else(|| failure(operation, Failure::Transport))?;
    let url = match operation {
        Operation::Archive => format!("{origin}/api/projects/{project_id}"),
        Operation::Delete => format!("{origin}/api/support/tickets"),
    };
    let deletion = deletion_body(project_id.as_str());
    let (response, credential) = send_account_authenticated_with_refresh(
        &client,
        env,
        |client, credential| match operation {
            Operation::Archive => client.delete(url.as_str()).bearer_auth(credential.token()),
            Operation::Delete => client
                .post(url.as_str())
                .bearer_auth(credential.token())
                .header("Idempotency-Key", project_id.as_str())
                .json(&deletion),
        },
    )
    .await
    .map_err(|error| request_error(operation, error))?;
    let status = response.status().as_u16();
    let body = crate::http::bounded_body(response, 64 * 1024)
        .await
        .map_err(|_| failure(operation, Failure::Response))?;

    match operation {
        Operation::Archive if status != 204 => {
            return Err(validate_archive_error(status, body.as_str(), &credential)?);
        }
        Operation::Archive if !body.is_empty() => {
            return Err(failure(operation, Failure::Response));
        }
        Operation::Delete if status != 200 => {
            if (200..300).contains(&status) {
                return Err(failure(operation, Failure::Response));
            }
            return Err(api_error(operation, status, &credential));
        }
        Operation::Delete => validate_deletion_receipt(body.as_str())?,
        Operation::Archive => {}
    }
    write_success(operation, project_id.as_str(), json, output)
}

/// Fixed request body shared by execution and public command introspection.
pub(crate) fn deletion_body(project_id: &str) -> serde_json::Value {
    serde_json::json!({
        "source": "cli",
        "category": "project_deletion",
        "project_id": project_id,
        "title": "Permanent project deletion request",
        "description": "Permanent project deletion requested from LogBrew CLI."
    })
}

/// Converts request failures into operation-specific, host-free recovery.
fn request_error(operation: Operation, error: RuntimeError) -> RuntimeError {
    if matches!(
        &error,
        RuntimeError::Unavailable {
            message: "account authentication is required",
            ..
        }
    ) {
        failure(operation, Failure::Auth)
    } else {
        error.auth_or(failure(operation, Failure::Transport))
    }
}

/// Accepts only the strict durable support receipt used by the deletion route.
fn validate_deletion_receipt(body: &str) -> Result<(), RuntimeError> {
    let receipt = serde_json::from_str::<DeletionReceipt>(body)
        .map_err(|_| failure(Operation::Delete, Failure::Response))?;
    if !is_support_ticket_id(receipt.ticket_id.as_str())
        || receipt.status != "open"
        || !crate::render::is_rfc3339_utc(receipt.created_at.as_str())
        || receipt.next.len() > 512
        || !safe_text(receipt.next.as_str(), 512)
        || receipt.next_action.code != "review_ticket"
        || receipt.next_action.target != "support_ticket"
    {
        return Err(failure(Operation::Delete, Failure::Response));
    }
    Ok(())
}

/// Validates one typed archive error before replacing all server text.
fn validate_archive_error(
    status: u16,
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    let envelope = serde_json::from_str::<ErrorEnvelope>(body)
        .map_err(|_| failure(Operation::Archive, Failure::Response))?;
    let safe = [
        (envelope.error.as_str(), 512),
        (envelope.code.as_str(), 64),
        (envelope.next.as_str(), 512),
        (envelope.next_action.code.as_str(), 64),
        (envelope.next_action.target.as_str(), 64),
    ]
    .into_iter()
    .all(|(value, limit)| value.len() <= limit && safe_text(value, limit));
    let fields = (
        envelope.code.as_str(),
        envelope.next_action.code.as_str(),
        envelope.next_action.target.as_str(),
    );
    let typed = match status {
        401 => fields == ("unauthorized", "sign_in", "auth"),
        403 => fields == ("forbidden", "request_access", "auth"),
        404 => fields == ("not_found", "check_resource", "resource"),
        405 => fields == ("method_not_allowed", "use_supported_method", "api_method"),
        500..=599 => matches!(
            fields,
            ("storage_error", "retry_or_check_storage", "backend_status")
                | ("json_error", "send_valid_json", "request")
                | ("internal_error", "retry_or_contact_support", "support")
        ),
        _ => false,
    };
    if !safe || !typed {
        return Err(failure(Operation::Archive, Failure::Response));
    }
    Ok(api_error(Operation::Archive, status, credential))
}

/// Writes deterministic local output without server-controlled prose.
fn write_success<W: std::io::Write>(
    operation: Operation,
    project_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if json {
        let (project_active, status) = match operation {
            Operation::Archive => (None, "archived"),
            Operation::Delete => (Some(false), "deletion_scheduled"),
        };
        serde_json::to_writer(
            &mut *output,
            &Success {
                ok: true,
                project_id,
                project_active,
                status,
                next: "run logbrew projects --json",
            },
        )
        .map_err(|_| std::io::Error::other("project lifecycle output could not be written"))?;
        writeln!(output)?;
    } else {
        match operation {
            Operation::Archive => {
                writeln!(output, "Project archived: {project_id}")?;
                writeln!(output, "Project ingest keys: disabled")?;
            }
            Operation::Delete => {
                writeln!(output, "Project deletion accepted: {project_id}")?;
                writeln!(output, "Project status: inactive")?;
                writeln!(output, "Permanent deletion: scheduled automatically")?;
            }
        }
        writeln!(output, "Next: run logbrew projects")?;
    }
    Ok(())
}

/// Creates one synthetic API error from allowlisted status metadata.
fn api_error(operation: Operation, status: u16, credential: &AuthCredential) -> RuntimeError {
    let error = match operation {
        Operation::Archive => "project archive could not be confirmed",
        Operation::Delete => "project deletion could not be confirmed",
    };
    let (code, next) = match (operation, status) {
        (_, 401) => ("unauthorized", "run logbrew login"),
        (_, 403) => ("forbidden", "use an account that owns the project"),
        (_, 404) => ("not_found", "refresh the active project list"),
        (Operation::Archive, 405) => ("method_not_allowed", "retry with logbrew projects archive"),
        (Operation::Delete, 400 | 422) => {
            ("validation_failed", "confirm the exact active project id")
        }
        (Operation::Delete, 409) => (
            "idempotency_conflict",
            "retry the exact same deletion command",
        ),
        _ => ("server_error", "retry the same command later"),
    };
    RuntimeError::Api {
        status,
        body: serde_json::json!({
            "error": error,
            "code": code,
            "next": next
        })
        .to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

/// Returns fixed local recovery for an operation failure boundary.
const fn failure(operation: Operation, kind: Failure) -> RuntimeError {
    let (message, next) = match (operation, kind) {
        (Operation::Archive, Failure::Auth) => (
            "account authentication is required",
            "run logbrew login and retry the project archive command",
        ),
        (Operation::Delete, Failure::Auth) => (
            "account authentication is required",
            "run logbrew login and retry the project deletion command",
        ),
        (Operation::Archive, Failure::Response) => (
            "project archive response was invalid",
            "refresh the active project list before retrying the archive command",
        ),
        (Operation::Delete, Failure::Response) => (
            "project deletion response was invalid",
            "refresh the active project list before retrying the deletion command",
        ),
        (Operation::Archive, Failure::Transport) => (
            "project archive request could not be completed",
            "check network connectivity and retry the same archive command",
        ),
        (Operation::Delete, Failure::Transport) => (
            "project deletion request could not be completed",
            "check network connectivity and retry the same deletion command",
        ),
    };
    RuntimeError::Unavailable { message, next }
}
