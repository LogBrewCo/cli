//! Native debug archive, resumable upload, auth, and retry contracts.

#[path = "native_debug_artifacts/archive_support.rs"]
mod archive_support;
#[path = "native_debug_artifacts/resumable_support.rs"]
mod resumable_support;
#[allow(dead_code)]
#[path = "native_debug_artifacts/support.rs"]
mod support;

use crate::{Mock, MockServer, Request, ResponseTemplate};
use archive_support::*;
use resumable_support::*;
use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use support::*;

const UPPERCASE_DWARFDUMP_UUID: &str = "10111213-1415-1617-1819-1A1B1C1D1E1F";
const MIXED_CASE_DWARFDUMP_UUID: &str = "10111213-1415-1617-1819-1a1B1c1D1e1F";
const ANDROID_ELF_UUID: &str = "cb3d22c0-8dd7-6229-2f4d-11671bb83d0f";
const ANDROID_ELF: &[u8] = include_bytes!("../fixtures/native_elf_fixture.so");
#[tokio::test]
async fn dsym_dry_run_discovers_identity_without_auth_or_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("dry-run")?;
    let dwarf = fixture
        .root
        .join("Customer Secret App.dSYM/Contents/Resources/DWARF");
    std::fs::create_dir_all(dwarf.as_path())?;
    std::fs::write(
        dwarf.join("private-app"),
        macho64(0x0100_000c, uuid_bytes(0x10)),
    )?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        dsym_upload_args(
            fixture.root.join("Customer Secret App.dSYM").as_os_str(),
            &[],
            true,
        ),
    )
    .await?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "validated");
    assert_eq!(body["artifact_count"], 1);
    assert_eq!(body["artifacts"][0]["image_uuid"], ARM64_UUID);
    assert_eq!(body["artifacts"][0]["architecture"], "arm64");
    assert!(body.get("upload_id").is_none());
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn dsym_dry_run_normalizes_apple_uuid_case() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("mixed-case-dwarfdump-uuid")?;
    let artifact = fixture.root.join("Customer Secret Mixed Case Symbols");
    std::fs::write(artifact.as_path(), macho64(0x0100_000c, uuid_bytes(0x10)))?;

    for supplied in [UPPERCASE_DWARFDUMP_UUID, MIXED_CASE_DWARFDUMP_UUID] {
        let output = invoke(
            &fixture,
            server.uri().as_str(),
            dsym_upload_args(artifact.as_os_str(), &[supplied], true),
        )
        .await?;
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;
        assert_eq!(body["artifacts"][0]["image_uuid"], ARM64_UUID);
        assert!(!text.contains(supplied));
        assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    }
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn resumable_upload_sends_only_missing_chunks_then_completes_and_verifies()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("resumable-success")?;
    let mut object = macho64(0x0100_000c, uuid_bytes(0x10));
    object.resize(RESUMABLE_CHUNK_SIZE + 257, 0x5a);
    let object_digest = sha256_hex(object.as_slice());
    let first_digest = sha256_hex(&object[..RESUMABLE_CHUNK_SIZE]);
    let final_digest = sha256_hex(&object[RESUMABLE_CHUNK_SIZE..]);
    let artifact = fixture.root.join("Customer Secret Resumable Symbols");
    std::fs::write(artifact.as_path(), object.as_slice())?;

    mount_lookup_sequence(
        &server,
        vec![
            missing_lookup(),
            found_lookup(object_digest.as_str(), object.len()),
        ],
    )
    .await;
    Mock::route("POST", "/api/native-debug-artifact-uploads")
        .respond_with(ResponseTemplate::new(200).set_body_json(start_response(&[&final_digest])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::route(
        "PUT",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{final_digest}"),
    )
    .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(&final_digest)))
    .expect(1)
    .mount(&server)
    .await;
    Mock::route(
        "POST",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete"),
    )
    .respond_with(ResponseTemplate::new(200).set_body_json(upload_success_body(1)))
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
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        ["GET", "POST", "PUT", "POST", "GET"]
    );
    assert_eq!(requests[0].url.path(), "/api/native-debug-artifacts");
    assert_eq!(requests[1].url.path(), "/api/native-debug-artifact-uploads");
    assert_eq!(
        requests[2].url.path(),
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{final_digest}")
    );
    assert_eq!(
        requests[3].url.path(),
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete")
    );
    assert_eq!(requests[4].url.path(), "/api/native-debug-artifacts");

    let start = &requests[1];
    assert_eq!(header_value(start, "content-type")?, "application/json");
    let manifest = serde_json::from_slice::<serde_json::Value>(start.body.as_slice())?;
    assert_eq!(
        manifest,
        serde_json::json!({
            "projectId": PROJECT_ID,
            "release": "checkout@1.2.3",
            "environment": "production",
            "service": "checkout-api",
            "artifactType": "apple_dsym_manifest",
            "validation": {"status": "ready"},
            "artifacts": [{
                "imageUuid": ARM64_UUID,
                "architecture": "arm64",
                "debugFile": {
                    "artifactSha256": object_digest,
                    "byteSize": object.len()
                },
                "chunks": [
                    {"sha256": first_digest, "byteSize": RESUMABLE_CHUNK_SIZE},
                    {"sha256": final_digest, "byteSize": 257}
                ]
            }]
        })
    );
    assert_eq!(
        header_value(&requests[2], "content-type")?,
        "application/octet-stream"
    );
    assert_eq!(requests[2].body, object[RESUMABLE_CHUNK_SIZE..]);
    assert!(requests[3].body.is_empty());
    assert!(requests.iter().all(
        |request| request.url.path() != "/api/native-debug-artifacts"
            || request.method.as_str() == "GET"
    ));
    Ok(())
}

