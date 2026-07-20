//! Apple native debug-artifact command contracts.

use std::ffi::OsString;
use std::process::Output;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const ARM64_UUID: &str = "10111213-1415-1617-1819-1a1b1c1d1e1f";
const X86_64_UUID: &str = "20212223-2425-2627-2829-2a2b2c2d2e2f";
const ARM64E_UUID: &str = "30313233-3435-3637-3839-3a3b3c3d3e3f";
const TOKEN: &str = "private-account-token-proof";
const UPLOAD_ID: &str = "nativeart_11111111111111111111111111111111";
const ARTIFACT_ID: &str = "nativeartifact_22222222222222222222222222222222";

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
            "use logbrew debug-artifacts upload <path> --project <project_id> --release <release> --environment <environment> --service <service>"
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
        "provide one validated Apple dSYM bundle or Mach-O debug object"
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
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
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
    let parts = multipart_parts(requests.first().ok_or("missing upload request")?)?;
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
    let parts = multipart_parts(requests.first().ok_or("missing upload request")?)?;
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
    assert_request_has_no_local_identity(&requests[0], &fixture);
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
    let parts = multipart_parts(requests.first().ok_or("missing upload request")?)?;
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
    mount_lookup(&server, found_lookup(digest.as_str(), object.len())).await;
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
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method.as_str(), "POST");
    assert_eq!(requests[1].method.as_str(), "GET");
    assert_exact_lookup_query(&requests[1]);
    Ok(())
}

#[tokio::test]
async fn upload_fails_closed_when_exact_lookup_is_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start().await;
    let fixture = Fixture::new("missing-verification")?;
    let object = macho64(0x0100_000c, uuid_bytes(0x10));
    mount_upload_success(&server, 1).await;
    mount_lookup(&server, missing_lookup()).await;
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
    mount_upload_success(&server, 1).await;
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

fn upload_args(path: &std::ffi::OsStr) -> Vec<OsString> {
    vec![
        OsString::from("debug-artifacts"),
        OsString::from("upload"),
        path.to_owned(),
        OsString::from("--project"),
        OsString::from(PROJECT_ID),
        OsString::from("--release"),
        OsString::from("checkout@1.2.3"),
        OsString::from("--environment"),
        OsString::from("production"),
        OsString::from("--service"),
        OsString::from("checkout-api"),
        OsString::from("--json"),
    ]
}

fn lookup_args<'a>(image_uuid: &'a str, architecture: &'a str) -> Vec<&'a str> {
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

async fn invoke<I, S>(
    fixture: &Fixture,
    base_url: &str,
    args: I,
) -> Result<Output, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let binary = OsString::from(env!("CARGO_BIN_EXE_logbrew"));
    let home = fixture.home.clone();
    let base_url = base_url.to_owned();
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(binary)
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("LOGBREW_API_URL", base_url)
            .env("LOGBREW_TOKEN", TOKEN)
            .output()
    })
    .await??;
    Ok(output)
}

