//! CLI action shortcut filter recovery tests.

use logbrew_cli::{parse_command, write_cli_error};

#[test]
fn rejects_filter_words_before_action_name_shortcuts_with_specific_hints() {
    for (args, argument, expected_next) in [
        (
            &["logbrew", "actions", "release", "checkout@1", "--json"][..],
            "release",
            "use --release <release>",
        ),
        (
            &["logbrew", "events", "env", "production", "--json"][..],
            "env",
            "use --environment <environment> or --env <environment>",
        ),
        (
            &[
                "logbrew",
                "read",
                "action",
                "status",
                "unresolved",
                "--json",
            ][..],
            "status",
            "use --status unresolved/open, --status resolved/closed, or --status ignored",
        ),
        (
            &["logbrew", "events", "trace-id", "trace_123", "--json"][..],
            "trace-id",
            "use --trace <trace_id> or --trace-id <trace_id>",
        ),
        (
            &["logbrew", "event", "name", "checkout_failed", "--json"][..],
            "name",
            "use --name <name>",
        ),
    ] {
        let error =
            parse_command(args.iter().copied()).expect_err("filter word before action name fails");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unexpected_argument");
        assert_eq!(
            body["message"],
            format!("unexpected argument for read: {argument}")
        );
        assert_eq!(body["next"], expected_next);
    }
}
