//! Native `LogBrew` CLI binary entry point.

#![forbid(unsafe_code)]

use std::process::ExitCode;

/// Runs the CLI process.
#[tokio::main]
async fn main() -> ExitCode {
    let args = std::env::args().collect::<Vec<_>>();
    let wants_json = args.iter().any(|arg| arg == "--json");
    let command = match logbrew_cli::parse_command(args) {
        Ok(command) => command,
        Err(error) => {
            let mut stderr = std::io::stderr();
            let _result = logbrew_cli::write_cli_error(&error, wants_json, &mut stderr);
            return ExitCode::from(2);
        }
    };

    let env = logbrew_cli::CliEnvironment::from_process();
    let mut stdout = std::io::stdout();
    if let Err(error) = logbrew_cli::execute_command(&command, &env, &mut stdout).await {
        let mut stderr = std::io::stderr();
        let _result = logbrew_cli::write_runtime_error(&error, command.wants_json(), &mut stderr);
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
