//! Native debug-artifact transport bounds and progress contracts.

#[allow(dead_code)]
#[path = "native_debug_artifacts/contract_support.rs"]
mod contract_support;
#[allow(dead_code)]
#[path = "native_debug_artifacts/resumable_support.rs"]
mod resumable_support;
#[allow(dead_code)]
#[path = "native_debug_artifacts/support.rs"]
mod support;

use contract_support::universal_macho;
use resumable_support::{
    RESUMABLE_CHUNK_SIZE, RESUMABLE_SESSION_ID, chunk_response, start_response,
};
use std::io::BufRead as _;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;
use support::{
    ARM64_UUID, Fixture, PROJECT_ID, TOKEN, UPLOAD_ID, X86_64_UUID, assert_private_values_absent,
    command_with_token, header_value, invoke, macho64, missing_lookup, mount_lookup_sequence,
    received_requests, sha256_hex, upload_args, upload_success_body, uuid_bytes,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const IMPLEMENTATION: &str = include_str!("../src/native_debug_artifacts.rs");

#[test]
fn native_debug_transport_has_explicit_request_and_overall_bounds_without_hidden_retries() {
    assert!(
        IMPLEMENTATION.contains(
            "const OVERALL_UPLOAD_TIMEOUT: std::time::Duration = \
             std::time::Duration::from_secs(30 * 60);"
        ),
        "native debug upload needs one fixed overall network deadline"
    );
    assert!(
        IMPLEMENTATION.contains(
            "const UPLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);"
        ),
        "compatibility upload must not retain the multi-minute request timeout"
    );
    assert!(
        IMPLEMENTATION.contains("const MAX_PHASE_ATTEMPTS: usize = 2;"),
        "resumable phases may perform at most one explicit retry"
    );
    assert!(
        IMPLEMENTATION.contains(".retry(reqwest::retry::never())"),
        "Reqwest protocol retries must be disabled for replayable upload bodies"
    );
}

#[tokio::test]
async fn human_upload_reports_a_fixed_phase_before_waiting_for_the_server()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_json(missing_lookup()),
        )
        .mount(&server)
        .await;

    let fixture = Fixture::new("bounded-progress")?;
    let artifact = fixture.root.join("Customer Secret Waiting Symbols");
    std::fs::write(artifact.as_path(), macho64(0x0100_000c, uuid_bytes(0x10)))?;
    let mut args = upload_args(artifact.as_os_str());
    assert_eq!(
        args.pop()
            .and_then(|value| value.into_string().ok())
            .as_deref(),
        Some("--json")
    );

    let mut command = command_with_token(&fixture, server.uri().as_str(), args, TOKEN);
    let _command = command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or("stdout was not piped")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .map(|_| line);
        drop(sender.send(result));
    });

    let line = receiver.recv_timeout(Duration::from_secs(2));
    drop(child.kill());
    let _status = child.wait()?;
    reader
        .join()
        .map_err(|_| -> Box<dyn std::error::Error> { "stdout reader panicked".into() })?;

    assert_eq!(
        line??, "Checking native debug artifact availability.\n",
        "progress must be fixed and free of scope, identity, and path values"
    );
    Ok(())
}

