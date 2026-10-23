//! Live JSON watch stream over the public WebSocket feed.

use futures_util::StreamExt as _;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};

use crate::auth::send_authenticated_with_refresh;
use crate::{CliEnvironment, RuntimeError, WatchOptions, WatchTarget, encode_component, http};

/// Initial delay before reconnecting a live watch stream.
const WATCH_RECONNECT_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
/// Maximum delay before reconnecting a live watch stream.
const WATCH_RECONNECT_MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(30);
/// Maximum jitter added to reconnect delays.
const WATCH_RECONNECT_JITTER_MAX_MILLIS: u64 = 250;

/// Executes the public live WebSocket watch flow.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes this private-module helper"
)]
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    target: WatchTarget,
    options: &WatchOptions,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if !json {
        return Err(RuntimeError::Unavailable {
            message: "watch streams JSON for agents",
            next: "run logbrew watch --json",
        });
    }

    let mut reconnect_backoff = WatchReconnectBackoff::default();
    loop {
        let ticket = match request_feed_ticket(env).await {
            Ok(ticket) => ticket,
            Err(error) if reconnect_backoff.connected_once() && !runtime_error_is_auth(&error) => {
                tokio::time::sleep(reconnect_backoff.next_delay()).await;
                continue;
            }
            Err(error) => return Err(error),
        };
        let live_url = feed_live_url(env.base_url.as_str(), ticket.as_str())?;
        let (mut websocket, _) = match connect_async(live_url.as_str()).await {
            Ok(connection) => connection,
            Err(error)
                if reconnect_backoff.connected_once() && !websocket_error_is_auth(&error) =>
            {
                tokio::time::sleep(reconnect_backoff.next_delay()).await;
                continue;
            }
            Err(error) => return Err(map_websocket_connect_error(error)),
        };
        reconnect_backoff.mark_connected();

        let mut emitted_before_disconnect = false;
        loop {
            let Some(message) = websocket.next().await else {
                break;
            };
            let message = match message {
                Ok(message) => message,
                Err(error) if websocket_error_is_auth(&error) => {
                    return Err(map_websocket_stream_error(error));
                }
                Err(_) => break,
            };
            match message {
                Message::Text(text) => {
                    let event = parse_live_event(text.as_str())?;
                    if watch_event_matches(target, options, &event) {
                        writeln!(output, "{event}")?;
                    }
                    emitted_before_disconnect = true;
                }
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                Message::Close(_) => return Ok(()),
            }
        }
        if emitted_before_disconnect {
            reconnect_backoff.reset();
        }
        tokio::time::sleep(reconnect_backoff.next_delay()).await;
    }
}

/// Reconnect state for long-running live watch streams.
#[derive(Debug, Default)]
struct WatchReconnectBackoff {
    /// Whether a live WebSocket connection has ever been established.
    connected_once: bool,
    /// Consecutive reconnect attempts since the last stable event.
    attempts: u32,
}

impl WatchReconnectBackoff {
    /// Returns whether the stream has connected at least once.
    const fn connected_once(&self) -> bool {
        self.connected_once
    }

    /// Records a successful WebSocket connection.
    const fn mark_connected(&mut self) {
        self.connected_once = true;
    }

    /// Resets retry delay after a stream successfully emits data.
    const fn reset(&mut self) {
        self.attempts = 0;
    }

    /// Returns the next capped exponential reconnect delay.
    fn next_delay(&mut self) -> std::time::Duration {
        let exponent = self.attempts.min(5);
        let multiplier = 1_u64 << exponent;
        self.attempts = self.attempts.saturating_add(1);
        let base = WATCH_RECONNECT_INITIAL_DELAY
            .as_secs()
            .saturating_mul(multiplier)
            .min(WATCH_RECONNECT_MAX_DELAY.as_secs());
        let delay = std::time::Duration::from_secs(base) + watch_reconnect_jitter();
        if delay > WATCH_RECONNECT_MAX_DELAY {
            WATCH_RECONNECT_MAX_DELAY
        } else {
            delay
        }
    }
}

/// Returns small jitter for reconnect delays without adding a random dependency.
fn watch_reconnect_jitter() -> std::time::Duration {
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return std::time::Duration::ZERO;
    };
    std::time::Duration::from_millis(
        u64::from(elapsed.subsec_millis()) % WATCH_RECONNECT_JITTER_MAX_MILLIS,
    )
}

/// Returns whether a runtime error should stop watch reconnect attempts.
const fn runtime_error_is_auth(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::MissingToken | RuntimeError::Api { status: 401, .. }
    )
}

