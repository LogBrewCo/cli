//! CLI issue shortcut recovery tests.

use logbrew_cli::{parse_command, write_cli_error};

#[test]
fn rejects_bare_issue_status_words_with_target_next_step() {
    for (args, next) in [
        (
            &["logbrew", "open", "--json"][..],
            "use logbrew open <issue_id> or logbrew open issues",
        ),
        (
            &["logbrew", "closed", "--json"][..],
            "use logbrew closed <issue_id> or logbrew closed issues",
        ),
        (
            &["logbrew", "ignored", "--json"][..],
            "use logbrew ignored <issue_id> or logbrew ignored issues",
        ),
    ] {
        let error = parse_command(args.iter().copied()).expect_err("bare status needs target");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "missing_argument");
        assert_eq!(body["message"], "missing argument: issue_id_or_issues");
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_issue_mutation_shortcuts_without_ids_with_command_next_step() {
    for (args, next) in [
        (
            &["logbrew", "resolve", "--json"][..],
            "use logbrew resolve <issue_id>",
        ),
        (
            &["logbrew", "close", "--json"][..],
            "use logbrew close <issue_id>",
        ),
        (
            &["logbrew", "ignore", "--json"][..],
            "use logbrew ignore <issue_id>",
        ),
        (
            &["logbrew", "reopen", "--json"][..],
            "use logbrew reopen <issue_id>",
        ),
    ] {
        let error = parse_command(args.iter().copied()).expect_err("shortcut needs issue id");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "missing_argument");
        assert_eq!(body["message"], "missing argument: issue_id");
        assert_eq!(body["next"], next);
    }
}

#[test]
fn issue_first_mutation_flags_preserve_shortcut_recovery() {
    for (args, error, message, next) in [
        (
            &[
                "logbrew",
                "issue",
                "issue_123",
                "resolve",
                "--release",
                "api@1",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for resolve: --release",
            "run logbrew resolve --help",
        ),
        (
            &[
                "logbrew",
                "issue",
                "issue_123",
                "ignored",
                "--auto",
                "--json",
            ][..],
            "unsupported_flag",
            "unsupported flag for ignored: --auto",
            "run logbrew ignored --help",
        ),
    ] {
        let error_value = parse_command(args.iter().copied()).expect_err("bad issue-first flag");
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
