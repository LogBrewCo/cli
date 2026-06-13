//! CLI parse error rendering tests.

use logbrew_cli::{CliError, parse_command, write_cli_error};

#[test]
fn rejects_non_numeric_limit_with_agent_next_step() {
    let error =
        parse_command(["logbrew", "logs", "--limit", "banana", "--json"]).expect_err("bad limit");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "invalid_limit");
    assert_eq!(body["message"], "invalid limit: banana");
    assert_eq!(body["next"], "use --limit with a positive whole number");
}

#[test]
fn rejects_zero_limit_with_human_next_step() {
    let error = parse_command(["logbrew", "issues", "--limit", "0"]).expect_err("bad limit");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "invalid limit: 0\nNext: use --limit with a positive whole number\n"
    );
}

#[test]
fn rejects_unknown_read_status_with_agent_next_step() {
    let error =
        parse_command(["logbrew", "issues", "--status", "done", "--json"]).expect_err("bad status");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unknown_status");
    assert_eq!(body["message"], "unknown issue status: done");
    assert_eq!(
        body["next"],
        "use one of unresolved/open, resolved/closed, ignored"
    );
}

#[test]
fn rejects_unknown_set_status_with_human_next_step() {
    let error =
        parse_command(["logbrew", "set", "issue", "issue_123", "done"]).expect_err("bad status");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unknown issue status: done\nNext: use one of unresolved/open, resolved/closed, ignored\n"
    );
}

#[test]
fn rejects_unknown_resources_with_command_specific_next_steps() {
    for (args, message, next) in [
        (
            &["logbrew", "read", "metrics", "--json"][..],
            "unknown resource: metrics",
            "choose one of logs, issues, actions, releases, trace, issue",
        ),
        (
            &["logbrew", "watch", "traces", "--json"][..],
            "unknown resource: traces",
            "choose logs or actions",
        ),
        (
            &["logbrew", "explain", "logs", "--json"][..],
            "unknown resource: logs",
            "choose issue or trace",
        ),
        (
            &["logbrew", "set", "release", "api@1", "resolved", "--json"][..],
            "unknown resource: release",
            "choose issue",
        ),
    ] {
        let error = parse_command(args.iter().copied()).expect_err("unknown resource");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unknown_resource");
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn suggests_obvious_top_level_command_typos_for_agents() {
    for (command, next) in [
        ("logg", "did you mean logbrew logs?"),
        ("releaze", "did you mean logbrew releases?"),
        ("statuz", "did you mean logbrew status?"),
    ] {
        let error = parse_command(["logbrew", command, "--json"]).expect_err("unknown command");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unknown_command");
        assert_eq!(body["message"], format!("unknown command: {command}"));
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_top_level_flag_typos_as_flags() {
    for args in [
        &["logbrew", "--bogus", "--json"][..],
        &["logbrew", "--json=true", "status", "--json"][..],
    ] {
        let error = parse_command(args.iter().copied()).expect_err("top-level flag fails");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unknown_flag");
        assert_eq!(body["message"], format!("unknown flag: {}", args[1]));
        assert_eq!(body["next"], "run logbrew --help");
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
fn rejects_plural_trace_read_resource_with_singular_next_step() {
    let error = parse_command(["logbrew", "read", "traces", "--json"]).expect_err("bad resource");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unknown_resource");
    assert_eq!(body["message"], "unknown resource: traces");
    assert_eq!(
        body["next"],
        "use singular trace with an id: logbrew read trace <trace_id>"
    );
}

#[test]
fn rejects_unknown_command_with_human_help_next_step() {
    let error = parse_command(["logbrew", "inspect"]).expect_err("unknown command");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(text, "unknown command: inspect\nNext: run logbrew --help\n");
}

#[test]
fn rejects_release_flag_without_value_before_json() {
    let error =
        parse_command(["logbrew", "logs", "--release", "--json"]).expect_err("bad release flag");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "missing_flag_value");
    assert_eq!(body["message"], "missing value for --release");
    assert_eq!(body["next"], "provide a value after --release");
}

#[test]
fn rejects_single_dash_flag_like_value_with_agent_next_step() {
    let error =
        parse_command(["logbrew", "logs", "--release", "-x", "--json"]).expect_err("bad release");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "missing_flag_value");
    assert_eq!(body["message"], "missing value for --release");
    assert_eq!(body["next"], "provide a value after --release");
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
        let error = parse_command(args.iter().copied()).expect_err("release value is missing");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "missing_flag_value");
        assert_eq!(body["message"], "missing value for --release");
        assert_eq!(body["next"], "provide a value after --release");
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
        let error = parse_command(args.iter().copied()).expect_err("filter value is invalid");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], error_code);
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
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
        let error = parse_command(args.iter().copied()).expect_err("filter is duplicated");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "duplicate_flag");
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_empty_equals_flag_value_with_agent_next_step() {
    let error =
        parse_command(["logbrew", "logs", "--release=", "--json"]).expect_err("bad release flag");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "missing_flag_value");
    assert_eq!(body["message"], "missing value for --release");
    assert_eq!(body["next"], "provide a value after --release");
}

