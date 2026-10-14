//! Complete public CLI contract.

#[macro_use]
mod async_test;
pub(crate) use async_test::run_async;

#[path = "integration/mock_http.rs"]
mod mock_http;
#[path = "integration/support.rs"]
mod support;

pub(crate) use mock_http::{Mock, MockServer, Request, ResponseTemplate, retry_then};
pub(crate) use support::*;

pub(crate) mod matchers {
    pub(crate) use super::mock_http::matchers::*;
}

mod grammar;
mod integration;
mod investigation;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn assert_command(args: &[&str], expected: logbrew_cli::Command) {
    assert_eq!(
        logbrew_cli::parse_command(args.iter().copied()),
        Ok(expected),
        "unexpected command for {args:?}"
    );
}

fn assert_help(args: &[&str], topic: logbrew_cli::HelpTopic, json: bool) {
    assert_command(args, logbrew_cli::Command::Help { topic, json });
}

fn render_error(args: &[&str], json: bool) -> TestResult<String> {
    let error = logbrew_cli::parse_command(args.iter().copied())
        .err()
        .ok_or("command must fail closed")?;
    let mut output = Vec::new();
    logbrew_cli::write_cli_error(&error, json, &mut output)?;
    Ok(String::from_utf8(output)?)
}

fn rendered_json_error(args: &[&str]) -> TestResult<(serde_json::Value, String)> {
    let text = render_error(args, true)?;
    Ok((serde_json::from_str(&text)?, text))
}
