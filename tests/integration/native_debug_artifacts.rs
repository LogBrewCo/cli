//! Native debug-artifact command and lookup contracts.

#[allow(dead_code)]
#[path = "native_debug_artifacts/contract_support.rs"]
mod contract_support;
#[allow(dead_code)]
#[path = "native_debug_artifacts/support.rs"]
mod support;

use crate::{Mock, MockServer, ResponseTemplate};
use contract_support::*;
use support::*;

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
        let (text, body) = json_failure(&output)?;

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
        ("10111213-1415-1617-1819-1a1b1c1d1e1", "arm64"),
        ("10111213-1415-1617-1819-1a1b1c1d1e1f00", "arm64"),
        ("10111213-1415-1617-1819-1a1b1c1d1e1g", "arm64"),
        ("1011121-31415-1617-1819-1a1b1c1d1e1f", "arm64"),
        (" 10111213-1415-1617-1819-1a1b1c1d1e1f", "arm64"),
        ("10111213-1415-1617-1819-1a1b1c1d1e1f ", "arm64"),
        ("10111213‐1415-1617-1819-1a1b1c1d1e1f", "arm64"),
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
        let (text, body) = json_failure(&output)?;

        assert_eq!(body["error"], "invalid_native_debug_command");
        assert_eq!(
            body["next"],
            "use a UUID in 8-4-4-4-12 form and architecture arm, arm64, arm64e, x86, or x86_64"
        );
        assert!(!text.contains(image_uuid));
        assert!(architecture == "arm64" || !text.contains(architecture));
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
    std::fs::write(artifact.as_path(), b"not a native debug object")?;

    let output = invoke(
        &fixture,
        server.uri().as_str(),
        upload_args(artifact.as_os_str()),
    )
    .await?;
    assert_eq!(output.status.code(), Some(1));
    let (text, body) = json_failure(&output)?;

    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert_eq!(
        body["next"],
        "provide one validated Apple dSYM, ZIP, Mach-O object, or Android ELF with debug information matching every --expect-image-uuid value"
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
    let (text, body) = json_failure(&output)?;
    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
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
    let (text, body) = json_failure(&output)?;
    assert_eq!(body["error"], "native_debug_artifact_invalid");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    assert!(received_requests(&server).await?.is_empty());
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
    let (text, body) = json_failure(&output)?;
    assert_eq!(body["error"], "native_debug_verification_failed");
    assert_private_values_absent(text.as_str(), &fixture, server.uri().as_str());
    Ok(())
}

#[tokio::test]
async fn lookup_uses_exact_canonical_query_and_redacts_malformed_success()
-> Result<(), Box<dyn std::error::Error>> {
    for architecture in ["arm", "arm64", "arm64e", "x86", "x86_64"] {
        let server = MockServer::start().await;
        Mock::route("GET", "/api/native-debug-artifacts")
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
            lookup_args(ARM64_UUID, architecture),
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
        Some(format!(
            "project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&service=checkout-api&image_uuid=10111213-1415-1617-1819-1a1b1c1d1e1f&architecture={architecture}"
        ).as_str())
    );
        assert_eq!(
            header_value(request, "authorization")?,
            format!("Bearer {TOKEN}")
        );
        assert_request_has_no_local_identity(request, &fixture);
    }
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
