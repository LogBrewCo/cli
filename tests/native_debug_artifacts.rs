//! Apple native debug-artifact command contracts.

#[path = "native_debug_artifacts/contract_support.rs"]
mod contract_support;
#[path = "native_debug_artifacts/support.rs"]
mod support;

use contract_support::*;
use support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn upload_grammar_is_closed_and_value_safe() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("upload-grammar")?;

    for args in [
        vec!["debug-artifacts", "upload", "--json"],
        vec![
            "debug-artifacts",
            "upload",
            "--authorization=private-account-token-proof",
            "--json",
        ],
        vec![
            "debug-artifacts",
            "upload",
            "ignored",
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@1",
            "--environment",
            "production",
            "--service",
            "checkout-api",
            "--image-uuid",
            ARM64_UUID,
            "--json",
        ],
        vec![
            "debug-artifacts",
            "upload",
            "hostile\npath",
            "--project",
            PROJECT_ID,
            "--release",
            "checkout@1",
            "--environment",
            "production",
            "--service",
            "checkout-api",
            "--json",
        ],
        vec!["debug-artifacts", "upload", "ignored", "extra", "--json"],
    ] {
        let output = invoke(&fixture, "http://127.0.0.1:9", args).await?;
        assert_eq!(output.status.code(), Some(2));
        let text = String::from_utf8(output.stderr)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["error"], "invalid_native_debug_command");
        assert_eq!(
            body["next"],
            "use logbrew debug-artifacts upload <path> --project <project_id> --release <release> --environment <environment> --service <service> with optional --expect-image-uuid, --dry-run, and --json"
        );
        assert_private_values_absent(text.as_str(), &fixture, "http://127.0.0.1:9");
    }
    Ok(())
}

#[tokio::test]
async fn lookup_grammar_rejects_noncanonical_identity_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("lookup-grammar")?;

    for (image_uuid, architecture) in [
        ("101112131415161718191a1b1c1d1e1f", "arm64"),
        ("10111213-1415-1617-1819-1A1B1C1D1E1F", "arm64"),
        (ARM64_UUID, "ARM64"),
        (ARM64_UUID, "arm64e/private"),
    ] {
        let output = invoke(
            &fixture,
            server.uri().as_str(),
            lookup_args(image_uuid, architecture),
        )
        .await?;
        assert_eq!(output.status.code(), Some(2));
        let text = String::from_utf8(output.stderr)?;
        let body: serde_json::Value = serde_json::from_str(text.as_str())?;

        assert_eq!(body["error"], "invalid_native_debug_command");
        assert_eq!(
            body["next"],
            "use a lowercase UUID and architecture arm64, arm64e, or x86_64"
        );
        assert!(!text.contains(image_uuid));
        if !matches!(architecture, "arm64" | "arm64e" | "x86_64") {
            assert!(!text.contains(architecture));
        }
        assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    }

    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn malformed_artifact_fails_before_network_without_path_reflection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("malformed-object")?;
    let artifact = fixture.root.join("Customer Secret Object.dwarf");
    std::fs::write(artifact.as_path(), b"not a Mach-O debug object")?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stderr)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;

    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert_eq!(
        body["next"],
        "provide one validated Apple dSYM, ZIP, or Mach-O object matching every --expect-image-uuid value"
    );
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn unreadable_debug_info_fails_before_network() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("unreadable-debug-info")?;
    let mut object = macho64(0x0100_000c, uuid_bytes(0x10));
    object[176..180].copy_from_slice(4096u32.to_le_bytes().as_slice());
    let artifact = fixture.root.join("Unreadable Debug Info");
    std::fs::write(artifact.as_path(), object)?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stderr)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn thin_macho_upload_uses_exact_manifest_and_binary_part_without_path_leak()
-> Result<(), Box<dyn std::error::Error>> {
    let server = malformed_success_server().await;
    let fixture = Fixture::new("thin-object")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    let artifact = fixture.root.join("Customer Secret Symbols");
    std::fs::write(artifact.as_path(), object.as_slice())?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    assert_invalid_response_is_redacted(&output, &fixture, &server)?;

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 2);
    let request = upload_request(requests.as_slice())?;
    assert_eq!(request.method.as_str(), "POST");
    assert_eq!(request.url.path(), "/api/native-debug-artifacts");
    assert_eq!(
        header_value(request, "authorization")?,
        format!("Bearer {TOKEN}")
    );
    let parts = multipart_parts(request)?;
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "manifest");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(parts[0].body.as_slice())?,
        manifest(serde_json::json!([{
            "imageUuid": ARM64_UUID,
            "architecture": "arm64",
            "debugFile": {
                "artifactSha256": sha256_hex(object.as_slice()),
                "byteSize": object.len()
            }
        }]))
    );
    assert_eq!(parts[1].name, "debug_file_0");
    assert_eq!(parts[1].body, object);
    assert_request_has_no_local_identity(request, &fixture);
    Ok(())
}

