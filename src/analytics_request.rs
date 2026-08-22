//! Shared request, response, and exact-body helpers for product analytics.

#![expect(
    clippy::missing_docs_in_private_items,
    reason = "private transport policy names mirror their documented analytics owners"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "private sibling analytics modules share this canonical transport policy"
)]

use crate::auth::{AuthCredential, send_authenticated_with_refresh};
use crate::{CliEnvironment, RuntimeError};

/// Analytics operation whose stable transport and recovery policy is required.
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Paths,
    Funnel,
    Segments,
    Lifecycle,
    Retention,
    Properties,
    Overview,
}

impl Kind {
    const fn text(self) -> ErrorText {
        match self {
            Self::Paths => ErrorText::new(
                "analytics path",
                "analytics path query",
                "check the exact project, time scope, direction, anchor, property predicates, depth, and path limit",
                "use the POST-backed logbrew analytics paths command",
                true,
            ),
            Self::Funnel => ErrorText::new(
                "analytics funnel",
                "analytics funnel query",
                "check the exact project, time scope, ordered steps, unit, and conversion window",
                "use the POST-backed logbrew analytics funnel command",
                true,
            ),
            Self::Segments => ErrorText::new(
                "analytics segment comparison",
                "analytics segment comparison",
                "check the exact project, time scope, target, segments, property filters, interval, and analysis unit",
                "use the POST-backed logbrew analytics compare command",
                false,
            ),
            Self::Lifecycle => ErrorText::new(
                "analytics lifecycle",
                "analytics lifecycle query",
                "check the exact project, time scope, event selector, interval, and history periods",
                "use the POST-backed logbrew analytics lifecycle command",
                true,
            ),
            Self::Retention => ErrorText::new(
                "analytics retention",
                "analytics retention query",
                "check the exact project, time scope, event selectors, interval, count, mode, and cohort mode",
                "use the POST-backed logbrew analytics retention command",
                true,
            ),
            Self::Properties => ErrorText::new(
                "analytics property",
                "analytics property query",
                "check the exact project, time scope, deployment filters, and limit",
                "use the GET-backed logbrew analytics properties command",
                true,
            ),
            Self::Overview => ErrorText::new(
                "analytics overview",
                "analytics overview query",
                "check the exact project, time scope, interval, context filters, and top limit",
                "use the GET-backed logbrew analytics overview command",
                true,
            ),
        }
    }

    const fn transport(self) -> RuntimeError {
        let (message, next) = match self {
            Self::Paths => (
                "analytics path request could not be completed",
                "check network connectivity and retry the same analytics path query",
            ),
            Self::Funnel => (
                "analytics funnel request could not be completed",
                "check network connectivity and retry the same analytics funnel query",
            ),
            Self::Segments => (
                "analytics segment comparison request could not be completed",
                "check network connectivity and retry the same analytics segment comparison",
            ),
            Self::Lifecycle => (
                "analytics lifecycle request could not be completed",
                "check network connectivity and retry the same analytics lifecycle query",
            ),
            Self::Retention => (
                "analytics retention request could not be completed",
                "check network connectivity and retry the same analytics retention query",
            ),
            Self::Properties => (
                "analytics property request could not be completed",
                "check network connectivity and retry the same analytics property query",
            ),
            Self::Overview => (
                "analytics overview request could not be completed",
                "check network connectivity and retry the same analytics overview query",
            ),
        };
        RuntimeError::Unavailable { message, next }
    }

    pub(crate) const fn invalid(self) -> RuntimeError {
        match self {
            Self::Paths => RuntimeError::AnalyticsResponseInvalid,
            Self::Funnel => RuntimeError::AnalyticsFunnelResponseInvalid,
            Self::Segments => RuntimeError::AnalyticsSegmentResponseInvalid,
            Self::Lifecycle => RuntimeError::AnalyticsLifecycleResponseInvalid,
            Self::Retention => RuntimeError::AnalyticsRetentionResponseInvalid,
            Self::Properties => RuntimeError::AnalyticsPropertiesResponseInvalid,
            Self::Overview => RuntimeError::AnalyticsOverviewResponseInvalid,
        }
    }

