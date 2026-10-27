//! Plan changes: a checkout link to upgrade and a billing management link.
//!
//! Payment stays with the account owner. Both commands return one short-lived
//! Stripe link for the owner to open in a browser.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the parent command executor consumes these private-module helpers"
)]

use crate::auth::{AuthCredential, send_account_authenticated_with_refresh};
use crate::{BillingTier, CliEnvironment, RuntimeError};

/// Maximum accepted session response body.
const RESPONSE_LIMIT: usize = 16 * 1024;
/// Maximum accepted link length.
const URL_LIMIT: usize = 2048;

/// Exact hosted-session response.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    /// Short-lived Stripe link.
    url: String,
}

/// Requests a checkout link for `tier`, or a billing management link when `tier` is `None`.
pub(crate) async fn execute<W: std::io::Write>(
    env: &CliEnvironment,
    tier: Option<BillingTier>,
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let origin = crate::http::normalized_origin(env.base_url.as_str()).ok_or_else(transport)?;
    let client = crate::http::api_client().map_err(|_| transport())?;
    let (url, body) = match tier {
        Some(tier) => (
            format!("{origin}/api/billing/checkout"),
            Some(serde_json::json!({
                "tier": tier.api_value(),
                "locale": "en",
                "request_id": request_id()?,
            })),
        ),
        None => (format!("{origin}/api/billing/portal"), None),
    };
    let (response, credential) =
        send_account_authenticated_with_refresh(&client, env, |client, credential| {
            let request = client.post(url.as_str()).bearer_auth(credential.token());
            match &body {
                Some(body) => request.json(body),
                None => request,
            }
        })
        .await
        .map_err(|error| error.auth_or(transport()))?;
    let status = response.status().as_u16();
    let text = crate::http::bounded_body(response, RESPONSE_LIMIT)
        .await
        .map_err(|error| match error {
            crate::http::BodyError::Invalid => invalid(),
            crate::http::BodyError::Transport => transport(),
        })?;
    if status != 200 {
        return Err(api_error(status, text.as_str(), tier, &credential));
    }
    let session = serde_json::from_str::<Session>(text.as_str()).map_err(|_| invalid())?;
    if !stripe_link(session.url.as_str()) {
        return Err(invalid());
    }
    let (what, next) = tier.map_or_else(
        || {
            (
                String::from("Billing management link"),
                "open the link in a browser to change the plan, payment method, or invoices",
            )
        },
        |tier| {
            (
                format!("Checkout link for {}", tier.label()),
                "open the link in a browser to pay, then run logbrew usage",
            )
        },
    );
    if json {
        let receipt = serde_json::json!({
            "ok": true,
            "url": session.url,
            "tier": tier.map(BillingTier::label_lowercase),
            "next": next,
        });
        writeln!(output, "{receipt}")?;
    } else {
        writeln!(output, "{what}: {}", session.url)?;
        writeln!(
            output,
            "The link is short-lived. Only the account owner can pay."
        )?;
        writeln!(output, "Next: {next}")?;
    }
    Ok(())
}

/// Whether `value` is an HTTPS link on a Stripe host, safe to print.
fn stripe_link(value: &str) -> bool {
    value.len() <= URL_LIMIT
        && crate::http::nonempty_display_safe(value, URL_LIMIT)
        && reqwest::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url
                    .host_str()
                    .is_some_and(|host| host == "stripe.com" || host.ends_with(".stripe.com"))
        })
}

/// A random version 4 UUID for the checkout retry identity.
fn request_id() -> Result<String, RuntimeError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| transport())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes
        .iter()
        .fold(String::with_capacity(32), |mut hex, byte| {
            use std::fmt::Write as _;
            let _written = write!(hex, "{byte:02x}");
            hex
        });
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

/// Validates a standard error envelope, then replaces all server text.
fn api_error(
    status: u16,
    body: &str,
    tier: Option<BillingTier>,
    credential: &AuthCredential,
) -> RuntimeError {
    let Some(shape) = crate::http::ErrorShape::parse(body, status, 512, |text, limit| {
        text.len() <= limit && crate::http::nonempty_display_safe(text, usize::MAX)
    }) else {
        return invalid();
    };
    let action = shape.action();
    let refused = status == 422 && action == ("validation_failed", "fix_request", "request");
    let closed = status == 503 && action == ("billing_unavailable", "contact_support", "support");
    let retry =
        status == 503 && action == ("billing_unavailable", "connect_or_retry_billing", "billing");
    if !refused && !closed && !retry && !shape.matches_read_status(status) {
        return invalid();
    }
    let body = shape.rate_limit_body("billing").unwrap_or_else(|| {
        let (code, next) = match (status, tier) {
            (401, _) => ("unauthorized", "run logbrew login"),
            (422, Some(_)) => (
                "subscription_exists",
                "the account already has a paid plan; run logbrew billing portal to change it",
            ),
            (422, None) => (
                "no_subscription",
                "the account has no paid plan; run logbrew billing upgrade pro or business",
            ),
            (503, _) if closed => (
                "checkout_closed",
                "self-serve checkout is closed; email sales@logbrew.co to change the plan",
            ),
            (405, _) => ("method_not_allowed", "update logbrew and retry"),
            _ => ("server_error", "retry the same command later"),
        };
        serde_json::json!({"error": "billing request failed", "code": code, "next": next})
            .to_string()
    });
    RuntimeError::Api {
        status,
        body,
        auth_source: credential.source(),
        auth_label: credential.label(),
    }
}

/// Fixed recovery for a malformed response.
const fn invalid() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "billing response was invalid",
        next: "retry the same command; if it repeats, report the public response contract",
    }
}

/// Fixed recovery for a failed request, without a URL.
const fn transport() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "billing request could not be completed",
        next: "check network connectivity and retry the same command",
    }
}