#[tokio::test]
async fn resumable_retry_replays_only_the_ambiguous_chunk() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    let (fixture, artifact, object) = artifact("chunk-retry", 257)?;
    let object_digest = sha256_hex(object.as_slice());
    mount_lookup_sequence(
        &server,
        vec![
            missing_lookup(),
            found_lookup(object_digest.as_str(), object.len()),
        ],
    )
    .await;
    Mock::route("POST", "/api/native-debug-artifact-uploads")
        .respond_with(ResponseTemplate::new(200).set_body_json(start_response(&[&object_digest])))
        .expect(1)
        .mount(&server)
        .await;
    let attempt = Arc::new(AtomicUsize::new(0));
    let response_attempt = Arc::clone(&attempt);
    let response_digest = object_digest.clone();
    Mock::route(
        "PUT",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{object_digest}"),
    )
    .respond_with(move |_request: &Request| {
        if response_attempt.fetch_add(1, Ordering::SeqCst) == 0 {
            ResponseTemplate::new(503).set_body_string("hostile transient text")
        } else {
            ResponseTemplate::new(200).set_body_json(chunk_response(response_digest.as_str()))
        }
    })
    .expect(2)
    .mount(&server)
    .await;
    Mock::route(
        "POST",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete"),
    )
    .respond_with(ResponseTemplate::new(200).set_body_json(upload_success_body(1)))
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
    assert!(!text.contains("hostile transient text"));
    let requests = received_requests(&server).await?;
    assert_eq!(
        requests
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        ["GET", "POST", "PUT", "PUT", "POST", "GET"]
    );
    assert_eq!(requests[2].body, object);
    assert_eq!(requests[3].body, requests[2].body);
    assert_eq!(attempt.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn android_elf_uses_exact_resumable_manifest_and_whole_object()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("android-elf")?;
    let artifact = fixture.root.join("Customer Secret Android Symbols.so");
    std::fs::write(artifact.as_path(), ANDROID_ELF)?;
    let digest = sha256_hex(ANDROID_ELF);
    let mut found = found_lookup(digest.as_str(), ANDROID_ELF.len());
    found["artifact"]["artifact_type"] = serde_json::json!("android_elf");
    found["artifact"]["image_uuid"] = serde_json::json!(ANDROID_ELF_UUID);
    mount_lookup_sequence(&server, vec![missing_lookup(), found]).await;
    Mock::route("POST", "/api/native-debug-artifact-uploads")
        .respond_with(ResponseTemplate::new(200).set_body_json(start_response(&[&digest])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::route(
        "PUT",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{digest}"),
    )
    .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(&digest)))
    .expect(1)
    .mount(&server)
    .await;
    Mock::route(
        "POST",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete"),
    )
    .respond_with(ResponseTemplate::new(200).set_body_json(upload_success_body(1)))
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
    assert_eq!(body["status"], "verified");
    assert_eq!(body["artifacts"][0]["artifact_type"], "android_elf");
    assert_eq!(body["artifacts"][0]["image_uuid"], ANDROID_ELF_UUID);
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());

    let requests = received_requests(&server).await?;
    assert_eq!(
        requests
            .iter()
            .map(|request| (request.method.as_str(), request.url.path()))
            .collect::<Vec<_>>(),
        [
            ("GET", "/api/native-debug-artifacts"),
            ("POST", "/api/native-debug-artifact-uploads"),
            (
                "PUT",
                format!(
                    "/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{digest}"
                )
                .as_str()
            ),
            (
                "POST",
                format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete")
                    .as_str()
            ),
            ("GET", "/api/native-debug-artifacts"),
        ]
    );
    let manifest = serde_json::from_slice::<serde_json::Value>(requests[1].body.as_slice())?;
    assert_eq!(manifest["artifactType"], "android_elf_manifest");
    assert_eq!(manifest["artifacts"][0]["imageUuid"], ANDROID_ELF_UUID);
    assert_eq!(
        manifest["artifacts"][0]["debugFile"]["artifactSha256"],
        digest
    );
    assert_eq!(requests[2].body.as_slice(), ANDROID_ELF);
    for request in [&requests[0], &requests[4]] {
        assert_eq!(request.url.query(), Some(format!(
            "project_id={PROJECT_ID}&release=checkout%401.2.3&environment=production&service=checkout-api&image_uuid={ANDROID_ELF_UUID}&architecture=arm64"
        ).as_str()));
    }
    Ok(())
}

