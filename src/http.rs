//! Shared bounded HTTP response and origin validation.
#![expect(
    clippy::redundant_pub_crate,
    reason = "sibling private command modules consume these shared helpers"
)]

/// Failure category while reading an authenticated response.
pub(crate) enum BodyError {
    /// The server response violated a bounded body contract.
    Invalid,
    /// The response stream failed before completion.
    Transport,
}

/// Duplicate-aware exact standard API error envelope.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ErrorShape {
    /// Human-readable error field.
    error: String,
    /// Stable error-code field.
    code: String,
    /// Recovery guidance field.
    next: String,
    /// Typed recovery-action field.
    next_action: ErrorActionShape,
    /// A present retry delay must be a positive whole number, never null.
    #[serde(default, deserialize_with = "present_retry_delay")]
    retry_after_seconds: Option<std::num::NonZeroU64>,
}

/// Duplicate-aware exact standard API recovery action.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ErrorActionShape {
    /// Stable action-code field.
    code: String,
    /// Stable action-target field.
    target: String,
}

/// Distinguishes an absent retry delay from a present invalid or null value.
fn present_retry_delay<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<std::num::NonZeroU64>, D::Error> {
    <std::num::NonZeroU64 as serde::Deserialize>::deserialize(deserializer).map(Some)
}

impl ErrorShape {
    /// Parses exact fields with the command's existing text bounds and policy.
    pub(crate) fn parse(
        body: &str,
        status: u16,
        error_limit: usize,
        valid_text: fn(&str, usize) -> bool,
    ) -> Option<Self> {
        let value: Self = serde_json::from_str(body).ok()?;
        ((status == 429) == value.retry_after_seconds.is_some()
            && [
                (value.error.as_str(), error_limit),
                (value.next.as_str(), 512),
                (value.code.as_str(), 64),
                (value.next_action.code.as_str(), 64),
                (value.next_action.target.as_str(), 64),
            ]
            .into_iter()
            .all(|(text, limit)| valid_text(text, limit)))
        .then_some(value)
    }

    /// Returns the complete error and recovery identity for exact matching.
    pub(crate) fn action(&self) -> (&str, &str, &str) {
        (&self.code, &self.next_action.code, &self.next_action.target)
    }

    /// Matches shared read failures without granting account-specific recovery.
    pub(crate) fn matches_read_status(&self, status: u16) -> bool {
        match status {
            401 => self.action() == ("unauthorized", "sign_in", "auth"),
            405 => self.action() == ("method_not_allowed", "use_supported_method", "api_method"),
            429 => self.action() == ("rate_limited", "retry_with_backoff", "request"),
            500..=599 => server_error_action_matches(self.action()),
            _ => false,
        }
    }

    /// Preserves the validated delay and replaces remote prose with local guidance.
    pub(crate) fn rate_limit_body(&self, command: &str) -> Option<String> {
        self.retry_after_seconds.map(|delay| {
            serde_json::json!({
                "error": "request is rate limited",
                "code": "rate_limited",
                "next": format!("wait at least {delay} seconds, then retry logbrew {command} with exponential backoff and jitter"),
                "next_action": {"code": "retry_with_backoff", "target": "request"},
                "retry_after_seconds": delay.get(),
            })
            .to_string()
        })
    }
}

/// Matches the exact recovery action associated with a generic server failure.
pub(crate) fn server_error_action_matches(fields: (&str, &str, &str)) -> bool {
    matches!(
        fields,
        ("storage_error", "retry_or_check_storage", "backend_status")
            | ("json_error", "send_valid_json", "request")
            | ("internal_error", "retry_or_contact_support", "support")
    )
}

/// Returns a client builder after installing the process-wide TLS provider.
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    drop(rustls::crypto::ring::default_provider().install_default());
    reqwest::Client::builder()
}

/// Builds the bounded redirect-refusing client used by API commands.
pub(crate) fn api_client() -> Result<reqwest::Client, reqwest::Error> {
    client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
}

/// Normalizes an HTTP(S) API origin without credentials, path, query, or fragment.
pub(crate) fn normalized_origin(value: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    Some(parsed.to_string().trim_end_matches('/').to_owned())
}

/// Reads UTF-8 response content without retaining a body beyond `limit` bytes.
pub(crate) async fn bounded_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<String, BodyError> {
    if response
        .content_length()
        .is_some_and(|length| usize::try_from(length).map_or(true, |length| length > limit))
    {
        return Err(BodyError::Invalid);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| BodyError::Transport)? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(BodyError::Invalid);
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| BodyError::Invalid)
}

/// Rejects controls and display-direction characters in bounded server text.
pub(crate) fn display_safe(value: &str, limit: usize) -> bool {
    value.chars().count() <= limit
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '\u{061c}'
                        | '\u{200b}'..='\u{200f}'
                        | '\u{2028}'..='\u{202e}'
                        | '\u{2060}'..='\u{206f}'
                        | '\u{feff}'
                        | '\u{fff9}'..='\u{fffb}'
                )
        })
}

/// Rejects empty or display-unsafe bounded server text.
pub(crate) fn nonempty_display_safe(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && display_safe(value, limit)
}

/// Rejects empty, oversized, or control-bearing server text before escaping.
pub(crate) fn nonempty_control_safe(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}

/// Escapes terminal controls and bidirectional-display characters.
pub(crate) fn terminal_safe(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            output.extend(character.escape_default());
        } else if matches!(
            character,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
        ) {
            output.extend(character.escape_unicode());
        } else {
            output.push(character);
        }
    }
    output
}
