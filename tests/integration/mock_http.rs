// Bounded HTTP/1 loopback fixture for the CLI integration contract.

use std::io::{Error, ErrorKind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

const HEADER_LIMIT: usize = 64 * 1024;
const REQUEST_LIMIT: usize = 128 * 1024 * 1024;

pub(crate) mod matchers {
    use super::Match;

    pub(crate) fn header(name: impl Into<String>, value: impl Into<String>) -> Match {
        Match::Header(name.into(), value.into())
    }

    pub(crate) fn query_param(name: impl Into<String>, value: impl Into<String>) -> Match {
        Match::Query(name.into(), value.into())
    }

    pub(crate) fn body_json(value: serde_json::Value) -> Match {
        Match::Json(value)
    }
}

pub(crate) enum Match {
    Method(String),
    Path(String),
    Header(String, String),
    Query(String, String),
    Json(serde_json::Value),
}

impl Match {
    fn matches(&self, request: &Request) -> bool {
        match self {
            Self::Method(value) => request.method.as_str() == value,
            Self::Path(value) => request.url.path() == value,
            Self::Header(name, value) => request
                .headers
                .get(name)
                .and_then(|header| header.to_str().ok())
                .is_some_and(|header| header == value),
            Self::Query(name, value) => request
                .url
                .query_pairs()
                .any(|(key, item)| key == name.as_str() && item == value.as_str()),
            Self::Json(value) => serde_json::from_slice::<serde_json::Value>(&request.body)
                .is_ok_and(|body| body == *value),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Request {
    pub(crate) method: reqwest::Method,
    pub(crate) url: reqwest::Url,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Vec<u8>,
}

pub(crate) trait Respond: Send + Sync + 'static {
    fn respond(&self, request: &Request) -> ResponseTemplate;
}

impl<F> Respond for F
where
    F: Fn(&Request) -> ResponseTemplate + Send + Sync + 'static,
{
    fn respond(&self, request: &Request) -> ResponseTemplate {
        self(request)
    }
}

pub(crate) fn retry_then(success: serde_json::Value) -> impl Respond {
    let calls = AtomicUsize::new(0);
    move |_request: &Request| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "hostile internal detail",
                "code": "internal_error",
                "next": "send the private token somewhere",
                "next_action": {"code": "retry", "target": "request"}
            }))
        } else {
            ResponseTemplate::new(200).set_body_json(success.clone())
        }
    }
}

#[derive(Clone)]
pub(crate) struct ResponseTemplate {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: std::time::Duration,
}

impl ResponseTemplate {
    pub(crate) fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            delay: std::time::Duration::ZERO,
        }
    }

    pub(crate) fn set_body_json(mut self, value: serde_json::Value) -> Self {
        self.body = serde_json::to_vec(&value)
            .unwrap_or_else(|error| panic!("test response JSON must serialize: {error}"));
        self.insert_header("content-type", "application/json")
    }

    pub(crate) fn set_body_string(mut self, value: impl Into<String>) -> Self {
        self.body = value.into().into_bytes();
        self
    }

    pub(crate) fn set_body_raw(mut self, value: impl AsRef<[u8]>, content_type: &str) -> Self {
        self.body = value.as_ref().to_vec();
        self.insert_header("content-type", content_type)
    }

    pub(crate) fn insert_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    pub(crate) fn set_delay(mut self, delay: std::time::Duration) -> Self {
        self.delay = delay;
        self
    }
}

impl Respond for ResponseTemplate {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.clone()
    }
}

struct MountedMock {
    matchers: Vec<Match>,
    response: Arc<dyn Respond>,
    expected: Option<u64>,
    received: u64,
}

pub(crate) struct Mock {
    matchers: Vec<Match>,
    response: Option<Arc<dyn Respond>>,
    expected: Option<u64>,
}

impl Mock {
    pub(crate) fn auth(method: &str, path: impl Into<String>, token: &str) -> Self {
        Self::route(method, path).and(Match::Header(
            "authorization".to_owned(),
            format!("Bearer {token}"),
        ))
    }

    pub(crate) fn route(method: &str, path: impl Into<String>) -> Self {
        Self {
            matchers: vec![Match::Method(method.to_owned()), Match::Path(path.into())],
            response: None,
            expected: None,
        }
    }

    pub(crate) fn and(mut self, matcher: Match) -> Self {
        self.matchers.push(matcher);
        self
    }

    pub(crate) fn respond_with(mut self, response: impl Respond) -> Self {
        self.response = Some(Arc::new(response));
        self
    }

