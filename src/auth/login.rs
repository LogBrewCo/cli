//! Interactive browser login and loopback callback handling.

use crate::{CliEnvironment, RuntimeError};

/// Provider used by the first-party CLI browser login.
const LOGIN_PROVIDER: &str = "github";
/// Loopback callback path accepted by the CLI listener.
const CALLBACK_PATH: &str = "/callback";
/// Maximum time the CLI waits for the provider round trip.
const LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
/// Maximum HTTP request header accepted by the loopback listener.
const CALLBACK_REQUEST_LIMIT: usize = 8 * 1024;
/// Lowercase hexadecimal alphabet for OAuth state encoding.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Executes interactive login or a non-mutating handoff mode.
pub(super) async fn execute_login<W: std::io::Write>(
    env: &CliEnvironment,
    should_open_browser: bool,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if json || !should_open_browser {
        return write_auth_handoff(env, json, output);
    }
    execute_with_opener(env, output, open_browser).await
}

/// Writes the legacy agent/manual handoff without binding or persisting state.
fn write_auth_handoff<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let auth_url = format!("{}/api/auth/cli/login", env.base_url.trim_end_matches('/'));
    if json {
        let body = serde_json::json!({
            "ok": true,
            "auth_url": auth_url,
            "browser_opened": false,
            "next": "open auth_url in a browser",
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "Open this URL to log in: {auth_url}")?;
        writeln!(output, "Browser: not opened")?;
        writeln!(output, "Next: open the URL in a browser")?;
    }
    Ok(())
}

/// Opens a URL in the user's default browser.
fn open_browser(url: &str) -> bool {
    let command = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("rundll32", vec!["url.dll,FileProtocolHandler", url])
    } else {
        ("xdg-open", vec![url])
    };

    std::process::Command::new(command.0)
        .args(command.1)
        .status()
        .is_ok_and(|status| status.success())
}

/// Executes interactive login with an injectable browser opener.
pub(super) async fn execute_with_opener<W, F>(
    env: &CliEnvironment,
    output: &mut W,
    open_browser: F,
) -> Result<(), RuntimeError>
where
    W: std::io::Write,
    F: FnOnce(&str) -> bool,
{
    let Some(home) = env.home.clone() else {
        return Err(login_unavailable("a home directory is required for login"));
    };
    let origin = super::normalized_api_base(env.base_url.as_str())?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let callback_address = listener.local_addr()?;
    let redirect_uri = format!("http://{callback_address}{CALLBACK_PATH}");
    let state = random_state()?;
    let auth_url = build_auth_url(origin.as_str(), redirect_uri.as_str(), state.as_str())?;

    if !open_browser(auth_url.as_str()) {
        return Err(login_unavailable("could not open the browser for login"));
    }
    writeln!(output, "Waiting for browser login...")?;

    let code = wait_for_callback(&listener, state.as_str()).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?;
    let exchange_url = format!("{}/api/auth/{LOGIN_PROVIDER}", origin.trim_end_matches('/'));
    let response = client
        .post(exchange_url)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(login_unavailable("browser login could not be completed"));
    }
    let value = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| login_unavailable("browser login returned an invalid response"))?;
    let access_token = token_field(&value, "access_token")?;
    let refresh_token = token_field(&value, "refresh_token")?;
    tokio::task::spawn_blocking(move || {
        let lock = super::store::CredentialStoreLock::exclusive(home.as_path())?;
        lock.persist(
            access_token.as_str(),
            refresh_token.as_str(),
            origin.as_str(),
        )
    })
    .await
    .map_err(|_| login_unavailable("local authentication storage failed"))??;

    writeln!(output, "Logged in to LogBrew.")?;
    writeln!(output, "Next: run logbrew status")?;
    Ok(())
}

