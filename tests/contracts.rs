//! Complete public CLI contract.

#[macro_use]
mod async_test;

#[path = "integration/mock_http.rs"]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Private test helpers need crate visibility; unreachable_pub stays denied. Review on Clippy upgrade; checked 2026-09-09."
)]
mod mock_http;
#[path = "integration/support.rs"]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Private test helpers need crate visibility; unreachable_pub stays denied. Review on Clippy upgrade; checked 2026-09-09."
)]
mod support;

pub(crate) use mock_http::{Mock, MockServer, Request, ResponseTemplate, matchers, retry_then};
pub(crate) use support::*;

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

async fn failed_command(
    command: &logbrew_cli::Command,
    env: &logbrew_cli::CliEnvironment,
) -> TestResult<logbrew_cli::RuntimeError> {
    let mut output = Vec::new();
    let error = execute_command(command, env, &mut output)
        .await
        .err()
        .ok_or("command must fail")?;
    assert!(output.is_empty(), "failed command leaked output");
    Ok(error)
}

fn runtime_error_text(error: &logbrew_cli::RuntimeError) -> TestResult<String> {
    let mut output = Vec::new();
    logbrew_cli::write_runtime_error(error, true, &mut output)?;
    Ok(String::from_utf8(output)?)
}

/// Runs one boxed asynchronous test and propagates runtime-creation failures.
fn run_async<T>(future: std::pin::Pin<Box<dyn Future<Output = T>>>) -> Result<T, std::io::Error> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(future))
}
