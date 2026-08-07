//! Strict release-investigation v3 validation and bounded comparison rendering.

mod render;
mod validate;

use serde_json::{Map, Value};

use crate::{ExplainReleaseTarget, RuntimeError};

/// Validates the complete schema-version-3 release envelope and comparison receipts.
pub(super) fn validate_response(
    response: &Map<String, Value>,
    expected: &ExplainReleaseTarget,
) -> Result<(), RuntimeError> {
    validate::validate_response(response, expected)
}

/// Appends deployment-aligned release comparison evidence after validation.
pub(super) fn render_comparison(output: &mut String, comparison: Option<&Value>) {
    render::render_comparison(output, comparison);
}
