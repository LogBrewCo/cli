//! Original native debug-artifact contract fixtures.

use std::process::Output;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub(crate) const ARM64E_UUID: &str = "30313233-3435-3637-3839-3a3b3c3d3e3f";

pub(crate) fn lookup_args<'a>(image_uuid: &'a str, architecture: &'a str) -> Vec<&'a str> {
    vec![
        "debug-artifacts",
        "lookup",
        "--project",
        PROJECT_ID,
        "--release",
        "checkout@1.2.3",
        "--environment",
        "production",
        "--service",
        "checkout-api",
        "--image-uuid",
        image_uuid,
        "--architecture",
        architecture,
        "--json",
    ]
}

pub(crate) async fn malformed_success_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(missing_lookup()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "unexpected": "hostile backend text"
        })))
        .expect(1)
        .mount(&server)
        .await;
    server
}

pub(crate) async fn mount_upload_success(server: &MockServer, artifact_count: usize) {
    Mock::given(method("POST"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(upload_success_body(artifact_count)))
        .expect(1)
        .mount(server)
        .await;
}

pub(crate) async fn mount_lookup_sequence(server: &MockServer, bodies: Vec<serde_json::Value>) {
    let bodies = Arc::new(bodies);
    let attempt = Arc::new(AtomicUsize::new(0));
    let bodies_for_response = Arc::clone(&bodies);
    let attempt_for_response = Arc::clone(&attempt);
    let expected = u64::try_from(bodies.len()).unwrap_or(u64::MAX);
    Mock::given(method("GET"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(move |_request: &Request| {
            let index = attempt_for_response.fetch_add(1, Ordering::SeqCst);
            let body = bodies_for_response
                .get(index)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"unexpected": "sequence exhausted"}));
            ResponseTemplate::new(200).set_body_json(body)
        })
        .expect(expected)
        .mount(server)
        .await;
}

pub(crate) fn manifest(artifacts: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "projectId": PROJECT_ID,
        "release": "checkout@1.2.3",
        "environment": "production",
        "service": "checkout-api",
        "artifactType": "apple_dsym_manifest",
        "validation": {"status": "ready"},
        "artifacts": artifacts
    })
}

pub(crate) fn universal_macho(
    slices: &[(u32, u32, &[u8])],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let header_size = 8usize.saturating_add(slices.len().saturating_mul(20));
    let mut offsets = Vec::with_capacity(slices.len());
    let mut next_offset = header_size;
    for (_, _, bytes) in slices {
        offsets.push(next_offset);
        next_offset = next_offset.saturating_add(bytes.len());
    }
    let mut universal = Vec::with_capacity(next_offset);
    universal.extend_from_slice(0xcafe_babeu32.to_be_bytes().as_slice());
    universal.extend_from_slice(u32::try_from(slices.len())?.to_be_bytes().as_slice());
    for ((cpu_type, cpu_subtype, bytes), offset) in slices.iter().zip(offsets.iter()) {
        for value in [
            *cpu_type,
            *cpu_subtype,
            u32::try_from(*offset)?,
            u32::try_from(bytes.len())?,
            0,
        ] {
            universal.extend_from_slice(value.to_be_bytes().as_slice());
        }
    }
    for (_, _, bytes) in slices {
        universal.extend_from_slice(bytes);
    }
    Ok(universal)
}

pub(crate) fn assert_invalid_response_is_redacted(
    output: &Output,
    fixture: &Fixture,
    server: &MockServer,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr.clone())?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["error"], "native_debug_response_invalid");
    assert_eq!(
        body["next"],
        "retry the native debug-artifact request; if it repeats, report the public response contract"
    );
    assert!(!text.contains("hostile backend text"));
    assert_private_values_absent(text.as_str(), fixture, server.uri().as_str());
    Ok(())
}

pub(crate) fn assert_request_has_no_local_identity(request: &Request, fixture: &Fixture) {
    let body = String::from_utf8_lossy(request.body.as_slice());
    let root = fixture.root.to_string_lossy();
    for private in [
        "Customer Secret",
        "private-arm",
        "private-x86",
        root.as_ref(),
    ] {
        assert!(!body.contains(private));
    }
}

pub(crate) fn upload_request(requests: &[Request]) -> Result<&Request, Box<dyn std::error::Error>> {
    requests
        .iter()
        .find(|request| request.method.as_str() == "POST")
        .ok_or_else(|| "missing upload request".into())
}
