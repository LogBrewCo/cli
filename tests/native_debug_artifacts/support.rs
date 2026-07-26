//! Shared native debug-artifact integration fixtures.

use std::ffi::OsString;
use std::process::Output;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub(crate) const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
pub(crate) const ARM64_UUID: &str = "10111213-1415-1617-1819-1a1b1c1d1e1f";
pub(crate) const X86_64_UUID: &str = "20212223-2425-2627-2829-2a2b2c2d2e2f";
pub(crate) const TOKEN: &str = "private-account-token-proof";
pub(crate) const UPLOAD_ID: &str = "nativeart_11111111111111111111111111111111";
const ARTIFACT_ID: &str = "nativeartifact_22222222222222222222222222222222";

pub(crate) fn upload_args(path: &std::ffi::OsStr) -> Vec<OsString> {
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

pub(crate) async fn invoke<I, S>(
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

pub(crate) async fn invoke_with_token<I, S>(
    fixture: &Fixture,
    base_url: &str,
    args: I,
    token: &str,
) -> Result<Output, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let binary = OsString::from(env!("CARGO_BIN_EXE_logbrew"));
    let home = fixture.home.clone();
    let base_url = base_url.to_owned();
    let token = token.to_owned();
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(binary)
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("LOGBREW_API_URL", base_url)
            .env("LOGBREW_TOKEN", token)
            .output()
    })
    .await??;
    Ok(output)
}

pub(crate) fn upload_success_body(artifact_count: usize) -> serde_json::Value {
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

pub(crate) fn json_failure(
    output: &Output,
) -> Result<(String, serde_json::Value), Box<dyn std::error::Error>> {
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout.clone())?;
    assert_eq!(text.lines().count(), 1);
    let body = serde_json::from_str::<serde_json::Value>(text.as_str())?;
    assert!(body.is_object());
    assert_eq!(body["ok"], false);
    Ok((text, body))
}

pub(crate) async fn mount_lookup(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/api/native-debug-artifacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
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

pub(crate) fn found_lookup(digest: &str, byte_size: usize) -> serde_json::Value {
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

pub(crate) fn missing_lookup() -> serde_json::Value {
    serde_json::json!({
        "artifact": null,
        "next": "No exact native debug artifact matched. Upload the release dSYM and retry lookup.",
        "next_action": {
            "code": "upload_native_debug_artifact",
            "target": "native_debug_artifact_upload"
        }
    })
}

pub(crate) fn macho64(cpu_type: u32, uuid: [u8; 16]) -> Vec<u8> {
    macho64_with_subtype(cpu_type, 0, uuid)
}

pub(crate) fn macho64_with_subtype(cpu_type: u32, cpu_subtype: u32, uuid: [u8; 16]) -> Vec<u8> {
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

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn uuid_bytes(first: u8) -> [u8; 16] {
    std::array::from_fn(|index| first.saturating_add(u8::try_from(index).unwrap_or(u8::MAX)))
}

pub(crate) fn assert_private_values_absent(text: &str, fixture: &Fixture, base_url: &str) {
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

pub(crate) fn assert_exact_lookup_query(request: &Request) {
    assert_eq!(request.method.as_str(), "GET");
    assert_eq!(request.url.path(), "/api/native-debug-artifacts");
    assert_eq!(
        request.url.query(),
        Some(
            "project_id=123e4567-e89b-12d3-a456-426614174000&release=checkout%401.2.3&environment=production&service=checkout-api&image_uuid=10111213-1415-1617-1819-1a1b1c1d1e1f&architecture=arm64"
        )
    );
}

pub(crate) fn upload_request(requests: &[Request]) -> Result<&Request, Box<dyn std::error::Error>> {
    requests
        .iter()
        .find(|request| {
            request.method.as_str() == "POST" && request.url.path() == "/api/native-debug-artifacts"
        })
        .ok_or_else(|| "missing upload request".into())
}

pub(crate) fn header_value<'a>(
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

pub(crate) struct MultipartPart {
    pub(crate) name: String,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn multipart_parts(
    request: &Request,
) -> Result<Vec<MultipartPart>, Box<dyn std::error::Error>> {
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

pub(crate) fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(crate) async fn received_requests(
    server: &MockServer,
) -> Result<Vec<Request>, Box<dyn std::error::Error>> {
    server
        .received_requests()
        .await
        .ok_or_else(|| "request recording is disabled".into())
}

pub(crate) struct Fixture {
    pub(crate) root: std::path::PathBuf,
    home: std::path::PathBuf,
}

impl Fixture {
    pub(crate) fn new(label: &str) -> Result<Self, std::io::Error> {
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
