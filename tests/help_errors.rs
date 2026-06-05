//! CLI help error recovery tests.

use logbrew_cli::{parse_command, write_cli_error};

#[test]
fn rejects_unknown_flag_in_explicit_help() {
    let error =
        parse_command(["logbrew", "help", "logs", "--bogus", "--json"]).expect_err("bad help");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unknown_flag");
    assert_eq!(body["message"], "unknown flag: --bogus");
    assert_eq!(body["next"], "run logbrew --help");
}

#[test]
fn rejects_known_filter_flag_in_help_shortcut() {
    let error = parse_command(["logbrew", "logs", "--help", "--release", "api@1"])
        .expect_err("bad help shortcut");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(text, "unknown flag: --release\nNext: run logbrew --help\n");
}

#[test]
fn rejects_extra_argument_after_root_help() {
    let error =
        parse_command(["logbrew", "--help", "logs", "--json"]).expect_err("extra help argument");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unexpected_argument");
    assert_eq!(body["message"], "unexpected argument for help: logs");
    assert_eq!(body["next"], "run logbrew --help");
}

#[test]
fn rejects_extra_argument_in_help_shortcut() {
    let error = parse_command(["logbrew", "logs", "--help", "checkout@1"])
        .expect_err("extra help shortcut argument");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unexpected argument for help: checkout@1\nNext: run logbrew --help\n"
    );
}

#[test]
fn rejects_extra_argument_in_read_help_topic() {
    let error = parse_command(["logbrew", "help", "read", "logs", "extra", "--json"])
        .expect_err("extra read help argument");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unexpected_argument");
    assert_eq!(body["message"], "unexpected argument for help: extra");
    assert_eq!(body["next"], "run logbrew --help");
}

#[test]
fn rejects_extra_argument_in_subcommand_resource_help_topic() {
    let error = parse_command(["logbrew", "help", "explain", "trace", "trace_123", "extra"])
        .expect_err("extra explain help argument");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unexpected argument for help: extra\nNext: run logbrew --help\n"
    );
}

#[test]
fn rejects_actual_extra_argument_after_copied_detail_help_ids() {
    for args in [
        &["logbrew", "help", "trace", "trace_123", "extra"][..],
        &["logbrew", "help", "trace", "trace_123", "explain", "extra"],
        &["logbrew", "help", "issue", "issue_123", "extra"],
        &["logbrew", "help", "issue", "issue_123", "explain", "extra"],
        &["logbrew", "help", "read", "trace", "trace_123", "extra"],
        &["logbrew", "help", "show", "errors", "issue_123", "extra"],
    ] {
        let error = parse_command(args.iter().copied()).expect_err("extra copied id help arg");
        let mut output = Vec::new();

        write_cli_error(&error, false, &mut output).expect("error writes");

        let text = String::from_utf8(output).expect("utf8 output");
        assert_eq!(
            text,
            "unexpected argument for help: extra\nNext: run logbrew --help\n"
        );
    }
}

#[test]
fn rejects_actual_extra_argument_after_copied_explain_help_ids() {
    for args in [
        &["logbrew", "help", "issue_123", "explain", "extra"][..],
        &["logbrew", "help", "trace_123", "explain", "extra"],
        &["logbrew", "help", "explain", "issue_123", "extra"],
        &[
            "logbrew",
            "help",
            "explain",
            "4bf92f3577b34da6a3ce929d0e0e4736",
            "extra",
        ],
    ] {
        let error = parse_command(args.iter().copied()).expect_err("extra copied explain help arg");
        let mut output = Vec::new();

        write_cli_error(&error, false, &mut output).expect("error writes");

        let text = String::from_utf8(output).expect("utf8 output");
        assert_eq!(
            text,
            "unexpected argument for help: extra\nNext: run logbrew --help\n"
        );
    }
}