async fn malformed_success_server() -> MockServer {
    let server = MockServer::start().await;
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

async fn mount_upload_success(server: &MockServer, artifact_count: usize) {
    Mock::given(method("POST"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "upload_id": UPLOAD_ID,
            "status": "uploaded",
            "artifact_count": artifact_count,
            "next": "Native debug artifact upload accepted. Verify exact image UUID and architecture lookup.",
            "next_action": {
                "code": "verify_native_debug_artifact_lookup",
                "target": "native_debug_artifact_lookup"
            }
        })))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_lookup(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

fn found_lookup(digest: &str, byte_size: usize) -> serde_json::Value {
    serde_json::json!({
        "artifact": {
            "artifact_id": ARTIFACT_ID,
            "upload_id": UPLOAD_ID,
            "project_id": PROJECT_ID,
            "release": "checkout@1.2.3",
            "environment": "production",
            "service": "checkout-api",
            "artifact_type": "apple_dsym",
            "image_uuid": ARM64_UUID,
            "architecture": "arm64",
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

fn missing_lookup() -> serde_json::Value {
    serde_json::json!({
        "artifact": null,
        "next": "No exact native debug artifact matched. Upload the release dSYM and retry lookup.",
        "next_action": {
            "code": "upload_native_debug_artifact",
            "target": "native_debug_artifact_upload"
        }
    })
}

fn manifest(artifacts: serde_json::Value) -> serde_json::Value {
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

fn macho64(cpu_type: u32, uuid: [u8; 16]) -> Vec<u8> {
    macho64_with_subtype(cpu_type, 0, uuid)
}

fn macho64_with_subtype(cpu_type: u32, cpu_subtype: u32, uuid: [u8; 16]) -> Vec<u8> {
    const COMMAND_BYTES: u32 = 24 + 152;
    const DATA_OFFSET: u32 = 32 + COMMAND_BYTES;
    let mut bytes = Vec::with_capacity(DATA_OFFSET as usize + 1);
    for value in [
        0xfeed_facfu32,
        cpu_type,
        cpu_subtype,
        10,
        2,
        COMMAND_BYTES,
        0,
        0,
        0x1b,
        24,
    ] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    bytes.extend_from_slice(uuid.as_slice());
    bytes.extend_from_slice(0x19u32.to_le_bytes().as_slice());
    bytes.extend_from_slice(152u32.to_le_bytes().as_slice());
    push_name(&mut bytes, b"__DWARF");
    for value in [0u64, 1, u64::from(DATA_OFFSET), 1] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    for value in [0u32, 0, 1, 0] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    push_name(&mut bytes, b"__debug_info");
    push_name(&mut bytes, b"__DWARF");
    for value in [0u64, 1] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    for value in [DATA_OFFSET, 0, 0, 0, 0, 0, 0, 0] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    bytes.push(1);
    bytes
}

fn push_name(bytes: &mut Vec<u8>, name: &[u8]) {
    let mut field = [0u8; 16];
    field[..name.len()].copy_from_slice(name);
    bytes.extend_from_slice(field.as_slice());
}

fn universal_macho(slices: &[(u32, u32, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn uuid_bytes(first: u8) -> [u8; 16] {
    std::array::from_fn(|index| first.saturating_add(u8::try_from(index).unwrap_or(u8::MAX)))
}

fn assert_invalid_response_is_redacted(
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

fn assert_private_values_absent(text: &str, fixture: &Fixture, base_url: &str) {
    let root = fixture.root.to_string_lossy();
    for private in [
        TOKEN,
        "Customer Secret",
        "private-arm",
        "private-x86",
        root.as_ref(),
        base_url,
    ] {
        assert!(!text.contains(private));
    }
}

fn assert_request_has_no_local_identity(request: &Request, fixture: &Fixture) {
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

fn assert_exact_lookup_query(request: &Request) {
    assert_eq!(request.method.as_str(), "GET");
    assert_eq!(request.url.path(), "/api/native-debug-artifacts");
    assert_eq!(
        request.url.query(),
        Some(
            "project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&service=checkout-api&image_uuid=10111213-1415-1617-1819-1a1b1c1d1e1f&architecture=arm64"
        )
    );
}

fn header_value<'a>(
    request: &'a Request,
    name: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    request
        .headers
        .get(name)
        .ok_or_else(|| -> Box<dyn std::error::Error> { format!("missing {name} header").into() })?
        .to_str()
        .map_err(Into::into)
}

struct MultipartPart {
    name: String,
    body: Vec<u8>,
}

fn multipart_parts(request: &Request) -> Result<Vec<MultipartPart>, Box<dyn std::error::Error>> {
    let content_type = header_value(request, "content-type")?;
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .ok_or("unexpected multipart content type")?;
    let marker = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();

    for segment in split_subslice(request.body.as_slice(), marker.as_slice()) {
        let Some(segment) = segment.strip_prefix(b"\r\n") else {
            continue;
        };
        if segment.starts_with(b"--") {
            continue;
        }
        let segment = segment.strip_suffix(b"\r\n").unwrap_or(segment);
        let header_end = find_subslice(segment, b"\r\n\r\n").ok_or("missing part headers")?;
        let headers = std::str::from_utf8(&segment[..header_end])?;
        assert!(!headers.to_ascii_lowercase().contains("filename="));
        let disposition = headers
            .lines()
            .find(|line| line.starts_with("Content-Disposition: form-data;"))
            .ok_or("missing content disposition")?;
        let name = disposition
            .split(';')
            .find_map(|field| field.trim().strip_prefix("name=\"")?.strip_suffix('"'))
            .ok_or("missing part name")?;
        parts.push(MultipartPart {
            name: name.to_owned(),
            body: segment[header_end + 4..].to_vec(),
        });
    }
    Ok(parts)
}

fn split_subslice<'a>(haystack: &'a [u8], needle: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut rest = haystack;
    while let Some(index) = find_subslice(rest, needle) {
        parts.push(&rest[..index]);
        rest = &rest[index + needle.len()..];
    }
    parts.push(rest);
    parts
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn received_requests(
    server: &MockServer,
) -> Result<Vec<Request>, Box<dyn std::error::Error>> {
    server
        .received_requests()
        .await
        .ok_or_else(|| "request recording is disabled".into())
}

struct Fixture {
    root: std::path::PathBuf,
    home: std::path::PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "logbrew-native-debug-{label}-{}-{nonce}",
            std::process::id()
        ));
        let home = root.join("home");
        std::fs::create_dir_all(home.as_path())?;
        Ok(Self { root, home })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(self.root.as_path()));
    }
}
