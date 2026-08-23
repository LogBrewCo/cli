//! CLI command grammar tests.

use logbrew_cli::{
    Command, HelpTopic, LoginProvider, ProjectSetupSeenOptions, ReadOptions, ReadTarget, SetTarget,
    help, parse_command,
};

fn assert_command(args: &[&str], expected: Command) {
    assert_eq!(
        parse_command(args.iter().copied()),
        Ok(expected),
        "unexpected command for {args:?}"
    );
}

fn assert_help(args: &[&str], topic: HelpTopic, json: bool) {
    assert_command(args, Command::Help { topic, json });
}

fn assert_path(args: &[&str], expected: &str) {
    let command = parse_command(args.iter().copied()).expect("command parses");
    assert_eq!(
        command.http_path().expect("command has endpoint"),
        expected,
        "unexpected endpoint for {args:?}"
    );
}

fn read_command(target: ReadTarget, options: ReadOptions, json: bool) -> Command {
    Command::Read {
        target,
        options: Box::new(options),
        json,
    }
}

fn assert_command_path(args: &[&str], expected: Command, path: &str) {
    let command = parse_command(args.iter().copied()).expect("command parses");
    assert_eq!(command, expected, "unexpected command for {args:?}");
    assert_eq!(
        command.http_path().expect("command has endpoint"),
        path,
        "unexpected endpoint for {args:?}"
    );
}

#[test]
fn read_options_default_has_no_filters() {
    assert_eq!(
        ReadOptions::default(),
        ReadOptions {
            name: None,
            service: None,
            since: None,
            user: None,
            trace: None,
            level: None,
            search: None,
            project: None,
            release: None,
            environment: None,
            status: None,
            limit: None,
            min_duration_ms: None,
            pagination: None,
            cursor_time: None,
            cursor_id: None,
        }
    );
}

#[test]
fn parses_root_help_forms() {
    for (args, json) in [
        (&["logbrew", "--help"][..], false),
        (&["logbrew"][..], false),
        (&["logbrew", "--json"][..], true),
    ] {
        assert_help(args, HelpTopic::Root, json);
    }
}

#[test]
fn parses_examples_help_for_first_run_discovery() {
    for args in [
        &["logbrew", "examples"][..],
        &["logbrew", "examples", "--help"],
        &["logbrew", "help", "examples"],
        &["logbrew", "help", "example"],
        &["logbrew", "sample"],
        &["logbrew", "recipes"],
    ] {
        assert_help(args, HelpTopic::Examples, false);
    }
    assert_help(
        &["logbrew", "--json", "examples"],
        HelpTopic::Examples,
        true,
    );
}

#[test]
fn parses_global_json_before_commands_for_agents() {
    assert_command(
        &["logbrew", "--json", "status"],
        Command::Status { json: true },
    );
    assert_command(
        &["logbrew", "--json", "logs", "--release", "checkout@1"],
        read_command(
            ReadTarget::Logs,
            ReadOptions {
                release: Some("checkout@1".to_owned()),
                ..ReadOptions::default()
            },
            true,
        ),
    );
}

#[test]
fn parses_health_and_doctor_as_status_aliases() {
    for args in [
        &["logbrew", "health"][..],
        &["logbrew", "ping"],
        &["logbrew", "doctor"],
        &["logbrew", "health", "--json"],
        &["logbrew", "--json", "ping"],
    ] {
        assert_command(
            args,
            Command::Status {
                json: args.contains(&"--json"),
            },
        );
    }

    for args in [
        &["logbrew", "health", "--help"][..],
        &["logbrew", "ping", "--help"],
        &["logbrew", "doctor", "--help"],
        &["logbrew", "help", "health"],
        &["logbrew", "help", "ping"],
        &["logbrew", "help", "doctor"],
    ] {
        assert_help(args, HelpTopic::Status, false);
    }
}

#[test]
fn parses_project_scoped_doctor_without_changing_the_bare_alias() {
    const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    for args in [
        &["logbrew", "doctor", "--project", PROJECT_ID, "--json"][..],
        &["logbrew", "doctor", "--project-id", PROJECT_ID, "--json"],
        &["logbrew", "--json", "doctor", "--project", PROJECT_ID],
        &[
            "logbrew",
            "doctor",
            "--project=123e4567-e89b-12d3-a456-426614174000",
            "--json",
        ],
    ] {
        assert_command(
            args,
            Command::Doctor {
                project_id: PROJECT_ID.to_owned(),
                json: true,
            },
        );
    }
    assert_command(&["logbrew", "doctor"], Command::Status { json: false });
}

#[test]
fn parses_whoami_and_me_as_authenticated_identity_reads() {
    for args in [
        &["logbrew", "whoami"][..],
        &["logbrew", "me"],
        &["logbrew", "whoami", "--json"],
        &["logbrew", "--json", "me"],
    ] {
        assert_command(
            args,
            Command::WhoAmI {
                json: args.contains(&"--json"),
            },
        );
    }

    for args in [
        &["logbrew", "whoami", "--help"][..],
        &["logbrew", "help", "me"],
    ] {
        assert_help(args, HelpTopic::Status, false);
    }
}

