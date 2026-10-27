//! Device sign-in: one link and one code that a person opens on any device,
//! then polling until they approve, deny, or the code expires.

use std::time::Duration;

use crate::{CliEnvironment, RuntimeError};

use super::login::login_unavailable;

/// Client identifier sent with every device sign-in request.
const CLIENT_ID: &str = "logbrew-cli";
/// Client name the approval page shows.
const CLIENT_NAME: &str = "LogBrew CLI";
/// Device authorization grant type.
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Largest device sign-in response body the CLI reads.
const BODY_LIMIT: usize = 16 * 1024;
/// Seconds added to the polling interval after each `slow_down`.
const SLOW_DOWN: Duration = Duration::from_secs(5);
/// Unreachable polls in a row that end the wait.
const MAX_FAILED_POLLS: u32 = 5;
/// Response that a failed or malformed exchange reports.
const INVALID: &str = "device sign-in returned an invalid response";

/// A validated device authorization: the secret code and what the person sees.
struct Authorization {
    /// Secret the CLI polls with. Never rendered.
    device_code: String,
    /// Code the person checks on the approval page.
    user_code: String,
    /// The one link the person opens.
    link: String,
    /// Seconds until the codes expire.
    expires_in: u64,
    /// Seconds between polls.
    interval: u64,
}

/// One poll result.
#[derive(Debug, PartialEq, Eq)]
enum Poll {
    /// Approved: the token response.
    Approved(serde_json::Value),
    /// Not decided yet.
    Pending,
    /// Polled too soon; wait 5 more seconds from now on.
    SlowDown,
    /// The person denied the sign-in.
    Denied,
    /// The codes expired.
    Expired,
    /// The server could not answer; try again at the next interval.
    Unreachable,
    /// Any other answer.
    Failed,
}

