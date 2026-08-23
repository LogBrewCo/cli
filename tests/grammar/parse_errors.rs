//! CLI parse error rendering tests.

use logbrew_cli::{CliError, parse_command, write_cli_error};

fn rendered_json_error(args: &[&str]) -> (serde_json::Value, String) {
    let error = parse_command(args.iter().copied()).expect_err("command must fail closed");
    let mut output = Vec::new();
    write_cli_error(&error, true, &mut output).expect("error writes");
    let text = String::from_utf8(output).expect("utf8 output");
    let body = serde_json::from_str(text.as_str()).expect("valid json");
    (body, text)
}

fn assert_json_error(args: &[&str], code: &str, message: &str, next: &str) {
    let (body, _) = rendered_json_error(args);
    assert_eq!(body["ok"], false, "{args:?}");
    assert_eq!(body["error"], code, "{args:?}");
    assert_eq!(body["message"], message, "{args:?}");
    assert_eq!(body["next"], next, "{args:?}");
}

fn assert_human_error(args: &[&str], message: &str, next: &str) {
    let error = parse_command(args.iter().copied()).expect_err("command must fail closed");
    let mut output = Vec::new();
    write_cli_error(&error, false, &mut output).expect("error writes");
    assert_eq!(
        String::from_utf8(output).expect("utf8 output"),
        format!("{message}\nNext: {next}\n"),
        "{args:?}"
    );
}

#[test]
fn project_doctor_rejects_malformed_or_hostile_grammar_without_reflection() {
    for args in [
        &["logbrew", "doctor", "--project", "not-a-project", "--json"][..],
        &[
            "logbrew",
            "doctor",
            "--authorization=hostile-secret",
            "--json",
        ],
        &[
            "logbrew",
            "doctor",
            "--project",
            "123e4567-e89b-12d3-a456-426614174000",
            "hostile-secret\ncontrol",
            "--json",
        ],
    ] {
        let (body, text) = rendered_json_error(args);

        assert_eq!(body["error"], "invalid_doctor_command");
        assert_eq!(body["message"], "invalid project doctor command");
        assert_eq!(
            body["next"],
            "use logbrew doctor --project <project_id> with optional --json"
        );
        assert!(!text.contains("hostile-secret"));
        assert!(!text.contains("authorization"));
        assert!(!text.contains("not-a-project"));
    }
}

#[test]
fn rejects_common_json_parse_errors_with_exact_recovery() {
    for (args, code, message, next) in [
        (
            &["logbrew", "logs", "--limit", "banana", "--json"][..],
            "invalid_limit",
            "invalid limit: banana",
            "use --limit with a positive whole number",
        ),
        (
            &["logbrew", "issues", "--status", "done", "--json"][..],
            "unknown_status",
            "unknown issue status: done",
            "use one of unresolved/open, resolved/closed, ignored",
        ),
        (
            &["logbrew", "logs", "--release", "--json"][..],
            "missing_flag_value",
            "missing value for --release",
            "provide a value after --release",
        ),
        (
            &["logbrew", "logs", "--release", "-x", "--json"][..],
            "missing_flag_value",
            "missing value for --release",
            "provide a value after --release",
        ),
        (
            &["logbrew", "logs", "--release=", "--json"][..],
            "missing_flag_value",
            "missing value for --release",
            "provide a value after --release",
        ),
        (
            &["logbrew", "logs", "--env", "--json"][..],
            "missing_flag_value",
            "missing value for --env",
            "provide a value after --env",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--release",
                "api@1",
                "--release",
                "api@2",
                "--json",
            ][..],
            "duplicate_flag",
            "duplicate flag: --release",
            "use --release once",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--env",
                "production",
                "--environment",
                "staging",
                "--json",
            ][..],
            "duplicate_flag",
            "duplicate flag: --environment",
            "use --environment once",
        ),
        (
            &["logbrew", "status", "production", "--json"][..],
            "unexpected_argument",
            "unexpected argument for status: production",
            "run logbrew status --help",
        ),
        (
            &["logbrew", "releases", "--bogus", "--json"][..],
            "unknown_flag",
            "unknown flag: --bogus",
            "run logbrew read releases --help",
        ),
        (
            &["logbrew", "status", "--limit", "10", "--json"][..],
            "unsupported_flag",
            "unsupported flag for status: --limit",
            "run logbrew status --help",
        ),
        (
            &["logbrew", "logs", "--level", "urgent", "--json"][..],
            "unknown_log_level",
            "unknown log level: urgent",
            "use one of info, warning, error, critical",
        ),
    ] {
        assert_json_error(args, code, message, next);
    }
}