#[test]
fn help_keeps_agent_auth_watch_and_investigation_contracts() {
    macro_rules! assert_help {
        ($topic:expr; $($expected:literal),+ $(,)?) => {
            let text = help::help_text($topic);
            $(assert!(text.contains($expected), "{}: {}", stringify!($topic), $expected);)+
        };
    }
    assert_help!(HelpTopic::Status;
        "logbrew whoami [--json]",
        "logbrew me [--json]",
        "logbrew auth status [--json]",
        "Status checks API reachability and authentication.",
        "Whoami/me return the authenticated account identity.");
    assert_help!(HelpTopic::Login;
        "stores a private local access/refresh pair",
        "refresh local auth once after an expired-token response",
        "--provider github|gitlab|bitbucket",
        "--json prints the auth handoff without opening a browser.");
    assert_help!(HelpTopic::Watch;
        "logbrew watch --json",
        "logbrew watch issues [--json]",
        "logbrew watch --severity error,critical --json",
        "Aliases: tail, follow, and stream use the same live watch flow.",
        "Live watch uses a short-lived feed ticket and WebSocket stream.",
        "Transient disconnects reconnect with a fresh ticket and backoff.");
    assert_help!(HelpTopic::Explain;
        "logbrew explain <issue_id_or_trace_id> [--json]",
        "logbrew issue <issue_id> explain [--occurrence <recommended|first|latest|occurrence_id>] [--json]",
        "logbrew trace <trace_id> explain [--json]",
        "logbrew <issue_id_or_trace_id> explain [--json]",
        "Pasted UUID/issue_* values are treated as issues",
        "32-hex/trace_* values are treated as traces");
}

#[test]
fn parses_basic_resource_help_forms() {
    for (args, topic, json) in [
        (
            &["logbrew", "read", "logs", "--help"][..],
            HelpTopic::ReadLogs,
            false,
        ),
        (
            &["logbrew", "help", "read", "logs"][..],
            HelpTopic::ReadLogs,
            false,
        ),
        (&["logbrew", "help", "logs"][..], HelpTopic::ReadLogs, false),
        (
            &["logbrew", "help", "releases", "--json"][..],
            HelpTopic::ReadReleases,
            true,
        ),
        (
            &["logbrew", "read", "actions", "--help", "--json"][..],
            HelpTopic::ReadActions,
            true,
        ),
        (
            &["logbrew", "releases", "--help"][..],
            HelpTopic::ReadReleases,
            false,
        ),
    ] {
        assert_help(args, topic, json);
    }
}

#[test]
fn parses_list_singular_collection_help_as_list_help() {
    for (args, topic) in [
        (
            &["logbrew", "list", "log", "--help", "--json"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "help", "list", "issue", "--json"],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "help", "list", "release", "--json"],
            HelpTopic::ReadReleases,
        ),
    ] {
        assert_help(args, topic, true);
    }
    assert_help(
        &["logbrew", "get", "issue", "--help", "--json"],
        HelpTopic::ReadIssue,
        true,
    );
}

#[test]
fn parses_common_help_terms_as_real_user_topics() {
    for (args, topic, json) in [
        (
            &["logbrew", "help", "traces", "--json"][..],
            HelpTopic::ReadTraces,
            true,
        ),
        (
            &["logbrew", "help", "spans", "--json"][..],
            HelpTopic::ReadTraces,
            true,
        ),
        (
            &["logbrew", "help", "errors", "--json"][..],
            HelpTopic::ReadIssues,
            true,
        ),
        (
            &["logbrew", "help", "action", "--json"][..],
            HelpTopic::ReadActions,
            true,
        ),
        (
            &["logbrew", "help", "events", "--json"][..],
            HelpTopic::ReadActions,
            true,
        ),
        (
            &["logbrew", "help", "environments", "--json"][..],
            HelpTopic::Read,
            true,
        ),
        (
            &["logbrew", "help", "filters", "--json"][..],
            HelpTopic::Read,
            true,
        ),
        (
            &["logbrew", "help", "filter", "--json"][..],
            HelpTopic::Read,
            true,
        ),
        (
            &["logbrew", "help", "project", "--json"][..],
            HelpTopic::Projects,
            true,
        ),
        (
            &["logbrew", "help", "projects", "--json"][..],
            HelpTopic::Projects,
            true,
        ),
        (
            &["logbrew", "help", "usage", "--json"][..],
            HelpTopic::Usage,
            true,
        ),
        (
            &["logbrew", "help", "project-id", "--json"][..],
            HelpTopic::Read,
            true,
        ),
        (
            &["logbrew", "action", "--help"][..],
            HelpTopic::ReadActions,
            false,
        ),
        (
            &["logbrew", "traces", "--help"][..],
            HelpTopic::ReadTraces,
            false,
        ),
        (
            &["logbrew", "help", "read", "traces"][..],
            HelpTopic::ReadTraces,
            false,
        ),
        (
            &["logbrew", "help", "read", "action"][..],
            HelpTopic::ReadActions,
            false,
        ),
        (
            &["logbrew", "show", "logs", "--help"][..],
            HelpTopic::ReadLogs,
            false,
        ),
        (
            &["logbrew", "help", "get", "issue"][..],
            HelpTopic::ReadIssue,
            false,
        ),
        (
            &["logbrew", "filters", "--help"][..],
            HelpTopic::Read,
            false,
        ),
        (
            &["logbrew", "project", "--help"][..],
            HelpTopic::Projects,
            false,
        ),
        (
            &["logbrew", "help", "read", "project"][..],
            HelpTopic::Read,
            false,
        ),
    ] {
        assert_help(args, topic, json);
    }
    assert!(help::help_text(HelpTopic::Read).contains(
        "Use --environment <environment> with logs, issues, actions, releases, or traces."
    ));
    assert!(help::help_text(HelpTopic::Read).contains(
        "Filter aliases: --service-name, --env, --project-id, --trace-id, and --distinct-id."
    ));
}