/// Builds the provider-login URL without rendering state to CLI output.
fn build_auth_url(
    base_url: &str,
    redirect_uri: &str,
    state: &str,
) -> Result<reqwest::Url, RuntimeError> {
    let mut url = reqwest::Url::parse(
        format!("{}/api/auth/cli/login", base_url.trim_end_matches('/')).as_str(),
    )
    .map_err(|_| login_unavailable("the configured API URL is invalid"))?;
    let _query = url
        .query_pairs_mut()
        .append_pair("provider", LOGIN_PROVIDER)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("state", state);
    Ok(url)
}

/// Creates a cryptographically random OAuth state value.
fn random_state() -> Result<String, RuntimeError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| login_unavailable("secure login state generation failed"))?;
    let mut state = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        state.push(char::from(HEX[usize::from(byte >> 4)]));
        state.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(state)
}

/// Waits for one matching provider callback while rejecting unrelated local requests.
async fn wait_for_callback(
    listener: &tokio::net::TcpListener,
    expected_state: &str,
) -> Result<String, RuntimeError> {
    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(login_unavailable("browser login timed out"));
        }
        let (mut stream, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .map_err(|_| login_unavailable("browser login timed out"))??;
        match read_callback(&mut stream, expected_state).await {
            Callback::Authorized(code) => {
                write_browser_response(
                    &mut stream,
                    200,
                    "Authorization received. Return to the terminal to confirm login.",
                )
                .await?;
                return Ok(code);
            }
            Callback::ProviderError => {
                write_browser_response(
                    &mut stream,
                    400,
                    "LogBrew authorization was not completed. Return to the terminal.",
                )
                .await?;
                return Err(login_unavailable("browser authorization was not completed"));
            }
            Callback::Invalid => {
                write_browser_response(&mut stream, 400, "Invalid LogBrew login callback.").await?;
            }
        }
    }
}

/// Reads and validates one bounded loopback HTTP request.
async fn read_callback(stream: &mut tokio::net::TcpStream, expected_state: &str) -> Callback {
    use tokio::io::AsyncReadExt as _;

    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    while request.len() < CALLBACK_REQUEST_LIMIT {
        let Ok(Ok(read)) =
            tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut chunk)).await
        else {
            return Callback::Invalid;
        };
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    parse_callback(request.as_slice(), expected_state)
}

/// Parses the callback request line and exact public query fields.
fn parse_callback(request: &[u8], expected_state: &str) -> Callback {
    let Ok(request) = std::str::from_utf8(request) else {
        return Callback::Invalid;
    };
    let Some(request_line) = request.lines().next() else {
        return Callback::Invalid;
    };
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Callback::Invalid;
    };
    if !version.starts_with("HTTP/1.") {
        return Callback::Invalid;
    }
    let Ok(url) = reqwest::Url::parse(format!("http://localhost{target}").as_str()) else {
        return Callback::Invalid;
    };
    if url.path() != CALLBACK_PATH {
        return Callback::Invalid;
    }

    let mut provider = None;
    let mut state = None;
    let mut code = None;
    let mut provider_error = None;
    for (name, value) in url.query_pairs() {
        let slot = match name.as_ref() {
            "provider" => &mut provider,
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut provider_error,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            return Callback::Invalid;
        }
    }
    if provider.as_deref() != Some(LOGIN_PROVIDER)
        || !state
            .as_deref()
            .is_some_and(|state| constant_time_equal(state, expected_state))
    {
        return Callback::Invalid;
    }
    if provider_error
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Callback::ProviderError;
    }
    code.filter(|value| !value.trim().is_empty())
        .map_or(Callback::Invalid, Callback::Authorized)
}

/// Compares the fixed-size OAuth state without a content-dependent early exit.
fn constant_time_equal(candidate: &str, expected: &str) -> bool {
    if candidate.len() != expected.len() {
        return false;
    }
    candidate
        .bytes()
        .zip(expected.bytes())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

/// Writes a minimal secret-free HTTP response to the browser.
async fn write_browser_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> Result<(), RuntimeError> {
    use tokio::io::AsyncWriteExt as _;

    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

/// Extracts one non-empty token without exposing its value in errors.
fn token_field(value: &serde_json::Value, field: &str) -> Result<String, RuntimeError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| login_unavailable("browser login returned an invalid response"))
}

