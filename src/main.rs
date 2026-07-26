//! Native `LogBrew` CLI binary entry point.

#![forbid(unsafe_code)]

use std::process::ExitCode;

/// Runs the CLI process.
#[tokio::main]
async fn main() -> ExitCode {
    let args = std::env::args().collect::<Vec<_>>();
    let wants_json = args.iter().any(|arg| arg == "--json");
    let native_debug_json = native_debug_json_requested(args.as_slice());
    let command = match logbrew_cli::parse_command(args) {
        Ok(command) => command,
        Err(error) => {
            if native_debug_json {
                let mut stdout = std::io::stdout();
                let _result = logbrew_cli::write_cli_error(&error, true, &mut stdout);
            } else {
                let mut stderr = std::io::stderr();
                let _result = logbrew_cli::write_cli_error(&error, wants_json, &mut stderr);
            }
            return ExitCode::from(2);
        }
    };

    let env = logbrew_cli::CliEnvironment::from_process();
    let mut stdout = std::io::stdout();
    if let Err(error) = logbrew_cli::execute_command(&command, &env, &mut stdout).await {
        if matches!(
            command,
            logbrew_cli::Command::NativeDebugArtifacts { json: true, .. }
        ) {
            let _result = logbrew_cli::write_native_debug_runtime_error(&error, &mut stdout);
        } else {
            let mut stderr = std::io::stderr();
            let _result =
                logbrew_cli::write_runtime_error(&error, command.wants_json(), &mut stderr);
        }
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

/// Detects JSON-mode native debug commands even when parsing fails.
fn native_debug_json_requested(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == "--json")
        && args
            .iter()
            .skip(1)
            .find(|arg| arg.as_str() != "--json")
            .is_some_and(|command| command == "debug-artifacts")
}