#[test]
fn parses_filter_terms_as_top_level_discovery_help() {
    for args in [
        ["logbrew", "env", "--json"],
        ["logbrew", "environment", "--json"],
        ["logbrew", "environments", "--json"],
        ["logbrew", "filters", "--json"],
        ["logbrew", "project-id", "--json"],
        ["logbrew", "service", "--json"],
        ["logbrew", "service-name", "--json"],
    ] {
        assert_help(&args, HelpTopic::Read, true);
    }
}

#[test]
fn parses_authenticated_project_and_usage_reads() {
    for args in [
        &["logbrew", "project", "--json"][..],
        &["logbrew", "projects", "--json"],
        &["logbrew", "--json", "projects"],
    ] {
        let command = parse_command(args.iter().copied()).expect("project catalog read parses");
        assert_eq!(command, Command::Projects { json: true });
    }

    for args in [
        &["logbrew", "usage", "--json"][..],
        &["logbrew", "--json", "usage"],
        &["logbrew", "account", "usage", "--json"],
    ] {
        let command = parse_command(args.iter().copied()).expect("usage read parses");
        assert_eq!(command, Command::Usage { json: true });
    }
}

#[test]
fn parses_project_setup_seen_contract_call() {
    let command = parse_command([
        "logbrew",
        "projects",
        "setup",
        "proj_123",
        "--runtime",
        "node",
        "--source",
        "cli",
        "--environment",
        "production",
        "--json",
    ])
    .expect("project setup seen parses");

    assert_eq!(
        command,
        Command::ProjectSetupSeen {
            project_id: "proj_123".to_owned(),
            options: ProjectSetupSeenOptions {
                runtime: Some("node".to_owned()),
                source: Some("cli".to_owned()),
                environment: Some("production".to_owned()),
            },
            json: true,
        }
    );

    let global_json = parse_command(["logbrew", "--json", "project", "setup", "proj_123"])
        .expect("global json project setup parses");
    assert_eq!(
        global_json,
        Command::ProjectSetupSeen {
            project_id: "proj_123".to_owned(),
            options: ProjectSetupSeenOptions::default(),
            json: true,
        }
    );
}

#[test]
fn parses_bare_trace_terms_by_singular_or_plural_meaning() {
    for args in [
        &["logbrew", "trace", "--json"][..],
        &["logbrew", "span", "--json"],
    ] {
        let command = parse_command(args.iter().copied()).expect("trace detail help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadTrace,
                json: args.contains(&"--json")
            }
        );
    }
    for args in [
        &["logbrew", "traces", "--json"][..],
        &["logbrew", "spans", "--json"],
        &["logbrew", "traces"],
        &["logbrew", "--json", "spans"],
    ] {
        assert_command(
            args,
            read_command(
                ReadTarget::Traces,
                ReadOptions::default(),
                args.contains(&"--json"),
            ),
        );
    }
}

#[test]
fn parses_auth_help_as_real_user_topic() {
    for args in [
        &["logbrew", "help", "auth"][..],
        &["logbrew", "auth", "--help"],
        &["logbrew", "auth"],
        &["logbrew", "help", "authentication"],
        &["logbrew", "help", "token"],
        &["logbrew", "token", "--help"],
        &["logbrew", "token"],
        &["logbrew", "help", "credentials"],
        &["logbrew", "credentials", "--help"],
        &["logbrew", "credentials"],
        &["logbrew", "help", "account"],
        &["logbrew", "account", "--help"],
        &["logbrew", "account"],
        &["logbrew", "help", "profile"],
        &["logbrew", "profile", "--help"],
        &["logbrew", "profile"],
        &["logbrew", "help", "identity"],
        &["logbrew", "identity", "--help"],
        &["logbrew", "identity"],
        &["logbrew", "help", "user"],
        &["logbrew", "user", "--help"],
        &["logbrew", "user"],
    ] {
        assert_help(args, HelpTopic::Auth, false);
    }

    let text = help::help_text(HelpTopic::Auth);
    assert!(text.contains("logbrew login"));
    assert!(text.contains("logbrew auth login"));
    assert!(text.contains("logbrew status"));
    assert!(text.contains("logbrew auth status"));
    assert!(text.contains("logbrew auth whoami"));
    assert!(text.contains("logbrew auth me"));
    assert!(text.contains("logbrew whoami"));
    assert!(text.contains("logbrew me"));
    assert!(text.contains("logbrew logout"));
    assert!(text.contains("logbrew auth logout"));
    assert!(text.contains("Use --json for agent-readable auth checks."));

    for args in [
        &["logbrew", "auth", "--json"][..],
        &["logbrew", "--json", "auth"],
        &["logbrew", "--json", "token"],
    ] {
        assert_help(args, HelpTopic::Auth, true);
    }
}

