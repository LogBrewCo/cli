//! Live watch parse recovery tests.

use super::{TestResult, assert_human_error, assert_json_error};
use logbrew_cli::parse_command;

#[test]
fn recovers_watch_positionals_to_historical_reads() -> TestResult {
    assert_json_error(
        &["logbrew", "watch", "actions", "checkout_failed", "--json"],
        "unexpected_argument",
        "unexpected argument for watch: checkout_failed",
        "use logbrew actions --name <name> for historical data, or logbrew watch actions --json \
         for live actions",
    )?;
    assert_human_error(
        &["logbrew", "tail", "logs", "error"],
        "unexpected argument for watch: error",
        "use logbrew logs --severity <severity> or --search <text> for historical data, or \
         logbrew watch logs --json for live logs",
    )
}

#[test]
fn recovers_watch_read_filters_to_historical_reads() -> TestResult {
    for (args, message) in [
        (
            &[
                "logbrew",
                "stream",
                "actions",
                "--name",
                "checkout_failed",
                "--json",
            ][..],
            "unsupported flag for watch: --name",
        ),
        (
            &[
                "logbrew",
                "watch",
                "actions",
                "--service",
                "checkout-api",
                "--json",
            ][..],
            "unsupported flag for watch: --service",
        ),
    ] {
        assert_json_error(
            args,
            "unsupported_flag",
            message,
            "use logbrew actions with filters for historical data, or logbrew watch actions --json \
             for live actions",
        )?;
    }
    assert_json_error(
        &["logbrew", "watch", "issues", "--since", "24h", "--json"],
        "unsupported_flag",
        "unsupported flag for watch: --since",
        "use logbrew issues with filters for historical data, or logbrew watch issues --severity \
         <severity> --json for live severity filtering",
    )?;
    assert_human_error(
        &["logbrew", "watch", "logs", "--search", "checkout"],
        "unsupported flag for watch: --search",
        "use logbrew logs with filters for historical data, or logbrew watch logs --severity \
         <severity> --json for live severity filtering",
    )?;
    for (flag, severity) in [("--level", "error"), ("--severity", "warning")] {
        drop(parse_command([
            "logbrew", "watch", "logs", flag, severity, "--json",
        ])?);
    }
    Ok(())
}
