//! Compatibility entry point for the versioned issue investigation bundle.

use crate::{CliEnvironment, ExplainTarget, IssueOccurrenceSelection, RuntimeError};

/// Executes the rich, read-only issue investigation used by `explain issue`.
///
/// Keeping `investigate issue` on the same implementation prevents humans and
/// agents from receiving a smaller legacy evidence set through one alias.
pub async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    issue_id: &str,
    occurrence: &IssueOccurrenceSelection,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let target = ExplainTarget::Issue {
        id: issue_id.to_owned(),
        occurrence: occurrence.clone(),
    };
    crate::explain::execute(env, &target, json, output)
        .await
        .map_err(investigation_error)
}

/// Preserves the established investigation-specific invalid-response code.
fn investigation_error(error: RuntimeError) -> RuntimeError {
    if matches!(&error, RuntimeError::ExplainResponseInvalid) {
        RuntimeError::InvestigationResponseInvalid
    } else {
        error
    }
}