#[test]
fn rejects_common_human_parse_errors_with_exact_recovery() {
    for (args, message, next) in [
        (
            &["logbrew", "issues", "--limit", "0"][..],
            "invalid limit: 0",
            "use --limit with a positive whole number",
        ),
        (
            &["logbrew", "set", "issue", "issue_123", "done"][..],
            "unknown issue status: done",
            "use one of unresolved/open, resolved/closed, ignored",
        ),
        (
            &["logbrew", "inspect"][..],
            "unknown command: inspect",
            "run logbrew --help",
        ),
        (
            &[
                "logbrew",
                "actions",
                "--name",
                "--environment",
                "production",
            ][..],
            "missing value for --name",
            "provide a value after --name",
        ),
        (
            &["logbrew", "login", "--no-open", "--no-open"][..],
            "duplicate flag: --no-open",
            "use --no-open once",
        ),
        (
            &["logbrew", "logs", "checkout@1"][..],
            "unexpected argument for read: checkout@1",
            "use --release <release> or run logbrew read --help",
        ),
        (
            &["logbrew", "search", "--"][..],
            "missing argument: search",
            "provide search text or run logbrew logs --help",
        ),
        (
            &["logbrew", "logs", "--"][..],
            "missing value for --search",
            "provide a value after --search",
        ),
        (
            &["logbrew", "explain", "trace", "--json"][..],
            "missing argument: trace_id",
            "provide a trace id",
        ),
        (
            &["logbrew", "logs", "--search", "--json"][..],
            "missing value for --search",
            "provide a value after --search",
        ),
        (
            &["logbrew", "logs", "--level", "panic"][..],
            "unknown log level: panic",
            "use one of info, warning, error, critical",
        ),
        (
            &[
                "logbrew",
                "read",
                "issue",
                "issue_123",
                "--release",
                "checkout@1.2.3",
            ][..],
            "unsupported flag for read issue: --release",
            "run logbrew read issue --help",
        ),
        (
            &["logbrew", "watch", "logs", "--auto"][..],
            "unsupported flag for watch: --auto",
            "run logbrew watch --help",
        ),
        (
            &["logbrew", "releases", "--bogus"][..],
            "unknown flag: --bogus",
            "run logbrew read releases --help",
        ),
    ] {
        assert_human_error(args, message, next);
    }
}

#[test]
fn rejects_unknown_resources_with_command_specific_next_steps() {
    for (args, message, next) in [
        (
            &["logbrew", "read", "metrics", "--json"][..],
            "unknown resource: metrics",
            "choose one of logs, issues, actions, releases, traces, trace, issue",
        ),
        (
            &["logbrew", "watch", "traces", "--json"][..],
            "unknown resource: traces",
            "use logbrew traces for recent traces, or logbrew trace <trace_id> for one trace",
        ),
        (
            &["logbrew", "explain", "logs", "--json"][..],
            "unknown resource: logs",
            "choose issue, log, action, trace, span, release, or metric",
        ),
        (
            &["logbrew", "set", "release", "api@1", "resolved", "--json"][..],
            "unknown resource: release",
            "choose issue",
        ),
    ] {
        assert_json_error(args, "unknown_resource", message, next);
    }
}

