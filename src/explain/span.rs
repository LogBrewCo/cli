//! Exact-span investigation validation and bounded human rendering.

mod render;
mod validate;

use serde_json::Value;

use crate::{ExplainSpanTarget, RuntimeError};

/// Validates one exact-span response for the parent explanation executor.
pub(super) fn validate_response(
    value: &Value,
    target: &ExplainSpanTarget,
) -> Result<(), RuntimeError> {
    validate::validate_response(value, target)
}

/// Renders one already-validated exact-span response.
pub(super) fn render(value: &Value) -> Option<String> {
    render::render(value)
}
