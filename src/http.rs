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

/// Returns a client builder after installing the process-wide TLS provider.
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    drop(rustls::crypto::ring::default_provider().install_default());
    reqwest::Client::builder()
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