/// Returns whether a WebSocket error is an auth failure.
fn websocket_error_is_auth(error: &WebSocketError) -> bool {
    matches!(error, WebSocketError::Http(response) if response.status().as_u16() == 401)
}

/// Requests a short-lived WebSocket feed ticket from the public API.
async fn request_feed_ticket(env: &CliEnvironment) -> Result<String, RuntimeError> {
    let url = format!("{}/api/feed/ticket", env.base_url.trim_end_matches('/'));
    let client = http::api_client()?;
    let (response, credential) =
        send_authenticated_with_refresh(&client, env, |client, credential| {
            client.post(url.as_str()).bearer_auth(credential.token())
        })
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(RuntimeError::Api {
            status: status.as_u16(),
            body: credential.redact_response_body(body.as_str()),
            auth_source: credential.source(),
            auth_label: credential.label(),
        });
    }

    let value = serde_json::from_str::<serde_json::Value>(body.as_str()).map_err(|_| {
        RuntimeError::Unavailable {
            message: "feed ticket response was not valid JSON",
            next: "retry logbrew watch or run logbrew status",
        }
    })?;
    value
        .get("ticket")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|ticket| !ticket.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(RuntimeError::Unavailable {
            message: "feed ticket response did not include a ticket",
            next: "retry logbrew watch or run logbrew status",
        })
}

/// Builds the WebSocket live feed URL without exposing the opaque ticket elsewhere.
fn feed_live_url(base_url: &str, ticket: &str) -> Result<String, RuntimeError> {
    let trimmed = base_url.trim_end_matches('/');
    let (scheme, rest) = websocket_base_parts(trimmed).ok_or(RuntimeError::Unavailable {
        message: "LOGBREW_API_URL must start with http:// or https://",
        next: "check LOGBREW_API_URL or run logbrew status",
    })?;
    Ok(format!(
        "{scheme}://{rest}/api/feed/live?ticket={}",
        encode_component(ticket)
    ))
}

/// Converts an HTTP API base URL into WebSocket scheme and authority/path base parts.
fn websocket_base_parts(base_url: &str) -> Option<(&'static str, &str)> {
    base_url
        .strip_prefix("https://")
        .map(|rest| ("wss", rest))
        .or_else(|| base_url.strip_prefix("http://").map(|rest| ("ws", rest)))
}

/// Parses one backend live event object.
fn parse_live_event(text: &str) -> Result<serde_json::Value, RuntimeError> {
    serde_json::from_str::<serde_json::Value>(text).map_err(|_| RuntimeError::Unavailable {
        message: "live watch event was not valid JSON",
        next: "retry logbrew watch or check LOGBREW_API_URL",
    })
}

/// Returns whether an event should be emitted for the requested watch target and filters.
fn watch_event_matches(
    target: WatchTarget,
    options: &WatchOptions,
    event: &serde_json::Value,
) -> bool {
    target_matches_event(target, event) && severity_matches(options, event)
}

/// Returns whether the event type belongs to the selected target.
fn target_matches_event(target: WatchTarget, event: &serde_json::Value) -> bool {
    let event_type = event
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match target {
        WatchTarget::All => true,
        WatchTarget::Logs => event_type == "native_log",
        WatchTarget::Issues => event_type == "native_issue",
        WatchTarget::Actions => event_type == "native_action",
    }
}

/// Applies client-side severity filters to log and issue events.
fn severity_matches(options: &WatchOptions, event: &serde_json::Value) -> bool {
    if options.severity.is_empty() {
        return true;
    }
    let Some(severity) = event
        .get("data")
        .and_then(|data| data.get("severity").or_else(|| data.get("level")))
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    options
        .severity
        .iter()
        .any(|allowed| allowed.as_str() == severity)
}

/// Maps a WebSocket connection failure to a token-safe runtime error.
fn map_websocket_connect_error(error: WebSocketError) -> RuntimeError {
    match error {
        WebSocketError::Http(response) if response.status().as_u16() == 401 => {
            RuntimeError::Unavailable {
                message: "live watch ticket was rejected",
                next: "run logbrew login",
            }
        }
        WebSocketError::Http(_) => RuntimeError::Unavailable {
            message: "live watch websocket upgrade failed",
            next: "retry logbrew watch or check LOGBREW_API_URL",
        },
        WebSocketError::ConnectionClosed
        | WebSocketError::AlreadyClosed
        | WebSocketError::Io(_)
        | WebSocketError::Tls(_)
        | WebSocketError::Capacity(_)
        | WebSocketError::Protocol(_)
        | WebSocketError::WriteBufferFull(_)
        | WebSocketError::Utf8(_)
        | WebSocketError::AttackAttempt
        | WebSocketError::Url(_)
        | WebSocketError::HttpFormat(_) => RuntimeError::Unavailable {
            message: "live watch websocket failed",
            next: "retry logbrew watch or check LOGBREW_API_URL",
        },
    }
}