#[tokio::test]
async fn ambiguous_completion_recovers_through_exact_lookup_without_file_replay()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let (fixture, artifact, object) = artifact("complete-recovery", 257)?;
    let digest = sha256_hex(object.as_slice());
    mount_lookup_sequence(
        &server,
        vec![
            missing_lookup(),
            found_lookup(digest.as_str(), object.len()),
        ],
    )
    .await;
    Mock::route("POST", "/api/native-debug-artifact-uploads")
        .respond_with(ResponseTemplate::new(200).set_body_json(start_response(&[&digest])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::route(
        "PUT",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{digest}"),
    )
    .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(&digest)))
    .expect(1)
    .mount(&server)
    .await;
    Mock::route(
        "POST",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete"),
    )
    .respond_with(ResponseTemplate::new(503).set_body_string("hostile completion text"))
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
    let body = serde_json::from_str::<serde_json::Value>(text.as_str())?;
    assert_eq!(body["status"], "verified");
    assert_eq!(body["upload_id"], UPLOAD_ID);
    assert!(!text.contains("hostile completion text"));
    let requests = received_requests(&server).await?;
    assert_eq!(
        requests
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        ["GET", "POST", "PUT", "POST", "GET"]
    );
    assert!(requests.iter().all(
        |request| request.url.path() != "/api/native-debug-artifacts"
            || request.method.as_str() == "GET"
    ));
    Ok(())
}

