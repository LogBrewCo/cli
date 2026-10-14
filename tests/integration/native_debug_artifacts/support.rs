//! Shared native debug-artifact integration fixtures.

use crate::{Mock, MockServer, Request, ResponseTemplate};
use std::ffi::OsString;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(in crate::integration) const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
pub(in crate::integration) const ARM64_UUID: &str = "10111213-1415-1617-1819-1a1b1c1d1e1f";
pub(in crate::integration) const X86_64_UUID: &str = "20212223-2425-2627-2829-2a2b2c2d2e2f";
pub(in crate::integration) const TOKEN: &str = "private-account-token-proof";
pub(in crate::integration) const ARTIFACT_TOKEN: &str = "lbw_ingest_artifact_private_value";
pub(in crate::integration) const UPLOAD_ID: &str = "nativeart_11111111111111111111111111111111";
const ARTIFACT_ID: &str = "nativeartifact_22222222222222222222222222222222";

pub(in crate::integration) fn upload_args(path: &std::ffi::OsStr) -> Vec<OsString> {
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

pub(in crate::integration) fn lookup_args<'a>(
    image_uuid: &'a str,
    architecture: &'a str,
) -> Vec<&'a str> {
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

pub(in crate::integration) async fn invoke<I, S>(
    fixture: &Fixture,
    base_url: &str,
    args: I,
) -> Result<Output, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    invoke_with_token(fixture, base_url, args, TOKEN).await
}

pub(in crate::integration) async fn invoke_with_token<I, S>(
    fixture: &Fixture,
    base_url: &str,
    args: I,
    token: &str,
) -> Result<Output, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut command = command_with_token(fixture, base_url, args, token);
    let output = tokio::task::spawn_blocking(move || command.output()).await??;
    Ok(output)
}

pub(in crate::integration) fn command_with_token<I, S>(
    fixture: &Fixture,
    base_url: &str,
    args: I,
    token: &str,
) -> Command
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut command = Command::new(env!("CARGO_BIN_EXE_logbrew"));
    let _command = command
        .args(args.into_iter().map(Into::into))
        .env_clear()
        .env("HOME", fixture.home.as_path())
        .env("LOGBREW_API_URL", base_url)
        .env("LOGBREW_TOKEN", token);
    command
}

pub(in crate::integration) fn upload_success_body(artifact_count: usize) -> serde_json::Value {
    serde_json::json!({
        "upload_id": UPLOAD_ID,
        "status": "uploaded",
        "artifact_count": artifact_count,
        "next": "Native debug artifact upload accepted. Verify exact image UUID and architecture lookup.",
        "next_action": {
            "code": "verify_native_debug_artifact_lookup",
            "target": "native_debug_artifact_lookup"
        }
    })
}

pub(in crate::integration) fn json_failure(
    output: &Output,
) -> Result<(String, serde_json::Value), Box<dyn std::error::Error>> {
    assert!(!output.status.success(), "invalid upload must fail");
    assert!(output.stderr.is_empty(), "JSON failure belongs on stdout");
    let text = String::from_utf8(output.stdout.clone())?;
    assert_eq!(text.lines().count(), 1, "failure must be one JSON line");
    let body = serde_json::from_str::<serde_json::Value>(text.as_str())?;
    assert!(body.is_object(), "failure must be an object");
    assert_eq!(body["ok"], false, "failure cannot claim success");
    Ok((text, body))
}

pub(in crate::integration) fn mount_lookup(server: &MockServer, body: &serde_json::Value) {
    Mock::route("GET", "/api/native-debug-artifacts")
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server);
}

pub(in crate::integration) fn mount_lookup_sequence(
    server: &MockServer,
    bodies: Vec<serde_json::Value>,
) {
    let attempt = AtomicUsize::new(0);
    let expected = u64::try_from(bodies.len()).unwrap_or(u64::MAX);
    Mock::route("GET", "/api/native-debug-artifacts")
        .respond_with(move |_request: &Request| {
            let index = attempt.fetch_add(1, Ordering::SeqCst);
            let body = bodies
                .get(index)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"unexpected": "sequence exhausted"}));
            ResponseTemplate::new(200).set_body_json(&body)
        })
        .expect(expected)
        .mount(server);
}