/// Maps an established WebSocket stream failure to a token-safe runtime error.
fn map_websocket_stream_error(error: WebSocketError) -> RuntimeError {
    match error {
        WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed => {
            RuntimeError::Unavailable {
                message: "live watch websocket closed",
                next: "retry logbrew watch",
            }
        }
        WebSocketError::Http(response) if response.status().as_u16() == 401 => {
            RuntimeError::Unavailable {
                message: "live watch ticket was rejected",
                next: "run logbrew login",
            }
        }
        WebSocketError::Http(_)
        | WebSocketError::Io(_)
        | WebSocketError::Tls(_)
        | WebSocketError::Capacity(_)
        | WebSocketError::Protocol(_)
        | WebSocketError::WriteBufferFull(_)
        | WebSocketError::Utf8(_)
        | WebSocketError::AttackAttempt
        | WebSocketError::Url(_)
        | WebSocketError::HttpFormat(_) => RuntimeError::Unavailable {
            message: "live watch websocket failed",
            next: "retry logbrew watch or check LOGBREW_API_URL",
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        WATCH_RECONNECT_MAX_DELAY, WatchReconnectBackoff, feed_live_url, runtime_error_is_auth,
        watch_event_matches,
    };
    use crate::{RuntimeError, WatchOptions, WatchTarget};

    #[test]
    fn reconnect_delay_doubles_to_the_cap_and_resets() {
        let mut backoff = WatchReconnectBackoff::default();
        for base in [1, 2, 4, 8, 16, 30, 30, 30] {
            let delay = backoff.next_delay();
            let floor = Duration::from_secs(base);
            assert!(delay >= floor, "{delay:?} is below {floor:?}");
            assert!(
                delay <= WATCH_RECONNECT_MAX_DELAY,
                "{delay:?} exceeds the cap"
            );
            assert!(
                delay < floor + Duration::from_millis(250),
                "{delay:?} has more than 250 ms of jitter"
            );
        }
        backoff.reset();
        assert!(backoff.next_delay() < Duration::from_secs(2));
    }

    #[test]
    fn live_url_uses_websocket_schemes_and_encodes_the_ticket() -> Result<(), RuntimeError> {
        assert_eq!(
            feed_live_url("https://api.example.test/", "a b/c?d")?,
            "wss://api.example.test/api/feed/live?ticket=a%20b%2Fc%3Fd"
        );
        assert_eq!(
            feed_live_url("http://127.0.0.1:8080/base", "t-1_2.3~")?,
            "ws://127.0.0.1:8080/base/api/feed/live?ticket=t-1_2.3~"
        );
        for base in ["ftp://api.example.test", "api.example.test"] {
            assert!(feed_live_url(base, "ticket").is_err(), "accepted {base}");
        }
        Ok(())
    }

    #[test]
    fn events_match_target_type_and_severity() {
        let log = serde_json::json!({"type": "native_log", "data": {"level": "error"}});
        let issue = serde_json::json!({"type": "native_issue", "data": {"severity": "warning"}});
        let action = serde_json::json!({"type": "native_action", "data": {}});
        let any = WatchOptions::default();
        let errors = WatchOptions {
            severity: vec![String::from("error")],
        };
        for (target, event, options, expected) in [
            (WatchTarget::All, &action, &any, true),
            (WatchTarget::Logs, &log, &any, true),
            (WatchTarget::Logs, &issue, &any, false),
            (WatchTarget::Issues, &issue, &any, true),
            (WatchTarget::Actions, &action, &any, true),
            (WatchTarget::Actions, &log, &any, false),
            (WatchTarget::All, &log, &errors, true),
            (WatchTarget::All, &issue, &errors, false),
            (WatchTarget::All, &action, &errors, false),
        ] {
            assert_eq!(
                watch_event_matches(target, options, event),
                expected,
                "{target:?} {event} {options:?}"
            );
        }
    }

    #[test]
    fn only_missing_or_rejected_credentials_stop_reconnects() {
        let api = |status| RuntimeError::Api {
            status,
            body: String::new(),
            auth_source: "account_session",
            auth_label: "account session",
        };
        assert!(runtime_error_is_auth(&RuntimeError::MissingToken));
        assert!(runtime_error_is_auth(&api(401)));
        assert!(!runtime_error_is_auth(&api(403)));
        assert!(!runtime_error_is_auth(&api(503)));
    }
}