#[test]
fn parses_auth_namespace_as_token_safe_command_aliases() {
    assert_command(
        &["logbrew", "auth", "status"],
        Command::Status { json: false },
    );

    for args in [
        &["logbrew", "auth", "status", "--json"][..],
        &["logbrew", "--json", "auth", "status"],
        &["logbrew", "auth", "--json", "status"],
    ] {
        assert_command(args, Command::Status { json: true });
    }

    for args in [
        &["logbrew", "auth", "whoami"][..],
        &["logbrew", "auth", "me"],
        &["logbrew", "auth", "whoami", "--json"],
        &["logbrew", "auth", "--json", "me"],
    ] {
        assert_command(
            args,
            Command::WhoAmI {
                json: args.contains(&"--json"),
            },
        );
    }

    assert_command(
        &["logbrew", "auth", "login", "--no-open"],
        Command::Login {
            provider: LoginProvider::GitHub,
            open_browser: false,
            json: false,
        },
    );
    for args in [
        &["logbrew", "auth", "login", "--json"][..],
        &["logbrew", "auth", "--json", "login"],
    ] {
        assert_command(
            args,
            Command::Login {
                provider: LoginProvider::GitHub,
                open_browser: false,
                json: true,
            },
        );
    }
    assert_command(
        &["logbrew", "auth", "logout", "--json"],
        Command::Logout { json: true },
    );
}

#[test]
fn parses_auth_namespace_help_for_subcommands() {
    for (args, topic) in [
        (
            &["logbrew", "auth", "login", "--help"][..],
            HelpTopic::Login,
        ),
        (&["logbrew", "auth", "status", "--help"], HelpTopic::Status),
        (&["logbrew", "auth", "whoami", "--help"], HelpTopic::Status),
        (&["logbrew", "auth", "me", "--help"], HelpTopic::Status),
        (&["logbrew", "auth", "logout", "--help"], HelpTopic::Logout),
        (&["logbrew", "help", "auth", "login"], HelpTopic::Login),
        (&["logbrew", "help", "auth", "status"], HelpTopic::Status),
        (&["logbrew", "help", "auth", "logout"], HelpTopic::Logout),
        (&["logbrew", "auth", "help", "login"], HelpTopic::Login),
        (&["logbrew", "auth", "help", "status"], HelpTopic::Status),
        (&["logbrew", "auth", "help", "whoami"], HelpTopic::Status),
        (&["logbrew", "auth", "help", "me"], HelpTopic::Status),
        (&["logbrew", "auth", "help", "logout"], HelpTopic::Logout),
    ] {
        assert_help(args, topic, false);
    }
    assert_help(
        &["logbrew", "auth", "help", "status", "--json"],
        HelpTopic::Status,
        true,
    );
}

#[test]
fn parses_json_help_as_agent_output_topic() {
    for args in [
        &["logbrew", "help", "json"][..],
        &["logbrew", "json", "--help"],
        &["logbrew", "json"],
        &["logbrew", "help", "output"],
    ] {
        assert_help(args, HelpTopic::Json, false);
    }

    let text = help::help_text(HelpTopic::Json);
    assert!(text.contains("logbrew --json status"));
    assert!(text.contains("logbrew status --json"));
    assert!(text.contains("Stable JSON keeps server response shapes"));
    assert!(text.contains("Errors include ok, error, message, and next."));

    assert_help(
        &["logbrew", "--json", "help", "json"],
        HelpTopic::Json,
        true,
    );
}

