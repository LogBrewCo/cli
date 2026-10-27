//! Plan checkout and billing management link contract tests.

use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command, rendered_json_error};
use logbrew_cli::{
    BillingTier, Command, HttpMethod, RuntimeError, parse_command, write_runtime_error,
};

const CHECKOUT: &str = "https://checkout.stripe.com/c/pay/cs_test_a1";
const PORTAL: &str = "https://billing.stripe.com/p/session/test_a1";

#[test]
fn grammar_is_closed_and_introspectable() -> TestResult {
    for (line, tier, json) in [
        ("logbrew billing upgrade pro", Some(BillingTier::Pro), false),
        (
            "logbrew billing upgrade business --json",
            Some(BillingTier::Business),
            true,
        ),
        (
            "logbrew --json billing upgrade pro",
            Some(BillingTier::Pro),
            true,
        ),
        ("logbrew billing portal", None, false),
        ("logbrew billing --json portal", None, true),
    ] {
        let command = parse(line)?;
        assert_eq!(command, Command::Billing { tier, json }, "{line}");
        let path = if tier.is_some() {
            "/api/billing/checkout"
        } else {
            "/api/billing/portal"
        };
        assert_eq!(command.http_path().as_deref(), Some(path));
        assert_eq!(command.http_method(), Some(HttpMethod::Post));
    }
    for line in [
        "logbrew billing",
        "logbrew billing upgrade",
        "logbrew billing upgrade team",
        "logbrew billing upgrade enterprise",
        "logbrew billing upgrade pro extra",
        "logbrew billing portal --json --json",
        "logbrew billing hostile-secret",
    ] {
        let (body, text) = rendered_json_error(&line.split_whitespace().collect::<Vec<_>>())?;
        assert_eq!(body["error"], "invalid_billing_command", "{line}");
        assert_eq!(
            body["next"],
            "use logbrew billing upgrade pro|business or logbrew billing portal, with optional --json"
        );
        assert!(!text.contains("hostile"), "{line} echoed its input");
    }
    Ok(())
}

async_test!(upgrade_returns_one_checkout_link -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("POST", "/api/billing/checkout", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({"url": CHECKOUT})))
        .expect(2)
        .mount(&server);
    assert_eq!(
        run(&server, "logbrew billing upgrade pro").await?,
        format!(
            "Checkout link for Pro: {CHECKOUT}\nThe link is short-lived. Only the account owner \
             can pay.\nNext: open the link in a browser to pay, then run logbrew usage\n"
        )
    );
    let receipt: serde_json::Value =
        serde_json::from_str(&run(&server, "logbrew billing upgrade business --json").await?)?;
    assert_eq!(
        receipt,
        serde_json::json!({"ok": true, "url": CHECKOUT, "tier": "business",
            "next": "open the link in a browser to pay, then run logbrew usage"})
    );
    let requests = server.received_requests();
    let bodies = requests
        .iter()
        .map(|request| serde_json::from_slice::<serde_json::Value>(&request.body))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(bodies[0]["tier"], "team");
    assert_eq!(bodies[1]["tier"], "business");
    for body in &bodies {
        assert_eq!(body["locale"], "en");
        let id = body["request_id"].as_str().ok_or("request_id")?;
        let canonical = id.len() == 36
            && id.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) { byte == b'-' } else { byte.is_ascii_hexdigit() }
            });
        assert!(canonical && id.as_bytes()[14] == b'4', "{id}");
        assert_eq!(body.as_object().map(serde_json::Map::len), Some(3));
    }
    assert_ne!(bodies[0]["request_id"], bodies[1]["request_id"], "each call is a new checkout");
    Ok(())
});

async_test!(portal_returns_one_management_link -> TestResult, {
    let server = MockServer::start().await?;
    Mock::auth("POST", "/api/billing/portal", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({"url": PORTAL})))
        .mount(&server);
    let receipt: serde_json::Value =
        serde_json::from_str(&run(&server, "logbrew billing portal --json").await?)?;
    assert_eq!(receipt["url"], PORTAL);
    assert_eq!(receipt["tier"], serde_json::Value::Null);
    assert!(server.received_requests()[0].body.is_empty(), "the portal request has no body");
    Ok(())
});