#[test]
fn rejects_name_flag_without_value_before_environment() {
    let error = parse_command([
        "logbrew",
        "actions",
        "--name",
        "--environment",
        "production",
    ])
    .expect_err("bad name flag");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "missing value for --name\nNext: provide a value after --name\n"
    );
}

#[test]
fn rejects_alias_flag_without_value_before_json() {
    let error = parse_command(["logbrew", "logs", "--env", "--json"]).expect_err("bad env flag");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "missing_flag_value");
    assert_eq!(body["message"], "missing value for --env");
    assert_eq!(body["next"], "provide a value after --env");
}

#[test]
fn rejects_duplicate_release_filter_with_agent_next_step() {
    let error = parse_command([
        "logbrew",
        "logs",
        "--release",
        "api@1",
        "--release",
        "api@2",
        "--json",
    ])
    .expect_err("duplicate release fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "duplicate_flag");
    assert_eq!(body["message"], "duplicate flag: --release");
    assert_eq!(body["next"], "use --release once");
}

#[test]
fn rejects_duplicate_alias_and_canonical_filters_with_agent_next_step() {
    let error = parse_command([
        "logbrew",
        "logs",
        "--env",
        "production",
        "--environment",
        "staging",
        "--json",
    ])
    .expect_err("duplicate environment fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "duplicate_flag");
    assert_eq!(body["message"], "duplicate flag: --environment");
    assert_eq!(body["next"], "use --environment once");
}

#[test]
fn rejects_duplicate_json_with_agent_next_step() {
    for args in [
        &["logbrew", "--json", "status", "--json"][..],
        &["logbrew", "--json", "search", "--json", "--", "--timeout"],
        &["logbrew", "help", "logs", "--json", "--json"],
        &["logbrew", "logs", "--help", "--json", "--json"],
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
#[test]
fn rejects_duplicate_login_flag_with_human_next_step() {
    let error =
        parse_command(["logbrew", "login", "--no-open", "--no-open"]).expect_err("duplicate flag");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "duplicate flag: --no-open\nNext: use --no-open once\n"
    );
}

#[test]
fn rejects_unexpected_status_argument_with_agent_next_step() {
    let error =
        parse_command(["logbrew", "status", "production", "--json"]).expect_err("extra arg");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unexpected_argument");
    assert_eq!(
        body["message"],
        "unexpected argument for status: production"
    );
    assert_eq!(body["next"], "run logbrew status --help");
}

#[test]
fn rejects_unexpected_read_argument_with_filter_hint() {
    let error = parse_command(["logbrew", "logs", "checkout@1"]).expect_err("extra arg");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unexpected argument for read: checkout@1\nNext: use --release <release> or run logbrew \
         read --help\n"
    );
}

#[test]
fn rejects_trace_word_after_read_shortcut_with_trace_hint() {
    let error = parse_command([
        "logbrew",
        "logs",
        "trace",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "--json",
    ])
    .expect_err("trace word after logs fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unexpected_argument");
    assert_eq!(body["message"], "unexpected argument for read: trace");
    assert_eq!(
        body["next"],
        "use --trace <trace_id> or run logbrew trace <trace_id>"
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
        let error = parse_command(["logbrew", "logs", argument, "value", "--json"])
            .expect_err("filter word after read shortcut fails");
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
        let error = parse_command(args).expect_err("flag is not an id");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "missing_argument");
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_missing_resources_with_command_specific_next_steps() {
    for (args, next) in [
        (
            &["logbrew", "read", "--json"][..],
            "choose one of logs, issues, actions, releases, trace, issue",
        ),
        (
            &["logbrew", "watch", "--json"][..],
            "choose logs or actions",
        ),
        (
            &["logbrew", "explain", "--json"][..],
            "choose issue or trace",
        ),
        (&["logbrew", "set", "--json"][..], "choose issue"),
    ] {
        let error = parse_command(args.iter().copied()).expect_err("missing resource fails");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "missing_argument");
        assert_eq!(body["message"], "missing argument: resource");
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_missing_search_text_with_log_search_next_step() {
    for command in ["search", "find", "grep"] {
        let error = parse_command(["logbrew", command, "--json"]).expect_err("missing search text");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "missing_argument");
        assert_eq!(body["message"], format!("missing argument: {command}"));
        assert_eq!(
            body["next"],
            "provide search text or run logbrew logs --help"
        );
    }
}

#[test]
fn rejects_empty_search_separator_with_log_search_next_step() {
    let error = parse_command(["logbrew", "search", "--"]).expect_err("missing search text");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "missing argument: search\nNext: provide search text or run logbrew logs --help\n"
    );
}

#[test]
fn rejects_empty_logs_separator_with_search_value_next_step() {
    let error = parse_command(["logbrew", "logs", "--"]).expect_err("missing search text");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "missing value for --search\nNext: provide a value after --search\n"
    );
}

#[test]
fn rejects_flag_like_missing_explain_id_with_human_next_step() {
    let error = parse_command(["logbrew", "explain", "trace", "--json"])
        .expect_err("flag is not an explain id");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "missing argument: trace_id\nNext: provide a trace id\n"
    );
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
        let error = parse_command(args.iter().copied()).expect_err("flag is not a set field");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "missing_argument");
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn writes_parse_errors_as_json_for_agents() {
    let error = parse_command(["logbrew", "releases", "--bogus", "--json"])
        .expect_err("unknown flag fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unknown_flag");
    assert_eq!(body["message"], "unknown flag: --bogus");
    assert_eq!(body["next"], "run logbrew read releases --help");
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
        let error = parse_command(args).expect_err("unsupported flag fails");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unsupported_flag");
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_read_filters_on_status_with_command_help_next_step() {
    let error = parse_command(["logbrew", "status", "--limit", "10", "--json"])
        .expect_err("unsupported flag fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unsupported_flag");
    assert_eq!(body["message"], "unsupported flag for status: --limit");
    assert_eq!(body["next"], "run logbrew status --help");
}

#[test]
fn rejects_ignored_trace_detail_filters_with_command_help_next_step() {
    let error = parse_command([
        "logbrew",
        "read",
        "trace",
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "--limit",
        "10",
        "--json",
    ])
    .expect_err("unsupported trace detail filter fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unsupported_flag");
    assert_eq!(body["message"], "unsupported flag for read trace: --limit");
    assert_eq!(body["next"], "run logbrew read trace --help");
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
    ] {
        let error = parse_command(args.iter().copied()).expect_err("detail filter is unsupported");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unsupported_flag");
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_log_only_filters_on_issue_lists_with_command_help_next_step() {
    let error = parse_command(["logbrew", "issues", "--level", "error", "--json"])
        .expect_err("unsupported issue list filter fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unsupported_flag");
    assert_eq!(
        body["message"],
        "unsupported flag for read issues: --severity"
    );
    assert_eq!(body["next"], "run logbrew read issues --help");
}

#[test]
fn rejects_canonical_severity_on_issue_lists_with_canonical_message() {
    let error = parse_command(["logbrew", "issues", "--severity", "error", "--json"])
        .expect_err("unsupported issue list filter fails");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unsupported_flag");
    assert_eq!(
        body["message"],
        "unsupported flag for read issues: --severity"
    );
    assert_eq!(body["next"], "run logbrew read issues --help");
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
        let error = parse_command(args.iter().copied()).expect_err("unsupported filter fails");
        let mut output = Vec::new();

        write_cli_error(&error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unsupported_flag");
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn rejects_search_without_value_with_human_next_step() {
    let error = parse_command(["logbrew", "logs", "--search", "--json"])
        .expect_err("missing search value fails");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "missing value for --search\nNext: provide a value after --search\n"
    );
}

#[test]
fn rejects_unknown_log_level_with_agent_next_step() {
    let error =
        parse_command(["logbrew", "logs", "--level", "urgent", "--json"]).expect_err("bad level");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid json");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "unknown_log_level");
    assert_eq!(body["message"], "unknown log level: urgent");
    assert_eq!(body["next"], "use one of info, warning, error, critical");
}

#[test]
fn rejects_unknown_log_level_with_human_next_step() {
    let error = parse_command(["logbrew", "logs", "--level", "panic"]).expect_err("bad level");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unknown log level: panic\nNext: use one of info, warning, error, critical\n"
    );
}

#[test]
fn rejects_ignored_issue_detail_filters_with_human_next_step() {
    let error = parse_command([
        "logbrew",
        "read",
        "issue",
        "issue_123",
        "--release",
        "checkout@1.2.3",
    ])
    .expect_err("unsupported issue detail filter fails");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unsupported flag for read issue: --release\nNext: run logbrew read issue --help\n"
    );
}

#[test]
fn rejects_setup_flags_on_watch_with_command_help_next_step() {
    let error =
        parse_command(["logbrew", "watch", "logs", "--auto"]).expect_err("unsupported flag fails");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unsupported flag for watch: --auto\nNext: run logbrew watch --help\n"
    );
}

#[test]
fn writes_parse_errors_with_human_next_step() {
    let error = parse_command(["logbrew", "releases", "--bogus"]).expect_err("unknown flag fails");
    let mut output = Vec::new();

    write_cli_error(&error, false, &mut output).expect("error writes");

    let text = String::from_utf8(output).expect("utf8 output");
    assert_eq!(
        text,
        "unknown flag: --bogus\nNext: run logbrew read releases --help\n"
    );
}