#[test]
fn suggests_obvious_top_level_command_typos_for_agents() {
    for (command, next) in [
        ("logg", "did you mean logbrew logs?"),
        ("releaze", "did you mean logbrew releases?"),
        ("statuz", "did you mean logbrew status?"),
    ] {
        assert_json_error(
            &["logbrew", command, "--json"],
            "unknown_command",
            format!("unknown command: {command}").as_str(),
            next,
        );
    }
}

#[test]
fn rejects_top_level_flag_typos_as_flags() {
    for args in [
        &["logbrew", "--bogus", "--json"][..],
        &["logbrew", "--json=true", "status", "--json"][..],
    ] {
        assert_json_error(
            args,
            "unknown_flag",
            format!("unknown flag: {}", args[1]).as_str(),
            "run logbrew --help",
        );
    }
}

#[test]
fn rejects_inline_values_on_simple_command_flags_with_command_help() {
    assert_eq!(
        parse_command(["logbrew", "logs", "--json=true", "--json"]),
        Err(CliError::UnsupportedFlag {
            flag: "--json=true".to_owned(),
            command: "read logs",
            next: "run logbrew read logs --help",
        })
    );
}

#[test]
fn keeps_missing_value_recovery_before_later_unsupported_filter() {
    for args in [
        &[
            "logbrew",
            "logs",
            "--release",
            "--status",
            "unresolved",
            "--json",
        ][..],
        &[
            "logbrew",
            "logs",
            "--release=",
            "--status",
            "unresolved",
            "--json",
        ][..],
    ] {
        assert_json_error(
            args,
            "missing_flag_value",
            "missing value for --release",
            "provide a value after --release",
        );
    }
}

#[test]
fn keeps_invalid_value_recovery_before_later_unsupported_filter() {
    for (args, error_code, message, next) in [
        (
            &[
                "logbrew",
                "logs",
                "--limit=banana",
                "--status",
                "unresolved",
                "--json",
            ][..],
            "invalid_limit",
            "invalid limit: banana",
            "use --limit with a positive whole number",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--level",
                "panic",
                "--status",
                "unresolved",
                "--json",
            ][..],
            "unknown_log_level",
            "unknown log level: panic",
            "use one of info, warning, error, critical",
        ),
        (
            &[
                "logbrew", "issues", "--status", "done", "--level", "error", "--json",
            ][..],
            "unknown_status",
            "unknown issue status: done",
            "use one of unresolved/open, resolved/closed, ignored",
        ),
    ] {
        assert_json_error(args, error_code, message, next);
    }
}

#[test]
fn keeps_duplicate_flag_recovery_before_later_unsupported_filter() {
    for (args, message, next) in [
        (
            &[
                "logbrew",
                "logs",
                "--json",
                "--json",
                "--status",
                "unresolved",
            ][..],
            "duplicate flag: --json",
            "use --json once",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--release",
                "checkout@1",
                "--release",
                "checkout@2",
                "--status",
                "unresolved",
                "--json",
            ][..],
            "duplicate flag: --release",
            "use --release once",
        ),
        (
            &[
                "logbrew",
                "logs",
                "--env",
                "production",
                "--environment",
                "staging",
                "--status",
                "unresolved",
                "--json",
            ][..],
            "duplicate flag: --environment",
            "use --environment once",
        ),
    ] {
        assert_json_error(args, "duplicate_flag", message, next);
    }
}

#[test]
fn rejects_duplicate_json_with_agent_next_step() {
    for args in [
        &["logbrew", "--json", "status", "--json"][..],
        &["logbrew", "--json", "search", "--json", "--", "--timeout"],
        &["logbrew", "help", "logs", "--json", "--json"],
        &["logbrew", "logs", "--help", "--json", "--json"],
    ] {
        assert_json_error(
            args,
            "duplicate_flag",
            "duplicate flag: --json",
            "use --json once",
        );
    }
}
#[test]
fn rejects_trace_word_after_read_shortcut_with_trace_hint() {
    assert_json_error(
        &[
            "logbrew",
            "logs",
            "trace",
            "4bf92f3577b34da6a3ce929d0e0e4736",
            "--json",
        ],
        "unexpected_argument",
        "unexpected argument for read: trace",
        "use --trace <trace_id> or run logbrew trace <trace_id>",
    );
}