pub(in crate::integration) fn found_lookup(digest: &str, byte_size: usize) -> serde_json::Value {
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

pub(in crate::integration) fn missing_lookup() -> serde_json::Value {
    serde_json::json!({
        "artifact": null,
        "next": "No exact native debug artifact matched. Upload the release debug file and retry lookup.",
        "next_action": {
            "code": "upload_native_debug_artifact",
            "target": "native_debug_artifact_upload"
        }
    })
}

pub(in crate::integration) fn macho64(cpu_type: u32, uuid: [u8; 16]) -> Vec<u8> {
    macho64_with_subtype(cpu_type, 0, uuid)
}

pub(in crate::integration) fn macho64_with_subtype(
    cpu_type: u32,
    cpu_subtype: u32,
    uuid: [u8; 16],
) -> Vec<u8> {
    const COMMAND_BYTES: u32 = 24 + 152;
    const DATA_OFFSET: u32 = 32 + COMMAND_BYTES;
    let mut bytes = Vec::with_capacity(DATA_OFFSET as usize + 1);
    for value in [
        0xfeed_facf_u32,
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
    bytes.extend_from_slice(0x19_u32.to_le_bytes().as_slice());
    bytes.extend_from_slice(152_u32.to_le_bytes().as_slice());
    push_name(&mut bytes, b"__DWARF");
    for value in [0_u64, 1, u64::from(DATA_OFFSET), 1] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    for value in [0_u32, 0, 1, 0] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    push_name(&mut bytes, b"__debug_info");
    push_name(&mut bytes, b"__DWARF");
    for value in [0_u64, 1] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    for value in [DATA_OFFSET, 0, 0, 0, 0, 0, 0, 0] {
        bytes.extend_from_slice(value.to_le_bytes().as_slice());
    }
    bytes.push(1);
    bytes
}

fn push_name(bytes: &mut Vec<u8>, name: &[u8]) {
    let mut field = [0_u8; 16];
    field[..name.len()].copy_from_slice(name);
    bytes.extend_from_slice(field.as_slice());
}

pub(in crate::integration) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}

pub(in crate::integration) fn uuid_bytes(first: u8) -> [u8; 16] {
    std::array::from_fn(|index| first.saturating_add(u8::try_from(index).unwrap_or(u8::MAX)))
}

pub(in crate::integration) fn assert_private_values_absent(
    text: &str,
    fixture: &Fixture,
    base_url: &str,
) {
    let root = fixture.root.to_string_lossy();
    for private in [
        TOKEN,
        ARTIFACT_TOKEN,
        "Customer Secret",
        "private-arm",
        "private-x86",
        root.as_ref(),
        base_url,
    ] {
        assert!(!text.contains(private), "output exposed private input");
    }
}

pub(in crate::integration) fn assert_exact_lookup_query(request: &Request) {
    assert_eq!(request.method.as_str(), "GET", "lookup must read only");
    assert_eq!(
        request.url.path(),
        "/api/native-debug-artifacts",
        "lookup route changed"
    );
    assert_eq!(
        request.url.query(),
        Some(
            "project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&service=checkout-api&image_uuid=10111213-1415-1617-1819-1a1b1c1d1e1f&architecture=arm64"
        ),
        "lookup must preserve the exact artifact scope"
    );
}

pub(in crate::integration) fn header_value<'a>(
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

pub(in crate::integration) fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(in crate::integration) struct Fixture {
    pub(in crate::integration) root: std::path::PathBuf,
    home: std::path::PathBuf,
}

impl Fixture {
    pub(in crate::integration) fn new(label: &str) -> Result<Self, std::io::Error> {
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
