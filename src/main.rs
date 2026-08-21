//! Native `LogBrew` CLI binary entry point.

#![forbid(unsafe_code)]

/// Runs the CLI process.
#[tokio::main]
async fn main() -> std::process::ExitCode {
    logbrew_cli::run_process().await
}
