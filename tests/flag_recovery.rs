//! CLI flag recovery tests.

use logbrew_cli::{parse_command, write_cli_error};

#[test]
fn read_unknown_flags_point_to_resource_help() {
    for (args, next) in [
        (
            &["logbrew", "logs", "--sort", "desc", "--json"][..],
            "run logbrew read logs --help",
        ),
        (
            &["logbrew", "issues", "--sort", "desc", "--json"][..],
            "run logbrew read issues --help",
        ),
        (
            &["logbrew", "actions", "--sort", "desc", "--json"][..],
            "run logbrew read actions --help",
        ),
        (
            &["logbrew", "releases", "--sort", "desc", "--json"][..],
            "run logbrew read releases --help",
        ),
        (
            &["logbrew", "trace", "trace_123", "--sort", "desc", "--json"][..],
            "run logbrew read trace --help",
        ),
    ] {
        let error = parse_command(args.iter().copied()).expect_err("bad flag");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unknown_flag");
        assert_eq!(body["message"], "unknown flag: --sort");
        assert_eq!(body["next"], next);
    }
}

#[test]
fn command_unknown_flags_point_to_command_help() {
    for (args, next) in [
        (
            &["logbrew", "login", "--bogus", "--json"][..],
            "run logbrew login --help",
        ),
        (
            &["logbrew", "status", "--bogus", "--json"][..],
            "run logbrew status --help",
        ),
        (
            &["logbrew", "setup", "--bogus", "--json"][..],
            "run logbrew setup --help",
        ),
        (
            &["logbrew", "watch", "logs", "--bogus", "--json"][..],
            "run logbrew watch --help",
        ),
    ] {
        let error = parse_command(args.iter().copied()).expect_err("bad flag");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unknown_flag");
        assert_eq!(body["message"], "unknown flag: --bogus");
        assert_eq!(body["next"], next);
    }
}

#[test]
fn issue_status_shortcut_flags_point_to_shortcut_help() {
    for (args, error, message, next) in [
        (
            &["logbrew", "resolve", "issue_123", "--bogus", "--json"][..],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew resolve --help",
        ),
        (
            &["logbrew", "close", "issue_123", "--bogus", "--json"],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew close --help",
        ),
        (
            &["logbrew", "ignore", "issue_123", "--auto", "--json"][..],
            "unsupported_flag",
            "unsupported flag for ignore: --auto",
            "run logbrew ignore --help",
        ),
        (
            &[
                "logbrew",
                "reopen",
                "issue_123",
                "--release",
                "api@1",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for reopen: --release",
            "run logbrew reopen --help",
        ),
        (
            &["logbrew", "closed", "issue_123", "--bogus", "--json"][..],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew closed --help",
        ),
        (
            &[
                "logbrew",
                "open",
                "issue_123",
                "--release",
                "api@1",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for open: --release",
            "run logbrew open --help",
        ),
    ] {
        let error_value = parse_command(args.iter().copied()).expect_err("bad shortcut flag");
        let mut output = Vec::new();

        write_cli_error(&error_value, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], error);
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn bare_issue_status_flags_point_to_shortcut_help() {
    for (args, error, message, next) in [
        (
            &["logbrew", "open", "--bogus", "--json"][..],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew open --help",
        ),
        (
            &["logbrew", "closed", "--release", "api@1", "--json"][..],
            "unsupported_flag",
            "unsupported flag for closed: --release",
            "run logbrew closed --help",
        ),
    ] {
        let error_value = parse_command(args.iter().copied()).expect_err("bad status flag");
        let mut output = Vec::new();

        write_cli_error(&error_value, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], error);
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn auth_namespace_help_rejects_duplicate_json() {
    for args in [
        &["logbrew", "auth", "--json", "--json"][..],
        &["logbrew", "auth", "token", "--json", "--json"][..],
    ] {
        let error = parse_command(args.iter().copied()).expect_err("duplicate json fails");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "duplicate_flag");
        assert_eq!(body["message"], "duplicate flag: --json");
        assert_eq!(body["next"], "use --json once");
    }
}