#[test]
fn parses_subcommand_resource_help_as_real_user_topics() {
    let cases = [
        (
            &["logbrew", "watch", "logs", "--help"][..],
            HelpTopic::Watch,
        ),
        (
            &["logbrew", "help", "watch", "actions"][..],
            HelpTopic::Watch,
        ),
        (&["logbrew", "watch", "help", "logs"][..], HelpTopic::Watch),
        (
            &["logbrew", "watch", "event", "--help"][..],
            HelpTopic::Watch,
        ),
        (
            &["logbrew", "help", "watch", "events"][..],
            HelpTopic::Watch,
        ),
        (
            &["logbrew", "explain", "trace", "--help"][..],
            HelpTopic::Explain,
        ),
        (
            &["logbrew", "help", "explain", "issue"][..],
            HelpTopic::Explain,
        ),
        (
            &["logbrew", "explain", "help", "trace"][..],
            HelpTopic::Explain,
        ),
        (&["logbrew", "set", "issue", "--help"][..], HelpTopic::Set),
        (&["logbrew", "help", "set", "issue"][..], HelpTopic::Set),
        (&["logbrew", "set", "help", "issue"][..], HelpTopic::Set),
        (&["logbrew", "help", "resolve", "issue"][..], HelpTopic::Set),
        (&["logbrew", "help", "close", "issue"][..], HelpTopic::Set),
        (&["logbrew", "help", "ignore", "issue"][..], HelpTopic::Set),
        (&["logbrew", "help", "reopen", "issue"][..], HelpTopic::Set),
    ];

    for (args, topic) in cases {
        assert_help(args, topic, false);
    }

    for args in [
        ["logbrew", "tail", "logs", "--help"],
        ["logbrew", "follow", "actions", "--help"],
        ["logbrew", "stream", "events", "--help"],
        ["logbrew", "help", "tail", "logs"],
        ["logbrew", "help", "follow", "actions"],
        ["logbrew", "help", "stream", "events"],
        ["logbrew", "tail", "help", "logs"],
        ["logbrew", "follow", "help", "actions"],
        ["logbrew", "stream", "help", "events"],
    ] {
        assert_help(&args, HelpTopic::Watch, false);
    }
}

#[test]
fn parses_recency_read_help_as_real_user_topics() {
    let cases = [
        (
            &["logbrew", "latest", "logs", "--help"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "recent", "issues", "--help"][..],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "last", "action", "checkout_failed", "--help"][..],
            HelpTopic::ReadActions,
        ),
        (
            &["logbrew", "newest", "release", "--help"][..],
            HelpTopic::ReadReleases,
        ),
        (
            &["logbrew", "help", "latest", "logs"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "help", "recent", "issues"][..],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "help", "last", "action"][..],
            HelpTopic::ReadActions,
        ),
        (
            &["logbrew", "help", "newest", "release"][..],
            HelpTopic::ReadReleases,
        ),
        (
            &["logbrew", "latest", "help", "logs"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "recent", "help", "issues"][..],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "last", "help", "action"][..],
            HelpTopic::ReadActions,
        ),
        (
            &["logbrew", "newest", "help", "release"][..],
            HelpTopic::ReadReleases,
        ),
    ];

    for (args, topic) in cases {
        assert_help(args, topic, false);
    }
}

#[test]
fn parses_prefix_help_words_as_real_user_topics() {
    let cases = [
        (
            &["logbrew", "read", "help", "logs"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "read", "help", "issues"][..],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "read", "help", "trace"][..],
            HelpTopic::ReadTrace,
        ),
        (
            &["logbrew", "show", "help", "logs"][..],
            HelpTopic::ReadLogs,
        ),
        (
            &["logbrew", "list", "help", "issues"][..],
            HelpTopic::ReadIssues,
        ),
        (
            &["logbrew", "get", "help", "issue"][..],
            HelpTopic::ReadIssue,
        ),
        (&["logbrew", "logs", "help"][..], HelpTopic::ReadLogs),
        (&["logbrew", "issues", "help"][..], HelpTopic::ReadIssues),
        (&["logbrew", "actions", "help"][..], HelpTopic::ReadActions),
        (
            &["logbrew", "releases", "help"][..],
            HelpTopic::ReadReleases,
        ),
        (&["logbrew", "trace", "help"][..], HelpTopic::ReadTrace),
        (&["logbrew", "issue", "help"][..], HelpTopic::ReadIssue),
        (&["logbrew", "resolve", "help"][..], HelpTopic::Set),
        (&["logbrew", "close", "help"][..], HelpTopic::Set),
        (&["logbrew", "ignore", "help"][..], HelpTopic::Set),
        (&["logbrew", "reopen", "help"][..], HelpTopic::Set),
        (&["logbrew", "resolve", "issue", "help"][..], HelpTopic::Set),
        (&["logbrew", "close", "issue", "help"][..], HelpTopic::Set),
        (&["logbrew", "ignore", "issue", "help"][..], HelpTopic::Set),
        (&["logbrew", "reopen", "issue", "help"][..], HelpTopic::Set),
    ];

    for (args, topic) in cases {
        assert_help(args, topic, false);
    }
    assert_help(
        &["logbrew", "--json", "read", "help", "logs"],
        HelpTopic::ReadLogs,
        true,
    );
    assert_help(
        &["logbrew", "resolve", "help", "--json"],
        HelpTopic::Set,
        true,
    );
    assert!(
        parse_command(["logbrew", "resolve", "help", "issue_123"]).is_err(),
        "issue ids after shortcut help words remain errors"
    );
}

#[test]
fn parses_agent_login_handoff_forms_without_opening_a_browser() {
    for args in [
        &["logbrew", "login", "--no-open", "--json"][..],
        &["logbrew", "login", "--json"],
        &["logbrew", "--json", "login"][..],
        &["logbrew", "--json", "auth", "login"][..],
    ] {
        assert_command(
            args,
            Command::Login {
                provider: LoginProvider::GitHub,
                open_browser: false,
                json: true,
            },
        );
    }
}