#[tokio::test]
async fn non_pending_completion_422_preserves_missing_chunk_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    const MISSING_CHUNK_NEXT: &str =
        "retry only the missing native debug artifact chunk with its exact digest";
    let server = MockServer::start().await;
    let (fixture, artifact, object) = artifact("complete-missing-chunk", 257)?;
    let digest = sha256_hex(object.as_slice());
    mount_lookup(&server, missing_lookup()).await;
    Mock::route("POST", "/api/native-debug-artifact-uploads")
        .respond_with(ResponseTemplate::new(200).set_body_json(start_response(&[&digest])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::route(
        "PUT",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/chunks/{digest}"),
    )
    .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(&digest)))
    .expect(1)
    .mount(&server)
    .await;
    Mock::route(
        "POST",
        format!("/api/native-debug-artifact-uploads/{RESUMABLE_SESSION_ID}/complete"),
    )
    .respond_with(ResponseTemplate::new(422).set_body_json(error_envelope(
        "validation_failed",
        MISSING_CHUNK_NEXT,
        "upload_native_debug_artifact_chunks",
        "native_debug_artifact_upload",
    )))
    .expect(1)
    .mount(&server)
    .await;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    let (text, body) = json_failure(&output)?;
    assert_eq!(body["error"], "validation_failed");
    assert_eq!(body["next"], MISSING_CHUNK_NEXT);
    assert!(!text.contains("wait briefly"));
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    Ok(())
}

