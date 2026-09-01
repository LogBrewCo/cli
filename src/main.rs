//! Native `LogBrew` CLI binary entry point.

#![forbid(unsafe_code)]

/// Runs the CLI process.
fn main() -> std::process::ExitCode {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return std::process::ExitCode::FAILURE;
    };
    runtime.block_on(logbrew_cli::run_process())
}
