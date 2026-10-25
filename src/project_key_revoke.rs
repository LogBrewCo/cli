//! Explicitly confirmed project key revocation.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes these private-module helpers"
)]

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::ids::is_uuid;
use crate::{CliEnvironment, CliError, RuntimeError};

/// Maximum accepted revocation response body.
const RESPONSE_LIMIT: usize = 64 * 1024;

/// Revoked key metadata; the raw key is never returned.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyMetadata {
    /// Key UUID.
    id: String,
    /// Key label, validated and discarded.
    label: String,
    /// Key kind.
    kind: String,
    /// Short non-secret key prefix, validated and discarded.
    token_prefix: String,
    /// Key state after the request.
    status: String,
    /// Creation time, validated and discarded.
    created_at: String,
    /// Optional expiry, validated and discarded.
    expires_at: Option<String>,
    /// Revocation time.
    revoked_at: Option<String>,
}

/// Revokes one project key: it stops working at its next request.
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    project_id: &str,
    key_id: &str,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !is_uuid(project_id) || !is_uuid(key_id) {
        return Err(CliError::InvalidProjectsCommand.into());
    }
    let (project_id, key_id) = (project_id.to_ascii_lowercase(), key_id.to_ascii_lowercase());
    let origin = crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport)?;
    let client = crate::http::api_client().map_err(|_| transport())?;
    let url = format!("{origin}/api/projects/{project_id}/ingest-keys/{key_id}");
    let (response, credential) =
        send_account_authenticated_with_refresh(&client, env, |client, credential| {
            client.delete(url.as_str()).bearer_auth(credential.token())
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
    if status != 200 {
        return Err(api_error(status, body.as_str(), &credential));
    }
    let revoked = serde_json::from_str::<KeyMetadata>(body.as_str()).map_err(|_| invalid())?;
    let safe = |text: &str| crate::http::nonempty_display_safe(text, 128);
    let timestamps = std::iter::once(Some(revoked.created_at.as_str()))
        .chain([revoked.expires_at.as_deref(), revoked.revoked_at.as_deref()])
        .flatten()
        .all(crate::time::is_rfc3339_utc);
    if revoked.id != key_id
        || revoked.status != "revoked"
        || revoked.revoked_at.is_none()
        || !timestamps
        || ![
            revoked.label.as_str(),
            revoked.kind.as_str(),
            revoked.token_prefix.as_str(),
        ]
        .into_iter()
        .all(safe)
    {
        return Err(invalid());
    }
    if json {
        let receipt = serde_json::json!({
            "ok": true,
            "project_id": project_id,
            "key_id": key_id,
            "kind": revoked.kind,
            "status": "revoked",
            "next": "create a replacement with logbrew projects keys create if it is still needed",
        });
        writeln!(output, "{receipt}")?;
    } else {
        writeln!(output, "Project key revoked: {key_id}")?;
        writeln!(output, "Project: {project_id}")?;
        writeln!(output, "Kind: {}", revoked.kind)?;
        writeln!(
            output,
            "Next: create a replacement with logbrew projects keys create if it is still needed"
        )?;
    }
    Ok(())
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
    let body = shape.rate_limit_body("project key").unwrap_or_else(|| {
        let (code, next) = match status {
            401 => ("unauthorized", "run logbrew login"),
            403 => ("forbidden", "use an account that owns the project"),
            404 => (
                "not_found",
                "check the project id and the key id printed when the key was created",
            ),
            405 => ("method_not_allowed", "update logbrew and retry"),
            _ => ("server_error", "retry the same command later"),
        };
        serde_json::json!({"error": "project key revocation failed", "code": code, "next": next})
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
        message: "project key revocation response was invalid",
        next: "retry the same command; if it repeats, report the public response contract",
    }
}

/// Fixed recovery for a failed request, without a URL.
const fn transport() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "project key revocation could not be completed",
        next: "check network connectivity and retry the same command",
    }
}
