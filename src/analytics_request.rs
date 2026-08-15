//! Shared exact request-body helpers for product analytics.

/// Adds one optional exact context filter without sending a null placeholder.
#[expect(
    clippy::redundant_pub_crate,
    reason = "private sibling analytics modules share this helper"
)]
pub(crate) fn insert_optional(
    body: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        drop(body.insert(key.to_owned(), serde_json::Value::String(value.to_owned())));
    }
}