#[tokio::test]
async fn arm64e_subtype_is_preserved_in_manifest_identity() -> Result<(), Box<dyn std::error::Error>>
{
    let server = malformed_success_server().await;
    let fixture = Fixture::new("arm64e-object")?;
    let object = macho64_with_subtype(0x0100_000c, 2, uuid_bytes(0x30));
    let artifact = fixture.root.join("Customer Secret Arm64e Symbols");
    std::fs::write(artifact.as_path(), object.as_slice())?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    assert_invalid_response_is_redacted(&output, &fixture, &server)?;

    let requests = received_requests(&server).await?;
    let parts = multipart_parts(upload_request(requests.as_slice())?)?;
    let body = serde_json::from_slice::<serde_json::Value>(parts[0].body.as_slice())?;
    assert_eq!(body["artifacts"][0]["imageUuid"], ARM64E_UUID);
    assert_eq!(body["artifacts"][0]["architecture"], "arm64e");
    assert_eq!(parts[1].body, object);
    Ok(())
}

#[tokio::test]
async fn dsym_bundle_enumerates_objects_in_canonical_identity_order()
-> Result<(), Box<dyn std::error::Error>> {
    let server = malformed_success_server().await;
    let fixture = Fixture::new("bundle")?;
    let dwarf = fixture
        .root
        .join("Customer Secret App.dSYM/Contents/Resources/DWARF");
    std::fs::create_dir_all(dwarf.as_path())?;
    let x86_64 = macho64(0x0100_0007, uuid_bytes(0x20));
    let arm64 = macho64(0x0100_000c, uuid_bytes(0x10));
    std::fs::write(dwarf.join("z-private-x86"), x86_64.as_slice())?;
    std::fs::write(dwarf.join("a-private-arm"), arm64.as_slice())?;
    let bundle = fixture.root.join("Customer Secret App.dSYM");

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(bundle.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    assert_invalid_response_is_redacted(&output, &fixture, &server)?;

    let requests = received_requests(&server).await?;
    let upload = upload_request(requests.as_slice())?;
    let parts = multipart_parts(upload)?;
    assert_eq!(parts.len(), 3);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(parts[0].body.as_slice())?,
        manifest(serde_json::json!([
            {
                "imageUuid": ARM64_UUID,
                "architecture": "arm64",
                "debugFile": {
                    "artifactSha256": sha256_hex(arm64.as_slice()),
                    "byteSize": arm64.len()
                }
            },
            {
                "imageUuid": X86_64_UUID,
                "architecture": "x86_64",
                "debugFile": {
                    "artifactSha256": sha256_hex(x86_64.as_slice()),
                    "byteSize": x86_64.len()
                }
            }
        ]))
    );
    assert_eq!(parts[1].name, "debug_file_0");
    assert_eq!(parts[1].body, arm64);
    assert_eq!(parts[2].name, "debug_file_1");
    assert_eq!(parts[2].body, x86_64);
    assert_request_has_no_local_identity(upload, &fixture);
    Ok(())
}

#[tokio::test]
async fn universal_macho_upload_emits_one_thin_part_per_supported_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let server = malformed_success_server().await;
    let fixture = Fixture::new("universal")?;
    let arm64 = macho64(0x0100_000c, uuid_bytes(0x10));
    let x86_64 = macho64(0x0100_0007, uuid_bytes(0x20));
    let universal = universal_macho(&[
        (0x0100_0007, 0, x86_64.as_slice()),
        (0x0100_000c, 0, arm64.as_slice()),
    ])?;
    let artifact = fixture.root.join("Customer Secret Universal Symbols");
    std::fs::write(artifact.as_path(), universal)?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    assert_invalid_response_is_redacted(&output, &fixture, &server)?;

    let requests = received_requests(&server).await?;
    let parts = multipart_parts(upload_request(requests.as_slice())?)?;
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[1].name, "debug_file_0");
    assert_eq!(parts[1].body, arm64);
    assert_eq!(parts[2].name, "debug_file_1");
    assert_eq!(parts[2].body, x86_64);
    Ok(())
}