#[tokio::test]
async fn large_universal_object_uses_bounded_resumable_chunks_and_one_json_document()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("bounded-large-universal")?;
    let mut arm64 = macho64(0x0100_000c, uuid_bytes(0x10));
    arm64.resize(RESUMABLE_CHUNK_SIZE + 257, 0x5a);
    let mut x86_64 = macho64(0x0100_0007, uuid_bytes(0x20));
    x86_64.resize(RESUMABLE_CHUNK_SIZE + 513, 0x6b);
    let universal = universal_macho(&[
        (0x0100_0007, 0, x86_64.as_slice()),
        (0x0100_000c, 0, arm64.as_slice()),
    ])?;
    let artifact = fixture.root.join("Customer Secret Large Universal Symbols");
    std::fs::write(artifact.as_path(), universal)?;

    let arm_digest = sha256_hex(arm64.as_slice());
    let x86_digest = sha256_hex(x86_64.as_slice());
    let chunks = [
        (
            sha256_hex(&arm64[..RESUMABLE_CHUNK_SIZE]),
            &arm64[..RESUMABLE_CHUNK_SIZE],
        ),
        (
            sha256_hex(&arm64[RESUMABLE_CHUNK_SIZE..]),
            &arm64[RESUMABLE_CHUNK_SIZE..],
        ),
        (
            sha256_hex(&x86_64[..RESUMABLE_CHUNK_SIZE]),
            &x86_64[..RESUMABLE_CHUNK_SIZE],
        ),
        (
            sha256_hex(&x86_64[RESUMABLE_CHUNK_SIZE..]),
            &x86_64[RESUMABLE_CHUNK_SIZE..],
        ),
    ];
    mount_lookup_sequence(
        &server,
        vec![
            missing_lookup(),
            missing_lookup(),
            found_lookup_for(ARM64_UUID, "arm64", arm_digest.as_str(), arm64.len(), 1),
            found_lookup_for(X86_64_UUID, "x86_64", x86_digest.as_str(), x86_64.len(), 2),
        ],
    )
    .await;
    let missing = chunks
        .iter()
        .map(|(digest, _)| digest.as_str())
        .collect::<Vec<_>>();
    Mock::given(method("POST"))
        .and(path("/api/native-debug-artifact-uploads"))
        .respond_with(ResponseTemplate::new(200).set_body_json(start_response(&missing)))
        .expect(1)
        .mount(&server)
        .await;
    for (digest, _) in &chunks {
        Mock::given(method("PUT"))
            .and(path(format!(
                "/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{digest}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(digest)))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path(format!(
            "/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(upload_success_body(2)))
        .expect(1)
        .mount(&server)
        .await;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout)?;
    assert_eq!(text.lines().count(), 1);
    let body = serde_json::from_str::<serde_json::Value>(text.as_str())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "verified");
    assert_eq!(body["upload_id"], UPLOAD_ID);
    assert_eq!(body["artifact_count"], 2);
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 10);
    assert_eq!(
        requests
            .iter()
            .filter(|request| {
                request.method.as_str() == "POST"
                    && request.url.path() == "/api/native-debug-artifacts"
            })
            .count(),
        0,
        "a resumable session must never replay the large compatibility body"
    );
    let start = requests
        .iter()
        .find(|request| request.url.path() == "/api/native-debug-artifact-uploads")
        .ok_or("missing resumable start")?;
    assert_eq!(header_value(start, "content-type")?, "application/json");
    let manifest = serde_json::from_slice::<serde_json::Value>(start.body.as_slice())?;
    assert_eq!(manifest["artifacts"].as_array().map(Vec::len), Some(2));
    assert_eq!(manifest["artifacts"][0]["imageUuid"], ARM64_UUID);
    assert_eq!(manifest["artifacts"][1]["imageUuid"], X86_64_UUID);
    for (digest, bytes) in &chunks {
        let request = requests
            .iter()
            .find(|request| request.url.path().ends_with(digest.as_str()))
            .ok_or("missing exact chunk request")?;
        assert_eq!(request.method.as_str(), "PUT");
        assert_eq!(
            header_value(request, "content-type")?,
            "application/octet-stream"
        );
        assert_eq!(request.body.as_slice(), *bytes);
    }
    Ok(())
}

fn found_lookup_for(
    image_uuid: &str,
    architecture: &str,
    digest: &str,
    byte_size: usize,
    id: u8,
) -> serde_json::Value {
    serde_json::json!({
        "artifact": {
            "artifact_id": format!("nativeartifact_{id:032x}"),
            "upload_id": UPLOAD_ID,
            "project_id": PROJECT_ID,
            "release": "checkout@1.2.3",
            "environment": "production",
            "service": "checkout-api",
            "artifact_type": "apple_dsym",
            "image_uuid": image_uuid,
            "architecture": architecture,
            "debug_file_sha256": digest,
            "debug_file_byte_size": byte_size,
            "upload_status": "uploaded",
            "created_at": "2026-07-20T12:00:00Z"
        },
        "next": "Native debug artifact lookup matched. Verify issue-detail native symbolication.",
        "next_action": {
            "code": "verify_native_issue_symbolication",
            "target": "native_issue_symbolication"
        }
    })
}