#[test]
fn rejects_filter_words_after_read_shortcuts_with_specific_hints() {
    for (argument, expected_next) in [
        (
            "env",
            "use --environment <environment> or --env <environment>",
        ),
        (
            "environment",
            "use --environment <environment> or --env <environment>",
        ),
        ("release", "use --release <release>"),
        (
            "project",
            "use --project <project_id> or --project-id <project_id>",
        ),
        (
            "status",
            "use --status unresolved/open, --status resolved/closed, or --status ignored",
        ),
        (
            "level",
            "use --severity info, warning, error, or critical; --level is also accepted",
        ),
        ("search", "use --search <text>"),
        (
            "user",
            "use --user <distinct_id> or --distinct-id <distinct_id>",
        ),
        ("name", "use --name <name>"),
        ("since", "use --since <duration>"),
        ("limit", "use --limit with a positive whole number"),
    ] {
        assert_json_error(
            &["logbrew", "logs", argument, "value", "--json"],
            "unexpected_argument",
            format!("unexpected argument for read: {argument}").as_str(),
            expected_next,
        );
    }
}

#[test]
fn rejects_flag_like_missing_read_ids_with_agent_next_steps() {
    for (args, message, next) in [
        (
            ["logbrew", "read", "trace", "--json"],
            "missing argument: trace_id",
            "provide a trace id",
        ),
        (
            ["logbrew", "read", "issue", "--json"],
            "missing argument: issue_id",
            "provide an issue id",
        ),
    ] {
        assert_json_error(&args, "missing_argument", message, next);
    }
}

#[test]
fn rejects_missing_resources_with_command_specific_next_steps() {
    for (args, next) in [
        (
            &["logbrew", "read", "--json"][..],
            "choose one of logs, issues, actions, releases, traces, trace, issue",
        ),
        (
            &["logbrew", "explain", "--json"][..],
            "choose issue, log, action, trace, span, release, or metric",
        ),
        (&["logbrew", "set", "--json"][..], "choose issue"),
    ] {
        assert_json_error(args, "missing_argument", "missing argument: resource", next);
    }
}

#[test]
fn rejects_missing_search_text_with_log_search_next_step() {
    for command in ["search", "find", "grep"] {
        assert_json_error(
            &["logbrew", command, "--json"],
            "missing_argument",
            format!("missing argument: {command}").as_str(),
            "provide search text or run logbrew logs --help",
        );
    }
}

#[test]
fn rejects_flag_like_missing_set_fields_with_agent_next_steps() {
    for (args, message, next) in [
        (
            &["logbrew", "set", "issue", "--json"][..],
            "missing argument: issue_id",
            "provide an issue id",
        ),
        (
            &["logbrew", "set", "issue", "issue_123", "--json"][..],
            "missing argument: status",
            "provide one of unresolved/open, resolved/closed, ignored",
        ),
    ] {
        assert_json_error(args, "missing_argument", message, next);
    }
}

#[test]
fn rejects_read_filters_on_login_with_command_help_next_step() {
    for (args, message, next) in [
        (
            ["logbrew", "login", "--release", "api@1", "--json"],
            "unsupported flag for login: --release",
            "run logbrew login --help",
        ),
        (
            ["logbrew", "logout", "--release", "api@1", "--json"],
            "unsupported flag for logout: --release",
            "run logbrew logout --help",
        ),
    ] {
        assert_json_error(&args, "unsupported_flag", message, next);
    }
}

