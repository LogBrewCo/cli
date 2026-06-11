//! Reserved watch parse recovery tests.

use logbrew_cli::{parse_command, write_cli_error};

#[test]
fn recovers_watch_positionals_to_historical_reads() {
    let action_error = parse_command(["logbrew", "watch", "events", "checkout_failed", "--json"])
        .expect_err("reserved watch action name fails");
    let mut action_output = Vec::new();

    write_cli_error(&action_error, true, &mut action_output).expect("error writes");

    let action_body: serde_json::Value =
        serde_json::from_slice(action_output.as_slice()).expect("valid json");
    assert_eq!(action_body["ok"], false);
    assert_eq!(action_body["error"], "unexpected_argument");
    assert_eq!(
        action_body["message"],
        "unexpected argument for watch: checkout_failed"
    );
    assert_eq!(
        action_body["next"],
        "use logbrew actions --name <name> for historical data until live watch is available"
    );

    let log_error = parse_command(["logbrew", "tail", "logs", "error"])
        .expect_err("reserved tail search fails");
    let mut log_output = Vec::new();

    write_cli_error(&log_error, false, &mut log_output).expect("error writes");

    let log_text = String::from_utf8(log_output).expect("utf8 output");
    assert_eq!(
        log_text,
        "unexpected argument for watch: error\nNext: use logbrew logs --severity <severity> or \
         --search <text> for historical data until live watch is available\n"
    );
}

#[test]
fn recovers_watch_read_filters_to_historical_reads() {
    let action_error = parse_command([
        "logbrew",
        "stream",
        "actions",
        "--name",
        "checkout_failed",
        "--json",
    ])
    .expect_err("reserved watch action filter fails");
    let mut action_output = Vec::new();

    write_cli_error(&action_error, true, &mut action_output).expect("error writes");

    let action_body: serde_json::Value =
        serde_json::from_slice(action_output.as_slice()).expect("valid json");
    assert_eq!(action_body["ok"], false);
    assert_eq!(action_body["error"], "unsupported_flag");
    assert_eq!(action_body["message"], "unsupported flag for watch: --name");
    assert_eq!(
        action_body["next"],
        "use logbrew actions with filters for historical data until live watch is available"
    );

    let log_error = parse_command(["logbrew", "watch", "logs", "--level", "error"])
        .expect_err("reserved watch log filter fails");
    let mut log_output = Vec::new();

    write_cli_error(&log_error, false, &mut log_output).expect("error writes");

    let log_text = String::from_utf8(log_output).expect("utf8 output");
    assert_eq!(
        log_text,
        "unsupported flag for watch: --level\nNext: use logbrew logs with filters for historical \
         data until live watch is available\n"
    );

    let severity_error = parse_command(["logbrew", "watch", "logs", "--severity", "warning"])
        .expect_err("reserved watch severity filter fails");
    let mut severity_output = Vec::new();

    write_cli_error(&severity_error, false, &mut severity_output).expect("error writes");

    let severity_text = String::from_utf8(severity_output).expect("utf8 output");
    assert_eq!(
        severity_text,
        "unsupported flag for watch: --severity\nNext: use logbrew logs with filters for \
         historical data until live watch is available\n"
    );
}