#[test]
fn parses_login_provider_in_separate_or_inline_form() {
    for (args, provider) in [
        (
            &["logbrew", "login", "--provider", "github"][..],
            LoginProvider::GitHub,
        ),
        (
            &["logbrew", "login", "--provider=gitlab"][..],
            LoginProvider::GitLab,
        ),
        (
            &[
                "logbrew",
                "auth",
                "login",
                "--no-open",
                "--provider",
                "bitbucket",
            ][..],
            LoginProvider::Bitbucket,
        ),
        (
            &["logbrew", "--json", "auth", "login", "--provider=gitlab"][..],
            LoginProvider::GitLab,
        ),
    ] {
        let command = parse_command(args.iter().copied()).expect("provider login parses");

        assert_eq!(
            command,
            Command::Login {
                provider,
                open_browser: !args.contains(&"--no-open") && !args.contains(&"--json"),
                json: args.contains(&"--json"),
            }
        );
    }
}

#[test]
fn rejects_invalid_missing_or_duplicate_login_provider_without_echoing_values() {
    for args in [
        &["logbrew", "login", "--provider", "hostile\nsecret"][..],
        &["logbrew", "login", "--provider="],
        &["logbrew", "login", "--provider"],
        &[
            "logbrew",
            "login",
            "--provider",
            "github",
            "--provider=gitlab",
        ],
    ] {
        let error = parse_command(args.iter().copied()).expect_err("invalid provider fails closed");
        let mut output = Vec::new();
        logbrew_cli::write_cli_error(&error, true, &mut output).expect("parse error writes");
        let text = String::from_utf8(output).expect("utf8 parse error");

        assert!(!text.contains("hostile"));
        assert!(!text.contains("secret"));
    }
}

#[test]
fn parses_agent_friendly_read_actions() {
    assert_command_path(
        &[
            "logbrew",
            "read",
            "actions",
            "--name",
            "checkout_failed",
            "--since",
            "24h",
            "--json",
        ],
        read_command(
            ReadTarget::Actions,
            ReadOptions {
                name: Some("checkout_failed".to_owned()),
                since: Some("24h".to_owned()),
                ..ReadOptions::default()
            },
            true,
        ),
        "/api/telemetry/actions?name=checkout_failed&since=24h",
    );
}

#[test]
fn parses_common_incident_scope_filters_for_collection_reads() {
    for (resource, expected_path) in [
        ("logs", "/api/logs?service_name=checkout-api&since=24h"),
        (
            "issues",
            "/api/telemetry/issues?service_name=checkout-api&since=24h",
        ),
        (
            "actions",
            "/api/telemetry/actions?service_name=checkout-api&since=24h",
        ),
        (
            "releases",
            "/api/telemetry/releases?service_name=checkout-api&since=24h",
        ),
    ] {
        let command = parse_command([
            "logbrew",
            resource,
            "--service",
            "checkout-api",
            "--since",
            "24h",
            "--json",
        ])
        .expect("incident scope filters parse");

        assert_eq!(
            command.http_path().expect("collection read has endpoint"),
            expected_path
        );
    }

    let alias = parse_command([
        "logbrew",
        "issues",
        "--service-name",
        "checkout-api",
        "--since",
        "2026-05-01T00:00:00Z",
        "--json",
    ])
    .expect("backend-aligned service alias parses");
    assert_eq!(
        alias.http_path().expect("issue read has endpoint"),
        "/api/telemetry/issues?service_name=checkout-api&since=2026-05-01T00%3A00%3A00Z"
    );

    let duplicate = parse_command([
        "logbrew",
        "logs",
        "--service",
        "checkout-api",
        "--service-name",
        "payments-api",
    ])
    .expect_err("service aliases are one canonical filter");
    assert_eq!(duplicate.to_string(), "duplicate flag: --service");
}

#[test]
fn collection_help_documents_incident_scope_forms() {
    for topic in [
        HelpTopic::ReadLogs,
        HelpTopic::ReadIssues,
        HelpTopic::ReadActions,
        HelpTopic::ReadReleases,
    ] {
        let text = help::help_text(topic);

        assert!(text.contains("--service <service_name>"));
        assert!(text.contains("--service-name <service_name>"));
    }

    for topic in [HelpTopic::ReadIssues, HelpTopic::ReadReleases] {
        let text = help::help_text(topic);

        assert!(text.contains("--since <24h|7d|RFC3339>"));
        assert!(text.contains("2026-05-01T00:00:00Z"));
    }
}

#[test]
fn parses_action_name_shortcut_help_as_actions_help() {
    for args in [
        &["logbrew", "events", "checkout_failed", "--help"][..],
        &["logbrew", "action", "checkout_failed", "--help"],
        &["logbrew", "read", "events", "checkout_failed", "--help"],
        &["logbrew", "read", "action", "checkout_failed", "--help"],
    ] {
        let command = parse_command(args.iter().copied()).expect("action name help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadActions,
                json: false
            }
        );
    }

    for args in [
        &["logbrew", "help", "events", "checkout_failed", "--json"][..],
        &[
            "logbrew",
            "help",
            "read",
            "events",
            "checkout_failed",
            "--json",
        ],
        &[
            "logbrew",
            "help",
            "read",
            "action",
            "checkout_failed",
            "--json",
        ],
        &["logbrew", "events", "help", "checkout_failed", "--json"],
        &[
            "logbrew",
            "read",
            "events",
            "help",
            "checkout_failed",
            "--json",
        ],
    ] {
        let command =
            parse_command(args.iter().copied()).expect("action name explicit help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadActions,
                json: true
            }
        );
    }
}