/// Prints a link and a code, waits for approval, and stores the tokens.
pub(super) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let Some(home) = env.home.clone() else {
        return Err(login_unavailable("a home directory is required for login"));
    };
    let origin = super::normalized_api_base(env.base_url.as_str())?;
    let client = crate::http::api_client()?;
    let authorization = authorize(&client, origin.as_str()).await?;
    if json {
        let body = serde_json::json!({
            "ok": true,
            "status": "waiting_for_approval",
            "link": authorization.link,
            "user_code": authorization.user_code,
            "expires_in": authorization.expires_in,
            "next": "open link in a browser, check that it shows user_code, and approve",
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "Open this link to sign in: {}", authorization.link)?;
        writeln!(output, "Code: {}", authorization.user_code)?;
        writeln!(output, "Waiting for approval...")?;
    }
    output.flush()?;
    let tokens = wait_for_approval(&client, origin.as_str(), &authorization).await?;
    super::login::store_session(home, origin, &tokens, INVALID).await?;
    if json {
        let body = serde_json::json!({
            "ok": true,
            "status": "logged_in",
            "next": "run logbrew status",
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "Logged in to LogBrew.")?;
        writeln!(output, "Next: run logbrew status")?;
    }
    Ok(())
}

/// Starts device sign-in.
async fn authorize(client: &reqwest::Client, origin: &str) -> Result<Authorization, RuntimeError> {
    let failed = || login_unavailable("device sign-in could not start");
    let response = client
        .post(format!("{origin}/api/auth/device"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form(&[
            ("client_id", CLIENT_ID),
            ("client_name", CLIENT_NAME),
        ]))
        .send()
        .await
        .map_err(|_| failed())?;
    if !response.status().is_success() {
        return Err(failed());
    }
    let body = crate::http::bounded_body(response, BODY_LIMIT)
        .await
        .map_err(|_| failed())?;
    parse_authorization(body.as_str()).ok_or_else(|| login_unavailable(INVALID))
}

/// Reads the authorization fields the CLI relies on, failing closed on any surprise.
fn parse_authorization(body: &str) -> Option<Authorization> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let text = |name: &str| value.get(name).and_then(serde_json::Value::as_str);
    let device_code = text("device_code").filter(|code| {
        (1..=512).contains(&code.len()) && code.bytes().all(|byte| byte.is_ascii_graphic())
    })?;
    let user_code = text("user_code").filter(|code| {
        (1..=32).contains(&code.len())
            && code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })?;
    let link = text("verification_uri_complete")
        .or_else(|| text("verification_uri"))
        .filter(|link| valid_link(link))?;
    let expires_in = value
        .get("expires_in")?
        .as_u64()
        .filter(|seconds| (1..=3600).contains(seconds))?;
    let interval = match value.get("interval") {
        None => 5,
        Some(interval) => interval
            .as_u64()
            .filter(|seconds| (1..=60).contains(seconds))?,
    };
    Some(Authorization {
        device_code: device_code.to_owned(),
        user_code: user_code.to_owned(),
        link: link.to_owned(),
        expires_in,
        interval,
    })
}

/// An absolute HTTP(S) link without credentials, safe to print.
fn valid_link(link: &str) -> bool {
    link.len() <= 2048
        && link.bytes().all(|byte| byte.is_ascii_graphic())
        && reqwest::Url::parse(link).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

/// Polls at the server's interval until the sign-in ends one way or another.
async fn wait_for_approval(
    client: &reqwest::Client,
    origin: &str,
    authorization: &Authorization,
) -> Result<serde_json::Value, RuntimeError> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(authorization.expires_in);
    let mut interval = Duration::from_secs(authorization.interval);
    let mut failed_polls = 0;
    loop {
        tokio::time::sleep(interval).await;
        if tokio::time::Instant::now() >= deadline {
            return Err(expired());
        }
        match poll(client, origin, authorization.device_code.as_str()).await {
            Poll::Approved(tokens) => return Ok(tokens),
            Poll::Pending => failed_polls = 0,
            Poll::SlowDown => {
                failed_polls = 0;
                interval += SLOW_DOWN;
            }
            Poll::Unreachable if failed_polls + 1 < MAX_FAILED_POLLS => failed_polls += 1,
            Poll::Unreachable => {
                return Err(RuntimeError::Unavailable {
                    message: "the LogBrew API stopped answering during device sign-in",
                    next: "check LOGBREW_API_URL and your network, then retry logbrew login",
                });
            }
            Poll::Denied => {
                return Err(RuntimeError::Unavailable {
                    message: "the sign-in was denied",
                    next: "run logbrew login again and approve the sign-in",
                });
            }
            Poll::Expired => return Err(expired()),
            Poll::Failed => return Err(login_unavailable("device sign-in could not be completed")),
        }
    }
}

/// Sends one token request and classifies the answer.
async fn poll(client: &reqwest::Client, origin: &str, device_code: &str) -> Poll {
    let request = client
        .post(format!("{origin}/api/auth/device/token"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form(&[
            ("grant_type", GRANT_TYPE),
            ("device_code", device_code),
            ("client_id", CLIENT_ID),
        ]));
    let Ok(response) = request.send().await else {
        return Poll::Unreachable;
    };
    let status = response.status().as_u16();
    match crate::http::bounded_body(response, BODY_LIMIT).await {
        Ok(body) => classify(status, body.as_str()),
        Err(crate::http::BodyError::Transport) => Poll::Unreachable,
        Err(crate::http::BodyError::Invalid) => Poll::Failed,
    }
}

/// Maps a token response to its RFC 8628 meaning.
fn classify(status: u16, body: &str) -> Poll {
    let value = serde_json::from_str::<serde_json::Value>(body).ok();
    match status {
        200 => value
            .filter(serde_json::Value::is_object)
            .map_or(Poll::Failed, Poll::Approved),
        429 => Poll::SlowDown,
        500..=599 => Poll::Unreachable,
        _ => match value
            .as_ref()
            .and_then(|value| value.get("error")?.as_str())
        {
            Some("authorization_pending") => Poll::Pending,
            Some("slow_down") => Poll::SlowDown,
            Some("access_denied") => Poll::Denied,
            Some("expired_token") => Poll::Expired,
            Some(_) | None => Poll::Failed,
        },
    }
}

/// Encodes form fields as `application/x-www-form-urlencoded`.
fn form(fields: &[(&str, &str)]) -> String {
    reqwest::Url::parse("http://localhost/")
        .map(|mut url| {
            let _pairs = url.query_pairs_mut().extend_pairs(fields);
            url.query().unwrap_or_default().to_owned()
        })
        .unwrap_or_default()
}

/// The codes expired before anyone approved them.
const fn expired() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "the sign-in code expired",
        next: "run logbrew login again and approve the sign-in before the code expires",
    }
}