#[tokio::test]
async fn dsym_dry_run_rejects_an_exact_uuid_mismatch_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("uuid-mismatch")?;
    let dwarf = fixture
        .root
        .join("Customer Secret App.dSYM/Contents/Resources/DWARF");
    std::fs::create_dir_all(dwarf.as_path())?;
    std::fs::write(
        dwarf.join("private-app"),
        macho64(0x0100_000c, uuid_bytes(0x10)),
    )?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        dsym_upload_args(
            fixture.root.join("Customer Secret App.dSYM").as_os_str(),
            &[X86_64_UUID],
            true,
        ),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let (text, body) = json_failure(&output)?;
    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert!(!text.contains(X86_64_UUID));
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn dsym_dry_run_human_output_is_bounded_and_path_free()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("dry-run-human")?;
    let dwarf = fixture
        .root
        .join("Customer Secret App.dSYM/Contents/Resources/DWARF");
    std::fs::create_dir_all(dwarf.as_path())?;
    std::fs::write(
        dwarf.join("private-app"),
        macho64(0x0100_000c, uuid_bytes(0x10)),
    )?;
    let mut args = upload_args(fixture.root.join("Customer Secret App.dSYM").as_os_str());
    let _json = args.pop();
    args.push(OsString::from("--dry-run"));

    let output = invoke(&fixture, server.uri().as_str(), args).await?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout)?;
    assert_eq!(
        text,
        format!(
            "Native debug artifacts validated.\nArtifacts: 1\narm64 {ARM64_UUID} validated\nNext: rerun without --dry-run to upload.\n"
        )
    );
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn zip_dry_run_reads_only_dsym_debug_objects() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("zip-dry-run")?;
    let archive = fixture.root.join("Customer Secret Symbols.zip");
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    std::fs::write(
        archive.as_path(),
        stored_zip(&[
            ZipFixtureEntry::file(
                "Build/App.dSYM/Contents/Resources/DWARF/private-app",
                object.as_slice(),
            ),
            ZipFixtureEntry::file("Build/App.dSYM/Contents/Info.plist", b"ignored metadata"),
        ])?,
    )?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        dsym_upload_args(archive.as_os_str(), &[], true),
    )
    .await?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["status"], "validated");
    assert_eq!(body["artifact_count"], 1);
    assert_eq!(body["artifacts"][0]["image_uuid"], ARM64_UUID);
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn zip_rejects_unsafe_paths_and_symlinks_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("unsafe-zip")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    for (label, entries) in [
        (
            "traversal",
            vec![ZipFixtureEntry::file(
                "../App.dSYM/Contents/Resources/DWARF/private-app",
                object.as_slice(),
            )],
        ),
        (
            "symlink",
            vec![ZipFixtureEntry::symlink(
                "App.dSYM/Contents/Resources/DWARF/private-app",
                b"private-target",
            )],
        ),
    ] {
        let archive = fixture.root.join(format!("Customer Secret {label}.zip"));
        std::fs::write(archive.as_path(), stored_zip(entries.as_slice())?)?;
        let output = invoke(
            &fixture,
            server.uri().as_str(),
            dsym_upload_args(archive.as_os_str(), &[ARM64_UUID], true),
        )
        .await?;
        assert_eq!(output.status.code(), Some(1));
        let (text, body) = json_failure(&output)?;
        assert_eq!(body["error"], "native_debug_artifact_invalid");
        assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    }
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn zip_crc_tampering_fails_before_network() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("tampered-zip")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    let mut archive = stored_zip(&[ZipFixtureEntry::file(
        "Build/App.dSYM/Contents/Resources/DWARF/private-app",
        object.as_slice(),
    )])?;
    let payload_offset =
        find_subslice(archive.as_slice(), object.as_slice()).ok_or("missing ZIP payload")?;
    let tampered = payload_offset + object.len() - 1;
    archive[tampered] ^= 1;
    let path = fixture.root.join("Customer Secret Tampered.zip");
    std::fs::write(path.as_path(), archive)?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        dsym_upload_args(path.as_os_str(), &[], true),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let (text, body) = json_failure(&output)?;
    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn native_upload_rejects_ingest_key_auth_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("ingest-auth")?;
    let artifact = fixture.root.join("Customer Secret Symbols");
    std::fs::write(artifact.as_path(), macho64(0x0100_000c, uuid_bytes(0x10)))?;

    for token in [
        "lbw_ingest_private_project_key",
        " \t lbw_ingest_private_project_key",
    ] {
        let output = invoke_with_token(
            &fixture,
            server.uri().as_str(),
            upload_args(artifact.as_os_str()),
            token,
        )
        .await?;
        assert_eq!(output.status.code(), Some(1));
        let (text, body) = json_failure(&output)?;
        assert_eq!(body["error"], "unavailable");
        assert_eq!(
            body["next"],
            "run logbrew login and retry the native debug-artifact command"
        );
        assert!(!text.contains("lbw_ingest_private_project_key"));
        assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    }
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn hosted_http_and_embedded_url_state_fail_closed_without_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("unsafe-url")?;
    let artifact = fixture.root.join("Customer Secret Symbols");
    std::fs::write(artifact.as_path(), macho64(0x0100_000c, uuid_bytes(0x10)))?;
    for base_url in [
        "http://example.com",
        "https://private-user:private-password@example.com",
        "https://example.com?private-token=secret",
        "https://example.com#private-fragment",
    ] {
        let output = invoke(&fixture, base_url, upload_args(artifact.as_os_str())).await?;
        assert_eq!(output.status.code(), Some(1));
        let (text, body) = json_failure(&output)?;
        assert_eq!(body["error"], "unavailable");
        assert!(!text.contains(base_url));
        assert!(!text.contains("private-user"));
        assert!(!text.contains("private-password"));
        assert!(!text.contains("private-token"));
        assert!(!text.contains("private-fragment"));
    }
    Ok(())
}

#[tokio::test]
async fn exact_already_present_artifact_is_success_without_upload()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("already-present")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    let digest = sha256_hex(object.as_slice());
    mount_lookup(&server, found_lookup(digest.as_str(), object.len())).await;
    let artifact = fixture.root.join("Customer Secret Existing Symbols");
    std::fs::write(artifact.as_path(), object)?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "already_present");
    assert_eq!(body["artifact_count"], 1);
    assert_eq!(body["artifacts"][0]["status"], "already_present");
    assert!(body.get("upload_id").is_none());
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 1);
    assert_exact_lookup_query(&requests[0]);
    Ok(())
}