#[test]
fn parses_read_filter_aliases_for_real_user_terms() {
    assert_command_path(
        &[
            "logbrew",
            "logs",
            "--env=production",
            "--project-id=checkout",
            "--trace-id=trace_123",
            "--json",
        ],
        read_command(
            ReadTarget::Logs,
            ReadOptions {
                trace: Some("trace_123".to_owned()),
                project: Some("checkout".to_owned()),
                environment: Some("production".to_owned()),
                ..ReadOptions::default()
            },
            true,
        ),
        "/api/logs?trace_id=trace_123&project_id=checkout&environment=production",
    );

    assert_command_path(
        &[
            "logbrew",
            "actions",
            "--distinct-id=user_123",
            "--env=production",
            "--json",
        ],
        read_command(
            ReadTarget::Actions,
            ReadOptions {
                user: Some("user_123".to_owned()),
                environment: Some("production".to_owned()),
                ..ReadOptions::default()
            },
            true,
        ),
        "/api/telemetry/actions?distinct_id=user_123&environment=production",
    );
}

#[test]
fn parses_common_read_filters_and_paths() {
    for (args, expected, path) in [
        (
            &[
                "logbrew",
                "read",
                "logs",
                "--release",
                "api@1.2.3",
                "--json",
            ][..],
            read_command(
                ReadTarget::Logs,
                ReadOptions {
                    release: Some("api@1.2.3".to_owned()),
                    ..ReadOptions::default()
                },
                true,
            ),
            "/api/logs?release=api%401.2.3",
        ),
        (
            &["logbrew", "read", "logs", "--limit", "25", "--json"][..],
            read_command(
                ReadTarget::Logs,
                ReadOptions {
                    limit: Some("25".to_owned()),
                    ..ReadOptions::default()
                },
                true,
            ),
            "/api/logs?limit=25",
        ),
        (
            &[
                "logbrew",
                "read",
                "releases",
                "--environment",
                "production",
                "--json",
            ][..],
            read_command(
                ReadTarget::Releases,
                ReadOptions {
                    environment: Some("production".to_owned()),
                    ..ReadOptions::default()
                },
                true,
            ),
            "/api/telemetry/releases?environment=production",
        ),
        (
            &[
                "logbrew",
                "releases",
                "--environment",
                "production",
                "--json",
            ][..],
            read_command(
                ReadTarget::Releases,
                ReadOptions {
                    environment: Some("production".to_owned()),
                    ..ReadOptions::default()
                },
                true,
            ),
            "/api/telemetry/releases?environment=production",
        ),
        (
            &["logbrew", "read", "trace", "trace-123", "--json"][..],
            read_command(
                ReadTarget::Trace("trace-123".to_owned()),
                ReadOptions::default(),
                true,
            ),
            "/api/telemetry/traces/trace-123",
        ),
    ] {
        assert_command_path(args, expected, path);
    }
}

#[test]
fn parses_common_normalized_read_paths() {
    for (args, path) in [
        (
            &[
                "logbrew",
                "logs",
                "--level=error",
                "--search=checkout failed",
                "--json",
            ][..],
            "/api/logs?severity=error&search=checkout%20failed",
        ),
        (
            &["logbrew", "logs", "--search=--timeout", "--json"][..],
            "/api/logs?search=--timeout",
        ),
        (
            &["logbrew", "logs", "--level", "WARNING", "--json"][..],
            "/api/logs?severity=warning",
        ),
        (
            &["logbrew", "issues", "--status", "Open", "--json"][..],
            "/api/telemetry/issues?status=unresolved",
        ),
        (
            &["logbrew", "issues", "--status", "Closed", "--json"][..],
            "/api/telemetry/issues?status=resolved",
        ),
    ] {
        assert_path(args, path);
    }
}

#[test]
fn accepts_legacy_log_level_alias_inputs_as_canonical_filters() {
    for (alias, canonical) in [
        ("trace", "info"),
        ("debug", "info"),
        ("information", "info"),
        ("warn", "warning"),
        ("err", "error"),
        ("fatal", "critical"),
    ] {
        assert_path(
            &["logbrew", "logs", "--severity", alias, "--json"],
            format!("/api/logs?severity={canonical}").as_str(),
        );
    }
}

#[test]
fn parses_json_before_read_resource_and_detail_id() {
    for (args, target) in [
        (&["logbrew", "read", "--json", "logs"][..], ReadTarget::Logs),
        (
            &["logbrew", "read", "trace", "--json", "trace-123"][..],
            ReadTarget::Trace("trace-123".to_owned()),
        ),
        (
            &["logbrew", "read", "issue", "--json", "issue_123"][..],
            ReadTarget::Issue("issue_123".to_owned()),
        ),
    ] {
        assert_command(args, read_command(target, ReadOptions::default(), true));
    }
}

