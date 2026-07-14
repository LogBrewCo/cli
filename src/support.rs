//! Privacy-bounded support-ticket request helpers.

use crate::{SupportTarget, SupportTicketCreateOptions};

/// Builds the public support-ticket endpoint for one operation.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) fn path(target: &SupportTarget) -> String {
    match target {
        SupportTarget::Create(_) => String::from("/api/support/tickets"),
        SupportTarget::List(options) => super::path_with_query(
            "/api/support/tickets",
            &[
                ("project_id", options.project_id.as_deref()),
                ("status", options.status.as_deref()),
                ("source", options.source.as_deref()),
                ("category", options.category.as_deref()),
                ("release", options.release.as_deref()),
                ("limit", options.limit.as_deref()),
                ("pagination", options.pagination.as_deref()),
                ("cursor_time", options.cursor_time.as_deref()),
                ("cursor_id", options.cursor_id.as_deref()),
            ],
        ),
        SupportTarget::Detail(ticket_id) => format!(
            "/api/support/tickets/{}",
            super::encode_component(ticket_id)
        ),
    }
}

/// Builds a support-ticket create body with fixed CLI source metadata.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) fn create_body(options: &SupportTicketCreateOptions) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    insert_string(&mut body, "source", "cli");
    insert_string(&mut body, "category", options.category.as_str());
    insert_string(&mut body, "title", options.title.as_str());
    insert_string(&mut body, "description", options.description.as_str());
    insert_optional(&mut body, "project_id", options.project_id.as_deref());
    insert_optional(&mut body, "environment", options.environment.as_deref());
    insert_optional(&mut body, "runtime", options.runtime.as_deref());
    insert_optional(&mut body, "framework", options.framework.as_deref());
    insert_optional(&mut body, "sdk_package", options.sdk_package.as_deref());
    insert_optional(&mut body, "sdk_version", options.sdk_version.as_deref());
    insert_optional(&mut body, "release", options.release.as_deref());
    insert_optional(&mut body, "trace_id", options.trace_id.as_deref());
    insert_optional(&mut body, "event_id", options.event_id.as_deref());
    if options.diagnostics {
        drop(body.insert(String::from("diagnostics"), generated_diagnostics()));
    }
    serde_json::Value::Object(body)
}

/// Returns a bounded diagnostics object without reading environment variables or files.
fn generated_diagnostics() -> serde_json::Value {
    sanitize_diagnostics(serde_json::json!({
        "arch": std::env::consts::ARCH,
        "binary": "logbrew",
        "cli_version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
    }))
}

/// Builds local support error guidance without retaining backend response text.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(super) fn safe_error_body(status: u16) -> String {
    let (error, code, next, action_code, action_target) = match status {
        400 | 422 => (
            "invalid support request",
            "validation_failed",
            "check support command flags and retry",
            "fix_request",
            "request",
        ),
        401 | 403 => (
            "support authentication failed",
            "unauthorized",
            "run logbrew login and retry",
            "login",
            "auth",
        ),
        404 => (
            "support ticket not found",
            "not_found",
            "check the support ticket id and retry",
            "fix_request",
            "support_ticket",
        ),
        429 => (
            "support request rate limited",
            "rate_limited",
            "wait, then retry the support command",
            "retry_later",
            "support",
        ),
        _ => (
            "support request failed",
            "support_request_failed",
            "retry the support command",
            "retry_support_request",
            "support",
        ),
    };
    serde_json::json!({
        "error": error,
        "code": code,
        "next": next,
        "next_action": {"code": action_code, "target": action_target}
    })
    .to_string()
}

/// Recursively removes sensitive keys and redacts sensitive-looking string values.
fn sanitize_diagnostics(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let safe = object
                .into_iter()
                .filter(|(key, _)| !is_sensitive_key(key))
                .map(|(key, value)| (key, sanitize_diagnostics(value)))
                .collect();
            serde_json::Value::Object(safe)
        }
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .take(32)
                .map(sanitize_diagnostics)
                .collect(),
        ),
        serde_json::Value::String(value) if looks_sensitive_value(value.as_str()) => {
            serde_json::Value::String(String::from("[redacted]"))
        }
        serde_json::Value::Null => serde_json::Value::Null,
        serde_json::Value::Bool(value) => serde_json::Value::Bool(value),
        serde_json::Value::Number(value) => serde_json::Value::Number(value),
        serde_json::Value::String(value) => serde_json::Value::String(value),
    }
}

/// Returns whether a diagnostics key could name authentication material.
fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', ' '], "_");
    [
        "token",
        "secret",
        "password",
        "authorization",
        "cookie",
        "private_key",
        "api_key",
        "credential",
        "session",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

/// Returns whether a string resembles an inline credential assignment or bearer value.
fn looks_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("bearer ")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || [
            "token",
            "secret",
            "password",
            "authorization",
            "cookie",
            "private_key",
            "api_key",
            "credential",
            "session",
        ]
        .iter()
        .any(|marker| {
            lower.contains(format!("{marker}=").as_str())
                || lower.contains(format!("{marker}:").as_str())
        })
}

/// Inserts one required string field.
fn insert_string(body: &mut serde_json::Map<String, serde_json::Value>, key: &str, value: &str) {
    drop(body.insert(key.to_owned(), serde_json::Value::String(value.to_owned())));
}

/// Inserts one optional string field when present.
fn insert_optional(
    body: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        insert_string(body, key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::{safe_error_body, sanitize_diagnostics};

    #[test]
    fn diagnostics_remove_sensitive_keys_and_redact_sensitive_values() {
        let auth_key = ["author", "ization"].concat();
        let auth_value = ["Bear", "er private-auth-value"].concat();
        let cookie_key = ["coo", "kie_value"].concat();
        let cookie_value = "private-cookie-value";
        let assignment_value = ["to", "ken=private-token-value"].concat();
        let mut nested = serde_json::Map::new();
        drop(nested.insert(cookie_key.clone(), cookie_value.into()));
        drop(nested.insert(String::from("note"), assignment_value.into()));
        let mut diagnostics = serde_json::Map::new();
        drop(diagnostics.insert(String::from("runtime"), "rust".into()));
        drop(diagnostics.insert(auth_key.clone(), auth_value.clone().into()));
        drop(diagnostics.insert(String::from("nested"), nested.into()));
        let safe = sanitize_diagnostics(diagnostics.into());
        let text = safe.to_string();
        assert_eq!(safe["runtime"], "rust");
        assert_eq!(safe["nested"]["note"], "[redacted]");
        for hidden in [
            auth_value.as_str(),
            cookie_value,
            "private-token-value",
            auth_key.as_str(),
            cookie_key.as_str(),
        ] {
            assert!(!text.contains(hidden));
        }
    }

    #[test]
    fn support_error_body_uses_local_guidance_instead_of_backend_text() {
        let safe = safe_error_body(422);
        let value: serde_json::Value = serde_json::from_str(&safe).expect("safe JSON");
        assert_eq!(value["code"], "validation_failed");
        assert_eq!(value["error"], "invalid support request");
        assert_eq!(value["next"], "check support command flags and retry");
        assert_eq!(value["next_action"]["code"], "fix_request");
    }
}
