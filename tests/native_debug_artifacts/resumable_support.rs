//! Resumable native debug-artifact integration fixtures.

use super::support::{Fixture, macho64, uuid_bytes};

pub(crate) const RESUMABLE_CHUNK_SIZE: usize = 4 * 1024 * 1024;
pub(crate) const RESUMABLE_SESSION_ID: &str =
    "nativeupload_1111111111111111111111111111111111111111111111111111111111111111";
pub(crate) const START_METHOD_NEXT: &str = "use the supported native debug-artifact request method";

pub(crate) fn start_response(missing_chunks: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "session_id": RESUMABLE_SESSION_ID,
        "status": "ready",
        "chunk_size": RESUMABLE_CHUNK_SIZE,
        "missing_chunks": missing_chunks,
        "next": "Continue the native debug artifact upload.",
        "next_action": {
            "code": if missing_chunks.is_empty() {
                "complete_native_debug_artifact_upload"
            } else {
                "upload_native_debug_artifact_chunks"
            },
            "target": "native_debug_artifact_upload"
        }
    })
}

pub(crate) fn chunk_response(digest: &str) -> serde_json::Value {
    serde_json::json!({
        "session_id": RESUMABLE_SESSION_ID,
        "chunk_sha256": digest,
        "status": "uploaded",
        "next": "Native debug artifact chunk accepted. Continue missing chunks or complete the session.",
        "next_action": {
            "code": "upload_native_debug_artifact_chunks",
            "target": "native_debug_artifact_upload"
        }
    })
}

pub(crate) fn artifact(
    label: &str,
    size: usize,
) -> Result<(Fixture, std::path::PathBuf, Vec<u8>), Box<dyn std::error::Error>> {
    let fixture = Fixture::new(label)?;
    let mut object = macho64(0x0100_000c, uuid_bytes(0x10));
    object.resize(size.max(object.len()), 0x5a);
    let path = fixture.root.join("Customer Secret Resumable Symbols");
    std::fs::write(path.as_path(), object.as_slice())?;
    Ok((fixture, path, object))
}

pub(crate) fn error_envelope(
    code: &str,
    next: &str,
    action_code: &str,
    target: &str,
) -> serde_json::Value {
    serde_json::json!({
        "error": "request failed",
        "code": code,
        "next": next,
        "next_action": {
            "code": action_code,
            "target": target
        }
    })
}
