//! Validation for in-app frame source links and suspect commits.

use serde_json::{Map, Value};

use super::{safe_revision, safe_source_path};
use crate::RuntimeError;
use crate::explain::contract::{
    optional_string, require_bool, require_exact_fields, require_safe_u64, require_string,
    require_timestamp, required_object, validate_object_array,
};
use crate::explain::correlation::validate_availability;
use crate::explain::evidence::{
    validate_evidence, validate_evidence_vocabulary, validate_field_receipts,
};
use crate::explain::invalid_response;

/// Maximum in-app frame links in one source locator.
const FRAME_LINK_LIMIT: usize = 16;
/// Maximum ranked suspect commits.
const SUSPECT_COMMIT_LIMIT: usize = 10;
/// Maximum matched files on one suspect commit.
const SUSPECT_FILE_LIMIT: usize = 5;
/// Every receipt a suspect-commit read emits.
const SUSPECT_EVIDENCE_FIELDS: [&str; 11] = [
    "suspect_commits.range",
    "suspect_commits.repository",
    "suspect_commits.frames",
    "suspect_commits.deployment",
    "suspect_commits.deployment.lookup",
    "suspect_commits.deployment.commit_sha",
    "suspect_commits.previous_deployment",
    "suspect_commits.previous_deployment.lookup",
    "suspect_commits.previous_deployment.commit_sha",
    "suspect_commits.authorization",
    "suspect_commits.provider",
];

/// Validates frame links: each names an in-app frame by index, with its file and line.
pub(super) fn validate_frame_links(
    locator: &Map<String, Value>,
    event: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    let links = validate_object_array(locator, "frame_links", FRAME_LINK_LIMIT)?;
    let frames = validate_object_array(event, "stack_frames", 256)?;
    if !links.is_empty()
        && optional_string(locator, "revision_source")? != Some("deployment_commit")
    {
        return Err(invalid_response());
    }
    let mut previous = None;
    for link in links.iter().filter_map(Value::as_object) {
        require_exact_fields(
            link,
            &["frame_index", "repository_relative_file", "line", "url"],
        )?;
        let index = require_safe_u64(link, "frame_index")?;
        let frame = usize::try_from(index)
            .ok()
            .and_then(|index| frames.get(index))
            .and_then(Value::as_object)
            .ok_or_else(invalid_response)?;
        let file = require_string(link, "repository_relative_file")?;
        if previous
            .replace(index)
            .is_some_and(|previous| previous >= index)
            || frame.get("in_app") != Some(&Value::Bool(true))
            || frame.get("file").and_then(Value::as_str) != Some(file)
            || frame.get("line") != link.get("line")
            || !safe_source_path(file, false)
            || !require_string(link, "url")?.starts_with("https://")
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}

/// Validates the suspect-commit read and its top-level receipts.
pub(super) fn validate_suspect_commits(
    suspects: &Map<String, Value>,
    event: &Map<String, Value>,
    overall: &Map<String, Value>,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        suspects,
        &[
            "status",
            "deployment_id",
            "base_revision",
            "head_revision",
            "commits",
            "truncated",
            "evidence",
        ],
    )?;
    let status = validate_availability(suspects, "status")?;
    let _deployment = optional_string(suspects, "deployment_id")?;
    for name in ["base_revision", "head_revision"] {
        if optional_string(suspects, name)?.is_some_and(|value| !safe_revision(value)) {
            return Err(invalid_response());
        }
    }
    let truncated = require_bool(suspects, "truncated")?;
    let commits = validate_object_array(suspects, "commits", SUSPECT_COMMIT_LIMIT)?;
    let evidence = required_object(suspects, "evidence")?;
    validate_evidence(evidence)?;
    validate_evidence_vocabulary(evidence, "suspect_commits", &SUSPECT_EVIDENCE_FIELDS)?;
    let partial = require_string(evidence, "status")? == "partial";
    if status != "available" && (truncated || !commits.is_empty() || !partial)
        || status == "available" && partial != truncated
    {
        return Err(invalid_response());
    }
    let frame_count = validate_object_array(event, "stack_frames", 256)?.len();
    for (position, commit) in commits.iter().filter_map(Value::as_object).enumerate() {
        validate_commit(commit, position, frame_count)?;
    }
    validate_evidence_vocabulary(
        overall,
        "suspect_commits",
        &["suspect_commits", "suspect_commits.complete"],
    )?;
    validate_field_receipts(overall, "suspect_commits", [true, false, false, false])?;
    validate_field_receipts(
        overall,
        "suspect_commits.complete",
        [false, partial, false, false],
    )
}

/// Validates one ranked commit and its matched files.
fn validate_commit(
    commit: &Map<String, Value>,
    position: usize,
    frame_count: usize,
) -> Result<(), RuntimeError> {
    require_exact_fields(
        commit,
        &[
            "rank",
            "sha",
            "url",
            "title",
            "author_name",
            "committed_at",
            "files",
        ],
    )?;
    let files = validate_object_array(commit, "files", SUSPECT_FILE_LIMIT)?;
    if usize::try_from(require_safe_u64(commit, "rank")?).ok() != Some(position + 1)
        || !safe_revision(require_string(commit, "sha")?)
        || !require_string(commit, "url")?.starts_with("https://")
        || files.is_empty()
    {
        return Err(invalid_response());
    }
    let _title = require_string(commit, "title")?;
    let _author = optional_string(commit, "author_name")?;
    if !commit.get("committed_at").is_some_and(Value::is_null) {
        let _committed_at = require_timestamp(commit, "committed_at")?;
    }
    for file in files.iter().filter_map(Value::as_object) {
        require_exact_fields(file, &["repository_relative_file", "frame_index"])?;
        let index = require_safe_u64(file, "frame_index")?;
        if !safe_source_path(require_string(file, "repository_relative_file")?, false)
            || usize::try_from(index).map_or(true, |index| index >= frame_count)
        {
            return Err(invalid_response());
        }
    }
    Ok(())
}