    const fn is_get(self) -> bool {
        matches!(self, Self::Properties | Self::Overview)
    }
}

/// Stable copy that preserves one analytics command's public recovery contract.
#[derive(Clone, Copy)]
struct ErrorText {
    noun: &'static str,
    operation: &'static str,
    validation_next: &'static str,
    method_next: &'static str,
    uses_request_word: bool,
}

impl ErrorText {
    const fn new(
        noun: &'static str,
        operation: &'static str,
        validation_next: &'static str,
        method_next: &'static str,
        uses_request_word: bool,
    ) -> Self {
        Self {
            noun,
            operation,
            validation_next,
            method_next,
            uses_request_word,
        }
    }
}

/// Sends one bounded analytics request through the canonical auth and HTTP path.
pub(crate) async fn send(
    env: &CliEnvironment,
    path: &str,
    kind: Kind,
    body: Option<&serde_json::Value>,
    limit: usize,
) -> Result<String, RuntimeError> {
    let origin =
        crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(|| kind.transport())?;
    let client = crate::http::api_client().map_err(|_| kind.transport())?;
    let url = format!("{origin}{path}");
    let response = send_authenticated_with_refresh(&client, env, |client, credential| {
        let request = if kind.is_get() {
            client.get(url.as_str())
        } else {
            client.post(url.as_str())
        }
        .bearer_auth(credential.token());
        if let Some(body) = body {
            request.json(body)
        } else {
            request
        }
    })
    .await
    .map_err(|error| error.auth_or(kind.transport()))?;
    let (response, credential) = response;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(api_error(status, &credential, kind.text()));
    }
    crate::http::bounded_body(response, limit)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => kind.invalid(),
            crate::http::BodyError::Transport => kind.transport(),
        })
}

/// Builds the existing value-safe HTTP error without repeating status policy.
fn api_error(status: u16, credential: &AuthCredential, text: ErrorText) -> RuntimeError {
    let request = if text.uses_request_word {
        " request"
    } else {
        ""
    };
    let (error, code, next) = match status {
        400 | 422 => (
            format!("{}{request} rejected", text.noun),
            "validation_failed",
            text.validation_next.to_owned(),
        ),
        401 => (
            String::from("authentication required"),
            "unauthorized",
            String::from("run logbrew login"),
        ),
        403 => (
            format!("{}{request} forbidden", text.noun),
            "forbidden",
            format!(
                "confirm account access and retry the same {}",
                text.operation
            ),
        ),
        404 => (
            format!("{} resource not found", text.noun),
            "not_found",
            format!("check the project and retry the same {}", text.operation),
        ),
        405 => (
            format!("{} method is not supported", text.noun),
            "method_not_allowed",
            text.method_next.to_owned(),
        ),
        429 => (
            format!("{}{request} rate limited", text.noun),
            "rate_limited",
            format!("retry the same {} later", text.operation),
        ),
        500..=599 => (
            format!("{} service unavailable", text.noun),
            "service_unavailable",
            format!("retry the same {} later", text.operation),
        ),
        _ => (
            format!("{}{request} failed", text.noun),
            "request_failed",
            format!("check account access and retry the same {}", text.operation),
        ),
    };
    RuntimeError::Api {
        status,
        body: serde_json::json!({"error": error, "code": code, "next": next}).to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

/// Adds one optional exact context filter without sending a null placeholder.
pub(crate) fn insert_optional(
    body: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        drop(body.insert(key.to_owned(), serde_json::Value::String(value.to_owned())));
    }
}

/// Checks the shared bounded product-event name contract.
pub(crate) fn valid_event_name(is_interaction: bool, value: &str) -> bool {
    crate::http::nonempty_control_safe(value, 256)
        && (!is_interaction
            || value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
                }))
}