#[test]
fn parses_explain_trace_for_agent_context() {
    let command =
        parse_command(["logbrew", "explain", "trace", "trace-123", "--json"]).expect("command");

    assert_eq!(
        command,
        Command::Explain {
            target: logbrew_cli::ExplainTarget::Trace("trace-123".to_owned()),
            json: true,
        }
    );
    assert_eq!(
        command.http_path().expect("explain trace has endpoint"),
        "/api/telemetry/traces/trace-123/investigation"
    );
}

#[test]
fn parses_log_release_and_metric_explanations() {
    let log = parse_command([
        "logbrew",
        "explain",
        "log",
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "--json",
    ])
    .expect("log explanation parses");
    assert_eq!(
        log.http_path().expect("log explanation has endpoint"),
        "/api/logs/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa/investigation"
    );

    let release = parse_command([
        "logbrew",
        "explain",
        "release",
        "checkout@1.2.3",
        "--project",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
        "--environment",
        "production",
        "--service",
        "checkout-api",
    ])
    .expect("release explanation parses");
    assert_eq!(
        release
            .http_path()
            .expect("release explanation has endpoint"),
        "/api/telemetry/releases/investigation?project_id=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa&release=checkout%401.2.3&environment=production&service_name=checkout-api&response_version=3"
    );

    let metric = parse_command([
        "logbrew",
        "explain",
        "metric",
        "http.server.duration",
        "--project",
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "--since",
        "24h",
        "--interval",
        "5m",
        "--group-by",
        "service",
        "--environment",
        "production",
        "--series-limit",
        "12",
        "--json",
    ])
    .expect("metric explanation parses");
    assert_eq!(
        metric.http_path().expect("metric explanation has endpoint"),
        "/api/telemetry/metrics/investigation?project_id=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa&name=http.server.duration&since=24h&interval=5m&group_by=service_name&environment=production&series_limit=12&response_version=2"
    );
}

#[test]
fn rejects_malformed_explicit_explanation_identifiers_before_network_use() {
    for args in [
        &["logbrew", "explain", "issue", "issue_"][..],
        &["logbrew", "explain", "log", "not-a-uuid"][..],
        &["logbrew", "explain", "trace", "not-a-trace"][..],
    ] {
        assert!(
            parse_command(args.iter().copied()).is_err(),
            "malformed explanation identifier must fail locally"
        );
    }
}

#[test]
fn parses_json_before_explain_resource_and_id() {
    let inferred =
        parse_command(["logbrew", "explain", "--json", "issue_123"]).expect("inferred explain");
    assert_eq!(
        inferred,
        Command::Explain {
            target: logbrew_cli::ExplainTarget::Issue {
                id: "issue_123".to_owned(),
                occurrence: logbrew_cli::IssueOccurrenceSelection::Recommended,
            },
            json: true,
        }
    );

    let trace =
        parse_command(["logbrew", "explain", "trace", "--json", "trace-123"]).expect("trace");
    assert_eq!(
        trace,
        Command::Explain {
            target: logbrew_cli::ExplainTarget::Trace("trace-123".to_owned()),
            json: true,
        }
    );
}

#[test]
fn parses_issue_status_mutation() {
    let command = parse_command(["logbrew", "set", "issue", "issue-123", "RESOLVED", "--json"])
        .expect("command parses");

    assert_eq!(
        command,
        Command::Set {
            target: SetTarget::IssueStatus {
                id: "issue-123".to_owned(),
                status: "resolved".to_owned(),
            },
            json: true,
        }
    );
    assert_eq!(
        command.http_path().expect("set issue has endpoint"),
        "/api/telemetry/issues/issue-123"
    );

    let closed = parse_command(["logbrew", "set", "issue", "issue-123", "closed", "--json"])
        .expect("closed status alias parses");

    assert_eq!(
        closed,
        Command::Set {
            target: SetTarget::IssueStatus {
                id: "issue-123".to_owned(),
                status: "resolved".to_owned(),
            },
            json: true,
        }
    );
}

#[test]
fn parses_json_before_set_resource_id_and_status() {
    for args in [
        ["logbrew", "set", "--json", "issue", "issue-123", "resolved"],
        ["logbrew", "set", "issue", "--json", "issue-123", "resolved"],
        ["logbrew", "set", "issue", "issue-123", "--json", "resolved"],
    ] {
        let command = parse_command(args).expect("set parses");

        assert_eq!(
            command,
            Command::Set {
                target: SetTarget::IssueStatus {
                    id: "issue-123".to_owned(),
                    status: "resolved".to_owned(),
                },
                json: true,
            }
        );
    }
}

#[test]
fn parses_setup_auto_yes() {
    let command =
        parse_command(["logbrew", "setup", "--auto", "--yes", "--json"]).expect("command");

    assert_eq!(
        command,
        Command::Setup {
            auto: true,
            yes: true,
            json: true
        }
    );
    assert!(command.http_path().is_none());
}