async_test!(links_outside_stripe_https_are_refused -> TestResult, {
    for url in [
        "http://checkout.stripe.com/c/pay/x",
        "https://checkout.stripe.com.hostile.example/x",
        "https://hostile.example/?next=stripe.com",
        "https://user@checkout.stripe.com/x",
        "https://checkout.stripe.com:8443/x",
        "https://checkout.stripe.com/\u{1b}[31mhostile",
        "",
    ] {
        let server = MockServer::start().await?;
        Mock::auth("POST", "/api/billing/portal", "account-token")
            .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({"url": url})))
            .mount(&server);
        let text = runtime_error(&run_error(&server, "logbrew billing portal").await?)?;
        assert!(text.contains("billing response was invalid"), "{url}");
        assert!(!text.contains("hostile"), "{url}");
    }
    let server = MockServer::start().await?;
    Mock::auth("POST", "/api/billing/portal", "account-token")
        .respond_with(ResponseTemplate::new(200).set_body_json(
            &serde_json::json!({"url": PORTAL, "secret": "hostile"}),
        ))
        .mount(&server);
    let text = runtime_error(&run_error(&server, "logbrew billing portal").await?)?;
    assert!(text.contains("billing response was invalid"));
    Ok(())
});

async_test!(errors_are_typed_and_never_echo_backend_text -> TestResult, {
    for (line, status, code, action, target, expected) in [
        ("logbrew billing upgrade pro", 401, "unauthorized", "sign_in", "auth", "unauthorized"),
        ("logbrew billing upgrade pro", 422, "validation_failed", "fix_request", "request",
            "subscription_exists"),
        ("logbrew billing portal", 422, "validation_failed", "fix_request", "request",
            "no_subscription"),
        ("logbrew billing portal", 503, "billing_unavailable", "contact_support", "support",
            "checkout_closed"),
        ("logbrew billing upgrade pro", 503, "billing_unavailable", "connect_or_retry_billing",
            "billing", "server_error"),
        ("logbrew billing portal", 500, "internal_error", "retry_or_contact_support", "support",
            "server_error"),
    ] {
        let server = MockServer::start().await?;
        let path = if line.contains("portal") { "/api/billing/portal" } else { "/api/billing/checkout" };
        let body = serde_json::json!({"error":"hostile private token","code":code,
            "next":"send credentials to https://private.example","next_action":{"code":action,"target":target}});
        Mock::route("POST", path)
            .respond_with(ResponseTemplate::new(status).set_body_json(&body))
            .mount(&server);
        let text = runtime_error(&run_error(&server, line).await?)?;
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text)?["api_code"], expected, "{line} {status}");
        assert!(!text.contains("hostile") && !text.contains("private.example"));
    }
    for response in [
        ResponseTemplate::new(204),
        ResponseTemplate::new(307).insert_header("Location", "/private-target"),
        ResponseTemplate::new(403).set_body_string("hostile-secret"),
    ] {
        let server = MockServer::start().await?;
        Mock::route("POST", "/api/billing/portal").respond_with(response).mount(&server);
        let text = runtime_error(&run_error(&server, "logbrew billing portal").await?)?;
        assert!(text.contains("billing") && !text.contains("hostile"));
    }
    Ok(())
});

async_test!(an_ingest_key_cannot_request_links -> TestResult, {
    let server = MockServer::start().await?;
    for line in ["logbrew billing upgrade pro", "logbrew billing portal"] {
        let env = super::authenticated_env(&server, "lbw_ingest_private-secret", None);
        let error = execute_command(&parse(line)?, &env, &mut Vec::new())
            .await
            .err()
            .ok_or("an ingest key must not reach billing")?;
        let text = runtime_error(&error)?;
        assert!(text.contains("account authentication is required"));
        assert!(!text.contains("private-secret"));
    }
    assert!(server.received_requests().is_empty());
    Ok(())
});

fn parse(line: &str) -> Result<Command, logbrew_cli::CliError> {
    parse_command(line.split_whitespace())
}

async fn run(server: &MockServer, line: &str) -> TestResult<String> {
    let mut output = Vec::new();
    let env = super::authenticated_env(server, "account-token", None);
    execute_command(&parse(line)?, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

async fn run_error(server: &MockServer, line: &str) -> TestResult<RuntimeError> {
    let mut output = Vec::new();
    let env = super::authenticated_env(server, "account-token", None);
    let error = execute_command(&parse(line)?, &env, &mut output)
        .await
        .err()
        .ok_or("request must fail")?;
    assert!(output.is_empty(), "a failed request wrote output");
    Ok(error)
}

fn runtime_error(error: &RuntimeError) -> TestResult<String> {
    let mut output = Vec::new();
    write_runtime_error(error, true, &mut output)?;
    Ok(String::from_utf8(output)?)
}