#[test]
fn rejects_ignored_trace_detail_filters_with_command_help_next_step() {
    assert_json_error(
        &[
            "logbrew",
            "read",
            "trace",
            "4bf92f3577b34da6a3ce929d0e0e4736",
            "--limit",
            "10",
            "--json",
        ],
        "unsupported_flag",
        "unsupported flag for read trace: --limit",
        "run logbrew read trace --help",
    );
}

#[test]
fn rejects_detail_filters_before_validating_list_only_values() {
    for (args, message, next) in [
        (
            &[
                "logbrew",
                "read",
                "trace",
                "4bf92f3577b34da6a3ce929d0e0e4736",
                "--limit",
                "0",
                "--json",
            ][..],
            "unsupported flag for read trace: --limit",
            "run logbrew read trace --help",
        ),
        (
            &[
                "logbrew",
                "4bf92f3577b34da6a3ce929d0e0e4736",
                "--limit",
                "0",
                "--json",
            ][..],
            "unsupported flag for read trace: --limit",
            "run logbrew read trace --help",
        ),
        (
            &[
                "logbrew",
                "read",
                "issue",
                "issue_123",
                "--status",
                "closed",
                "--json",
            ][..],
            "unsupported flag for read issue: --status",
            "run logbrew read issue --help",
        ),
        (
            &["logbrew", "issue_123", "--status", "closed", "--json"][..],
            "unsupported flag for read issue: --status",
            "run logbrew read issue --help",
        ),
        (
            &[
                "logbrew",
                "read",
                "trace",
                "4bf92f3577b34da6a3ce929d0e0e4736",
                "--service",
                "checkout-api",
                "--json",
            ][..],
            "unsupported flag for read trace: --service",
            "run logbrew read trace --help",
        ),
        (
            &[
                "logbrew",
                "read",
                "issue",
                "issue_123",
                "--service-name",
                "checkout-api",
                "--json",
            ][..],
            "unsupported flag for read issue: --service",
            "run logbrew read issue --help",
        ),
    ] {
        assert_json_error(args, "unsupported_flag", message, next);
    }
}

#[test]
fn rejects_log_only_filters_on_issue_lists_with_command_help_next_step() {
    assert_json_error(
        &["logbrew", "issues", "--level", "error", "--json"],
        "unsupported_flag",
        "unsupported flag for read issues: --severity",
        "run logbrew read issues --help",
    );
}

#[test]
fn rejects_canonical_severity_on_issue_lists_with_canonical_message() {
    assert_json_error(
        &["logbrew", "issues", "--severity", "error", "--json"],
        "unsupported_flag",
        "unsupported flag for read issues: --severity",
        "run logbrew read issues --help",
    );
}

#[test]
fn rejects_list_filters_that_target_cannot_apply() {
    for (args, message, next) in [
        (
            &["logbrew", "logs", "--status", "unresolved", "--json"][..],
            "unsupported flag for read logs: --status",
            "run logbrew read logs --help",
        ),
        (
            &["logbrew", "logs", "--status", "closed", "--json"][..],
            "unsupported flag for read logs: --status",
            "run logbrew read logs --help",
        ),
        (
            &["logbrew", "logs", "--name", "checkout_failed", "--json"][..],
            "unsupported flag for read logs: --name",
            "run logbrew read logs --help",
        ),
        (
            &["logbrew", "actions", "--status", "unresolved", "--json"][..],
            "unsupported flag for read actions: --status",
            "run logbrew read actions --help",
        ),
        (
            &["logbrew", "issues", "--level", "panic", "--json"][..],
            "unsupported flag for read issues: --severity",
            "run logbrew read issues --help",
        ),
        (
            &["logbrew", "releases", "--name", "checkout_failed", "--json"][..],
            "unsupported flag for read releases: --name",
            "run logbrew read releases --help",
        ),
        (
            &["logbrew", "releases", "--level", "panic", "--json"][..],
            "unsupported flag for read releases: --severity",
            "run logbrew read releases --help",
        ),
    ] {
        assert_json_error(args, "unsupported_flag", message, next);
    }
}
