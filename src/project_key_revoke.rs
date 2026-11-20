//! Explicitly confirmed project key revocation.

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::ids::is_uuid;
use crate::{CliEnvironment, CliError, RuntimeError};

/// Maximum accepted revocation response body.
const RESPONSE_LIMIT: usize = 64 * 1024;

/// Key metadata exactly as the list and revocation routes return it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyWire {
    /// Key UUID.
    id: String,
    /// Key label.
    label: String,
    /// Key kind.
    kind: String,
    /// Short non-secret key prefix.
    token_prefix: String,
    /// Key state: `active`, `expired`, or `revoked`.
    status: String,
    /// Creation time.
    created_at: String,
    /// Optional expiry.
    expires_at: Option<String>,
    /// Revocation time, present exactly when the key is revoked.
    revoked_at: Option<String>,
}

/// Lifecycle state of one project key.
#[derive(Clone, Copy)]
pub(crate) enum KeyStatus {
    /// The key authenticates ingest requests.
    Active,
    /// The key passed its expiry time.
    Expired,
    /// The key was revoked.
    Revoked,
}

impl KeyStatus {
    /// Stable wire value.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }
}

/// Validated key metadata from the list and revocation routes; the raw key is never returned.
#[derive(serde::Deserialize)]
#[serde(try_from = "KeyWire")]
pub(crate) struct KeyMetadata {
    /// Key UUID.
    pub(crate) id: String,
    /// Key label.
    pub(crate) label: String,
    /// Key kind.
    pub(crate) kind: String,
    /// Short non-secret key prefix.
    pub(crate) token_prefix: String,
    /// Key state.
    pub(crate) status: KeyStatus,
    /// Creation time.
    pub(crate) created_at: String,
}

impl TryFrom<KeyWire> for KeyMetadata {
    type Error = &'static str;

    fn try_from(wire: KeyWire) -> Result<Self, Self::Error> {
        let status = match (wire.status.as_str(), wire.revoked_at.is_some()) {
            ("active", false) => KeyStatus::Active,
            ("expired", false) => KeyStatus::Expired,
            ("revoked", true) => KeyStatus::Revoked,
            _ => return Err("invalid key status"),
        };
        let safe = |text: &str| crate::http::nonempty_display_safe(text, 128);
        let valid = is_uuid(wire.id.as_str())
            && std::iter::once(Some(wire.created_at.as_str()))
                .chain([wire.expires_at.as_deref(), wire.revoked_at.as_deref()])
                .flatten()
                .all(crate::time::is_rfc3339_utc)
            && [&wire.label, &wire.kind, &wire.token_prefix]
                .into_iter()
                .all(|text| safe(text));
        if !valid {
            return Err("invalid key metadata");
        }
        Ok(Self {
            id: wire.id,
            label: wire.label,
            kind: wire.kind,
            token_prefix: wire.token_prefix,
            status,
            created_at: wire.created_at,
        })
    }
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
    let client = crate::http::api_client()?;
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
    if revoked.id != key_id || !matches!(revoked.status, KeyStatus::Revoked) {
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

#[cfg(test)]
mod tests {
    use super::KeyMetadata;

    const REVOKED_AT: &str = "2026-11-20T10:00:00Z";

    fn key(status: &str, revoked_at: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": "123e4567-e89b-12d3-a456-426614174000",
            "label": "server",
            "kind": "ingest",
            "token_prefix": "lbw_ingest_ab",
            "status": status,
            "created_at": "2026-11-01T10:00:00Z",
            "expires_at": null,
            "revoked_at": revoked_at,
        })
    }

    #[test]
    fn key_status_matches_its_revocation_time() {
        for (status, revoked_at, expected) in [
            ("active", None, Some("active")),
            ("expired", None, Some("expired")),
            ("revoked", Some(REVOKED_AT), Some("revoked")),
            ("active", Some(REVOKED_AT), None),
            ("expired", Some(REVOKED_AT), None),
            ("revoked", None, None),
            ("disabled", None, None),
        ] {
            let parsed = serde_json::from_value::<KeyMetadata>(key(status, revoked_at)).ok();
            assert_eq!(
                parsed.map(|key| key.status.as_str()),
                expected,
                "{status} revoked_at={revoked_at:?}"
            );
        }
    }

    #[test]
    fn key_metadata_rejects_unsafe_or_unknown_fields() {
        for (field, value) in [
            ("id", serde_json::json!("not-a-uuid")),
            ("label", serde_json::json!("")),
            ("token_prefix", serde_json::json!("lbw\nforged")),
            ("created_at", serde_json::json!("yesterday")),
            ("expires_at", serde_json::json!("2026-11-01")),
            ("owner", serde_json::json!("someone")),
        ] {
            let mut body = key("active", None);
            body[field] = value;
            assert!(
                serde_json::from_value::<KeyMetadata>(body).is_err(),
                "{field}"
            );
        }
    }
}