#[tokio::test]
async fn duplicate_bundle_identity_fails_before_network() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    let fixture = Fixture::new("duplicate")?;
    let dwarf = fixture.root.join("Duplicate.dSYM/Contents/Resources/DWARF");
    std::fs::create_dir_all(dwarf.as_path())?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    std::fs::write(dwarf.join("first"), object.as_slice())?;
    std::fs::write(dwarf.join("second"), object.as_slice())?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(fixture.root.join("Duplicate.dSYM").as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stderr)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn successful_upload_verifies_exact_lookup_and_emits_bounded_json()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let fixture = Fixture::new("composite-success")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    let digest = sha256_hex(object.as_slice());
    mount_upload_success(&server, 1).await;
    mount_lookup_sequence(
        &server,
        vec![
            missing_lookup(),
            found_lookup(digest.as_str(), object.len()),
        ],
    )
    .await;
    let artifact = fixture.root.join("Customer Secret Composite Symbols");
    std::fs::write(artifact.as_path(), object.as_slice())?;

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
    assert_eq!(body["status"], "verified");
    assert_eq!(body["upload_id"], UPLOAD_ID);
    assert_eq!(body["artifact_count"], 1);
    assert_eq!(body["artifacts"][0]["image_uuid"], ARM64_UUID);
    assert_eq!(body["artifacts"][0]["architecture"], "arm64");
    assert_eq!(body["artifacts"][0]["debug_file_sha256"], digest);
    assert!(body.get("project_id").is_none());
    assert!(!text.contains("checkout-api"));
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        ["GET", "POST", "GET"]
    );
    assert_exact_lookup_query(&requests[2]);
    Ok(())
}

#[tokio::test]
async fn upload_fails_closed_when_exact_lookup_is_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    let fixture = Fixture::new("missing-verification")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    mount_upload_success(&server, 1).await;
    mount_lookup_sequence(&server, vec![missing_lookup(), missing_lookup()]).await;
    let artifact = fixture.root.join("Missing Verification Symbols");
    std::fs::write(artifact.as_path(), object)?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stderr)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["error"], "native_debug_verification_failed");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    Ok(())
}

#[tokio::test]
async fn upload_fails_closed_when_lookup_hash_mismatches() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    let fixture = Fixture::new("mismatch-verification")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    mount_lookup(
        &server,
        found_lookup(
            "0000000000000000000000000000000000000000000000000000000000000000",
            object.len(),
        ),
    )
    .await;
    let artifact = fixture.root.join("Mismatched Verification Symbols");
    std::fs::write(artifact.as_path(), object)?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stderr)?;
    let body: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(body["error"], "native_debug_verification_failed");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    Ok(())
}

#[tokio::test]
async fn lookup_uses_exact_canonical_query_and_redacts_malformed_success()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "unexpected": "hostile backend text"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let fixture = Fixture::new("lookup")?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        lookup_args(ARM64_UUID, "arm64"),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    assert_invalid_response_is_redacted(&output, &fixture, &server)?;

    let requests = received_requests(&server).await?;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method.as_str(), "GET");
    assert_eq!(request.url.path(), "/api/native-debug-artifacts");
    assert_eq!(
        request.url.query(),
        Some(
            "project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&service=checkout-api&image_uuid=10111213-1415-1617-1819-1a1b1c1d1e1f&architecture=arm64"
        )
    );
    assert_eq!(
        header_value(request, "authorization")?,
        format!("Bearer {TOKEN}")
    );
    assert_request_has_no_local_identity(request, &fixture);
    Ok(())
}

#[tokio::test]
async fn standalone_lookup_distinguishes_found_and_missing_json()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new("lookup-states")?;
    let found_server = MockServer::start().await;
    mount_lookup(
        &found_server,
        found_lookup(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            209,
        ),
    )
    .await;
    let found = invoke(
        &fixture,
        found_server.uri().as_str(),
        lookup_args(ARM64_UUID, "arm64"),
    )
    .await?;
    assert!(found.status.success());
    assert!(found.stderr.is_empty());
    let found_text = String::from_utf8(found.stdout)?;
    let found_body: serde_json::Value = serde_json::from_str(found_text.as_str())?;
    assert_eq!(found_body["status"], "found");
    assert!(found_body["artifact"].is_object());
    assert_private_values_absent(found_text.as_str(), &fixture, found_server.uri().as_str());

    let missing_server = MockServer::start().await;
    mount_lookup(&missing_server, missing_lookup()).await;
    let missing = invoke(
        &fixture,
        missing_server.uri().as_str(),
        lookup_args(ARM64_UUID, "arm64"),
    )
    .await?;
    assert!(missing.status.success());
    assert!(missing.stderr.is_empty());
    let missing_text = String::from_utf8(missing.stdout)?;
    let missing_body: serde_json::Value = serde_json::from_str(missing_text.as_str())?;
    assert_eq!(missing_body["status"], "missing");
    assert!(missing_body["artifact"].is_null());
    assert_private_values_absent(
        missing_text.as_str(),
        &fixture,
        missing_server.uri().as_str(),
    );
    Ok(())
}
