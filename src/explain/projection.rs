//! Shared validation of backend-scrubbed arbitrary telemetry projections.

use serde_json::Value;

use super::invalid_response;
use crate::RuntimeError;

/// Maximum nested object or array depth retained by the backend projection.
const PROJECTION_DEPTH_LIMIT: usize = 4;
/// Maximum elements retained from one projected array.
const PROJECTION_ARRAY_LIMIT: usize = 16;
/// Maximum characters retained in one projected key.
const PROJECTION_KEY_LIMIT: usize = 64;
/// Maximum characters retained in one projected string.
const PROJECTION_STRING_LIMIT: usize = 512;

/// Validates an already-scrubbed arbitrary telemetry projection.
pub(super) fn validate_projection(value: &Value) -> Result<(), RuntimeError> {
    validate_projection_value(value, "", 0)
}

/// Recursively validates one projected value while retaining parent-key semantics.
fn validate_projection_value(
    value: &Value,
    parent_key: &str,
    depth: usize,
) -> Result<(), RuntimeError> {
    match value {
        Value::Object(object) => {
            if depth >= PROJECTION_DEPTH_LIMIT {
                return Err(invalid_response());
            }
            for (key, child) in object {
                if key.is_empty()
                    || key.chars().count() > PROJECTION_KEY_LIMIT
                    || key.chars().any(char::is_control)
                    || sensitive_key(key)
                {
                    return Err(invalid_response());
                }
                validate_projection_value(child, key, depth.saturating_add(1))?;
            }
            Ok(())
        }
        Value::Array(items) => {
            if depth >= PROJECTION_DEPTH_LIMIT || items.len() > PROJECTION_ARRAY_LIMIT {
                return Err(invalid_response());
            }
            for item in items {
                validate_projection_value(item, parent_key, depth.saturating_add(1))?;
            }
            Ok(())
        }
        Value::String(text)
            if text.chars().count() > PROJECTION_STRING_LIMIT
                || sensitive_string(text, parent_key) =>
        {
            Err(invalid_response())
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
    }
}

/// Counts retained scalar leaves exactly as the backend projection receipt does.
pub(super) fn count_scalar_leaves(value: &Value) -> u64 {
    match value {
        Value::Object(object) => object.values().map(count_scalar_leaves).sum(),
        Value::Array(items) => items.iter().map(count_scalar_leaves).sum(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 1,
    }
}

/// Mirrors the backend's credential, direct-identity, and raw-request key boundary.
pub(super) fn sensitive_key(key: &str) -> bool {
    let compact = compact_key(key);
    [
        "apikey",
        "authorization",
        "connectionstring",
        "cookie",
        "credential",
        "deviceid",
        "distinctid",
        "email",
        "fullname",
        "hostid",
        "hostname",
        "ipaddress",
        "macaddress",
        "password",
        "passwd",
        "phone",
        "privatekey",
        "requestbody",
        "responsebody",
        "secret",
        "sessionid",
        "subjectid",
        "token",
        "userid",
        "username",
        "urlfull",
    ]
    .iter()
    .any(|term| compact.contains(term))
}

/// Extends direct-key screening with the backend context-tag vocabulary.
pub(super) fn sensitive_context_tag_key(key: &str) -> bool {
    let compact = compact_key(key);
    ["auth", "dsn"].iter().any(|term| compact.contains(term))
}

/// Rejects private, credential-like, or instruction-like strings before JSON echo.
pub(super) fn sensitive_string(value: &str, key: &str) -> bool {
    let compact = value.trim().to_ascii_lowercase();
    let safe_route = key.eq_ignore_ascii_case("route")
        && compact.starts_with('/')
        && !compact.contains('?')
        && !compact.contains('#')
        && !compact.contains("..")
        && !compact.starts_with("//");
    let email = compact.split_once('@').is_some_and(|(mailbox, domain)| {
        !mailbox.is_empty()
            && domain.rsplit_once('.').is_some_and(|(_, suffix)| {
                suffix.len() >= 2 && suffix.bytes().all(|byte| byte.is_ascii_alphabetic())
            })
            && !compact.chars().any(char::is_whitespace)
    });
    let windows_path = compact.as_bytes().windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'/' | b'\\')
    });
    email
        || compact.parse::<std::net::IpAddr>().is_ok()
        || compact.starts_with('/') && !safe_route
        || windows_path
        || compact.contains("://") && compact.contains('?')
        || [
            "ignore prior instructions",
            "ignore previous instructions",
            "developer message",
            "<|im_start|>",
            "authorization:",
            "basic ",
            "bearer ",
            "cookie:",
            "password:",
            "password=",
            "secret:",
            "secret=",
            "token:",
            "token=",
            "akia",
            "ghp_",
            "github_pat_",
            "sk_live_",
            "sk_test_",
            "xox",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
}

/// Compacts a telemetry key for defensive vocabulary matching.
fn compact_key(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}