/// Secret-free callback classification.
enum Callback {
    /// Provider returned a one-time exchange code.
    Authorized(String),
    /// Provider returned an explicit error for the matching state.
    ProviderError,
    /// Request was malformed, duplicated, unrelated, or had the wrong state.
    Invalid,
}

/// Builds stable interactive-login recovery without provider details or credentials.
const fn login_unavailable(message: &'static str) -> RuntimeError {
    RuntimeError::Unavailable {
        message,
        next: "retry logbrew login",
    }
}

#[cfg(test)]
mod tests {
    use super::execute_with_opener;
    use crate::CliEnvironment;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn loopback_login_exchanges_code_and_persists_redacted_token_pair()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth/github"))
            .and(body_json(serde_json::json!({ "code": "provider-code" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "saved-access",
                "refresh_token": "saved-refresh"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let home = login_home("loopback-success")?;
        let env = CliEnvironment {
            base_url: server.uri(),
            token: None,
            home: Some(home.clone()),
            cwd: None,
        };
        let mut output = Vec::new();

        execute_with_opener(&env, &mut output, |auth_url| {
            let auth_url = reqwest::Url::parse(auth_url).expect("valid auth URL");
            assert_eq!(auth_url.path(), "/api/auth/cli/login");
            let query = auth_url
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(
                query.get("provider").map(|value| value.as_ref()),
                Some("github")
            );
            let redirect_uri = query.get("redirect_uri").expect("redirect URI").to_string();
            let state = query.get("state").expect("state").to_string();
            let callback = reqwest::Url::parse(redirect_uri.as_str()).expect("callback URL");
            let _callback = tokio::spawn(async move {
                let mut wrong_state = callback.clone();
                let _query = wrong_state
                    .query_pairs_mut()
                    .append_pair("provider", "github")
                    .append_pair("code", "ignored-code")
                    .append_pair("state", "wrong-state");
                let response = reqwest::get(wrong_state)
                    .await
                    .expect("wrong-state response");
                assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);

                let mut valid = callback;
                let _query = valid
                    .query_pairs_mut()
                    .append_pair("provider", "github")
                    .append_pair("code", "provider-code")
                    .append_pair("state", state.as_str());
                let _response = reqwest::get(valid).await;
            });
            true
        })
        .await?;

        let text = String::from_utf8(output)?;
        assert!(text.contains("Logged in to LogBrew."));
        for secret in ["provider-code", "saved-access", "saved-refresh"] {
            assert!(!text.contains(secret));
        }
        let saved: serde_json::Value = serde_json::from_str(
            std::fs::read_to_string(home.join(".logbrew/session.json"))?.as_str(),
        )?;
        assert_eq!(saved["access_token"], "saved-access");
        assert_eq!(saved["refresh_token"], "saved-refresh");
        assert_eq!(saved["origin"], server.uri());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(home.join(".logbrew"))?
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            for name in ["session.json", "credentials.lock"] {
                assert_eq!(
                    std::fs::metadata(home.join(".logbrew").join(name))?
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
        Ok(())
    }

    #[test]
    fn callback_parser_rejects_duplicate_or_mismatched_security_fields() {
        let duplicate =
            b"GET /callback?provider=github&state=expected&state=expected&code=a HTTP/1.1\r\n\r\n";
        assert!(matches!(
            super::parse_callback(duplicate, "expected"),
            super::Callback::Invalid
        ));
        let wrong_provider =
            b"GET /callback?provider=gitlab&state=expected&code=a HTTP/1.1\r\n\r\n";
        assert!(matches!(
            super::parse_callback(wrong_provider, "expected"),
            super::Callback::Invalid
        ));
    }

    /// Creates one isolated home directory for login tests.
    fn login_home(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
        let home =
            std::env::temp_dir().join(format!("logbrew-cli-login-{name}-{}", std::process::id()));
        match std::fs::remove_dir_all(home.as_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::create_dir_all(home.as_path())?;
        Ok(home)
    }
}
