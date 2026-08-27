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
    match crate::explain::execute(env, &target, json, output).await {
        Err(RuntimeError::ExplainResponseInvalid) => {
            Err(RuntimeError::InvestigationResponseInvalid)
        }
        error => error,
    }
}