#[cfg(test)]
mod tests {
    use super::{Poll, classify, form, parse_authorization};

    #[test]
    fn token_answers_map_to_rfc_8628_meanings() {
        let error = |code: &str| format!("{{\"error\":\"{code}\"}}");
        for (status, body, expected) in [
            (400, error("authorization_pending"), Poll::Pending),
            (400, error("slow_down"), Poll::SlowDown),
            (400, error("access_denied"), Poll::Denied),
            (400, error("expired_token"), Poll::Expired),
            (400, error("invalid_grant"), Poll::Failed),
            (400, String::from("not json"), Poll::Failed),
            (429, String::new(), Poll::SlowDown),
            (503, error("temporarily_unavailable"), Poll::Unreachable),
            (200, String::from("[]"), Poll::Failed),
            (
                200,
                String::from("{\"access_token\":\"a\"}"),
                Poll::Approved(serde_json::json!({"access_token": "a"})),
            ),
        ] {
            assert_eq!(classify(status, body.as_str()), expected, "{status} {body}");
        }
    }

    #[test]
    fn authorization_fails_closed_on_unsafe_or_missing_fields() -> Result<(), &'static str> {
        let valid = serde_json::json!({
            "device_code": "secret",
            "user_code": "BCDF-GHJK",
            "verification_uri": "https://logbrew.example/device",
            "verification_uri_complete": "https://logbrew.example/device?user_code=BCDF-GHJK",
            "expires_in": 600,
            "interval": 5,
        });
        let changed = |name: &str, value: Option<serde_json::Value>| {
            let mut changed = valid.clone();
            if let Some(fields) = changed.as_object_mut() {
                match value {
                    Some(value) => drop(fields.insert(name.to_owned(), value)),
                    None => drop(fields.remove(name)),
                }
            }
            parse_authorization(changed.to_string().as_str())
        };
        let parsed = parse_authorization(valid.to_string().as_str()).ok_or("valid")?;
        assert_eq!(
            (parsed.link.as_str(), parsed.expires_in, parsed.interval),
            ("https://logbrew.example/device?user_code=BCDF-GHJK", 600, 5),
            "the complete link wins"
        );
        let plain = changed("verification_uri_complete", None).ok_or("plain")?;
        assert_eq!(plain.link, "https://logbrew.example/device", "plain link");
        let default = changed("interval", None).ok_or("default")?;
        assert_eq!(default.interval, 5, "default interval");
        for (name, value) in [
            ("device_code", None),
            ("device_code", Some("has space".into())),
            ("user_code", Some("BCDF\u{1b}[2J".into())),
            (
                "verification_uri_complete",
                Some("javascript:alert(1)".into()),
            ),
            (
                "verification_uri_complete",
                Some("https://user:pass@logbrew.example/device".into()),
            ),
            (
                "verification_uri_complete",
                Some("https://logbrew.example/a b".into()),
            ),
            ("expires_in", Some(0.into())),
            ("expires_in", Some("600".into())),
            ("interval", Some(0.into())),
            ("interval", Some(61.into())),
        ] {
            assert!(changed(name, value.clone()).is_none(), "{name} {value:?}");
        }
        Ok(())
    }

    #[test]
    fn form_fields_are_url_encoded() {
        assert_eq!(
            form(&[("grant_type", "urn:a:b"), ("client_name", "LogBrew CLI")]),
            "grant_type=urn%3Aa%3Ab&client_name=LogBrew+CLI",
            "form encoding"
        );
    }
}
