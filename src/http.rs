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
