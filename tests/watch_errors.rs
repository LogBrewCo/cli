//! Live watch parse recovery tests.

use logbrew_cli::{parse_command, write_cli_error};

#[test]
fn recovers_watch_positionals_to_historical_reads() {
    let action_error = parse_command(["logbrew", "watch", "actions", "checkout_failed", "--json"])
        .expect_err("watch action name fails");
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
        "use logbrew actions --name <name> for historical data, or logbrew watch actions --json \
         for live actions"
    );

    let log_error =
        parse_command(["logbrew", "tail", "logs", "error"]).expect_err("watch log search fails");
    let mut log_output = Vec::new();

    write_cli_error(&log_error, false, &mut log_output).expect("error writes");

    let log_text = String::from_utf8(log_output).expect("utf8 output");
    assert_eq!(
        log_text,
        "unexpected argument for watch: error\nNext: use logbrew logs --severity <severity> or \
         --search <text> for historical data, or logbrew watch logs --json for live logs\n"
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
    .expect_err("watch action filter fails");
    let mut action_output = Vec::new();

    write_cli_error(&action_error, true, &mut action_output).expect("error writes");

    let action_body: serde_json::Value =
        serde_json::from_slice(action_output.as_slice()).expect("valid json");
    assert_eq!(action_body["ok"], false);
    assert_eq!(action_body["error"], "unsupported_flag");
    assert_eq!(action_body["message"], "unsupported flag for watch: --name");
    assert_eq!(
        action_body["next"],
        "use logbrew actions with filters for historical data, or logbrew watch actions --json \
         for live actions"
    );

    let service_error = parse_command([
        "logbrew",
        "watch",
        "actions",
        "--service",
        "checkout-api",
        "--json",
    ])
    .expect_err("watch service filter fails");
    let mut service_output = Vec::new();

    write_cli_error(&service_error, true, &mut service_output).expect("error writes");

    let service_body: serde_json::Value =
        serde_json::from_slice(service_output.as_slice()).expect("valid json");
    assert_eq!(service_body["ok"], false);
    assert_eq!(service_body["error"], "unsupported_flag");
    assert_eq!(
        service_body["message"],
        "unsupported flag for watch: --service"
    );
    assert_eq!(
        service_body["next"],
        "use logbrew actions with filters for historical data, or logbrew watch actions --json \
         for live actions"
    );

    let issue_error = parse_command(["logbrew", "watch", "issues", "--since", "24h", "--json"])
        .expect_err("watch issue recency filter fails");
    let mut issue_output = Vec::new();

    write_cli_error(&issue_error, true, &mut issue_output).expect("error writes");

    let issue_body: serde_json::Value =
        serde_json::from_slice(issue_output.as_slice()).expect("valid json");
    assert_eq!(issue_body["ok"], false);
    assert_eq!(issue_body["error"], "unsupported_flag");
    assert_eq!(issue_body["message"], "unsupported flag for watch: --since");
    assert_eq!(
        issue_body["next"],
        "use logbrew issues with filters for historical data, or logbrew watch issues --severity \
         <severity> --json for live severity filtering"
    );

    let log_error = parse_command(["logbrew", "watch", "logs", "--search", "checkout"])
        .expect_err("watch log search filter fails");
    let mut log_output = Vec::new();

    write_cli_error(&log_error, false, &mut log_output).expect("error writes");

    let log_text = String::from_utf8(log_output).expect("utf8 output");
    assert_eq!(
        log_text,
        "unsupported flag for watch: --search\nNext: use logbrew logs with filters for historical \
         data, or logbrew watch logs --severity <severity> --json for live severity filtering\n"
    );

    drop(
        parse_command(["logbrew", "watch", "logs", "--level", "error", "--json"])
            .expect("watch level alias parses"),
    );
    drop(
        parse_command([
            "logbrew",
            "watch",
            "logs",
            "--severity",
            "warning",
            "--json",
        ])
        .expect("watch severity filter parses"),
    );
}