    pub(crate) fn expect(mut self, count: u64) -> Self {
        self.expected = Some(count);
        self
    }

    pub(crate) fn mount(self, server: &MockServer) {
        lock(&server.state).mocks.push(MountedMock {
            matchers: self.matchers,
            response: self
                .response
                .unwrap_or_else(|| Arc::new(ResponseTemplate::new(200))),
            expected: self.expected,
            received: 0,
        });
    }
}

#[derive(Default)]
struct State {
    mocks: Vec<MountedMock>,
    requests: Vec<Request>,
}

pub(crate) struct MockServer {
    uri: String,
    state: Arc<Mutex<State>>,
    task: tokio::task::JoinHandle<()>,
}

impl MockServer {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap_or_else(|error| panic!("loopback test server must bind: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("loopback test address must exist: {error}"));
        let state = Arc::new(Mutex::new(State::default()));
        let server_state = Arc::clone(&state);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let connection_state = Arc::clone(&server_state);
                drop(tokio::spawn(async move {
                    let _result = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        handle_connection(stream, connection_state),
                    )
                    .await;
                }));
            }
        });
        Self {
            uri: format!("http://{address}"),
            state,
            task,
        }
    }

    pub(crate) fn uri(&self) -> String {
        self.uri.clone()
    }

    pub(crate) fn received_requests(&self) -> Vec<Request> {
        lock(&self.state).requests.clone()
    }

    pub(crate) fn reset(&self) {
        *lock(&self.state) = State::default();
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
        if !std::thread::panicking() {
            for mock in &lock(&self.state).mocks {
                if let Some(expected) = mock.expected {
                    assert_eq!(mock.received, expected, "unexpected request count");
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<State>>) -> Result<(), Error> {
    let Some(request) = read_request(&mut stream).await? else {
        return Ok(());
    };
    let response = response_for(&state, &request);
    tokio::time::sleep(response.delay).await;
    write_response(&mut stream, &response).await
}

fn response_for(state: &Mutex<State>, request: &Request) -> ResponseTemplate {
    let response = {
        let mut state = lock(state);
        state.requests.push(request.clone());
        state.mocks.iter_mut().find_map(|mock| {
            let matches = mock.matchers.iter().all(|matcher| matcher.matches(request));
            let available = mock
                .expected
                .is_none_or(|expected| expected == 0 || mock.received < expected);
            (matches && available).then(|| {
                mock.received += 1;
                Arc::clone(&mock.response)
            })
        })
    };
    response.map_or_else(
        || ResponseTemplate::new(404),
        |response| response.respond(request),
    )
}

async fn read_request(stream: &mut TcpStream) -> Result<Option<Request>, Error> {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) = find(&bytes, b"\r\n\r\n") {
            if index > HEADER_LIMIT {
                return Err(invalid("request headers too large"));
            }
            break index;
        }
        if bytes.len() >= HEADER_LIMIT || stream.read_buf(&mut bytes).await? == 0 {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(invalid("partial or oversized HTTP headers"))
            };
        }
    };
    let head = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| invalid("request headers are not UTF-8"))?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines
        .next()
        .ok_or_else(|| invalid("missing request line"))?
        .split_whitespace();
    let method = request_line
        .next()
        .ok_or_else(|| invalid("missing request method"))?
        .parse()
        .map_err(|_| invalid("invalid request method"))?;
    let target = request_line
        .next()
        .ok_or_else(|| invalid("missing request target"))?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| invalid("invalid request target"))?;
    let mut headers = HeaderMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("invalid request header"))?;
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| invalid("invalid request header name"))?;
        let value = HeaderValue::from_str(value.trim())
            .map_err(|_| invalid("invalid request header value"))?;
        let _replaced = headers.append(name, value);
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if length > REQUEST_LIMIT {
        return Err(invalid("request body too large"));
    }
    let body_start = header_end + 4;
    while bytes.len().saturating_sub(body_start) < length {
        if stream.read_buf(&mut bytes).await? == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "partial request body"));
        }
    }
    Ok(Some(Request {
        method,
        url,
        headers,
        body: bytes[body_start..body_start + length].to_vec(),
    }))
}

async fn write_response(stream: &mut TcpStream, response: &ResponseTemplate) -> Result<(), Error> {
    let mut head = format!(
        "HTTP/1.1 {} Test\r\ncontent-length: {}\r\nconnection: close\r\n",
        response.status,
        response.body.len()
    );
    for (name, value) in &response.headers {
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err(invalid("invalid response header"));
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    stream.write_all(format!("{head}\r\n").as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await
}

fn invalid(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
