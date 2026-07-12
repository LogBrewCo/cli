//! CLI command grammar tests.

use logbrew_cli::{
    Command, HelpTopic, ProjectSetupSeenOptions, ReadOptions, ReadTarget, SetTarget, help,
    parse_command,
};

#[test]
fn parses_root_help_for_real_user_discovery() {
    let command = parse_command(["logbrew", "--help"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::Root,
            json: false
        }
    );
}

#[test]
fn parses_bare_invocation_as_root_help() {
    let command = parse_command(["logbrew"]).expect("bare invocation shows help");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::Root,
            json: false
        }
    );
}

#[test]
fn parses_top_level_json_as_root_help_for_agents() {
    let command = parse_command(["logbrew", "--json"]).expect("top-level json shows help");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::Root,
            json: true
        }
    );
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
        let command = parse_command(args.iter().copied()).expect("examples help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Examples,
                json: false
            }
        );
    }

    assert_eq!(
        parse_command(["logbrew", "--json", "examples"]).expect("global json examples parses"),
        Command::Help {
            topic: HelpTopic::Examples,
            json: true
        }
    );
}

#[test]
fn parses_global_json_before_command_for_agents() {
    let command = parse_command(["logbrew", "--json", "status"]).expect("command parses");

    assert_eq!(command, Command::Status { json: true });
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
        let command = parse_command(args.iter().copied()).expect("health alias parses");

        assert_eq!(
            command,
            Command::Status {
                json: args.contains(&"--json")
            }
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
        let command = parse_command(args.iter().copied()).expect("health help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Status,
                json: false
            }
        );
    }
}

#[test]
fn parses_whoami_and_me_as_status_aliases() {
    for args in [
        &["logbrew", "whoami"][..],
        &["logbrew", "me"],
        &["logbrew", "whoami", "--json"],
        &["logbrew", "--json", "me"],
    ] {
        let command = parse_command(args.iter().copied()).expect("status alias parses");

        assert_eq!(
            command,
            Command::Status {
                json: args.contains(&"--json")
            }
        );
    }

    for args in [
        &["logbrew", "whoami", "--help"][..],
        &["logbrew", "help", "me"],
    ] {
        let command = parse_command(args.iter().copied()).expect("status alias help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Status,
                json: false
            }
        );
    }
}

#[test]
fn parses_global_json_before_read_shortcut_for_agents() {
    let command =
        parse_command(["logbrew", "--json", "logs", "--release", "checkout@1"]).expect("command");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: Some("checkout@1".to_owned()),
                environment: None,
                status: None,
                limit: None,
            }),
            json: true,
        }
    );
}

#[test]
fn status_help_advertises_identity_aliases() {
    let text = help::help_text(HelpTopic::Status);

    assert!(text.contains("logbrew whoami [--json]"));
    assert!(text.contains("logbrew me [--json]"));
    assert!(text.contains("logbrew auth status [--json]"));
    assert!(text.contains("Identity aliases: logbrew whoami, logbrew me, logbrew auth status."));
}

#[test]
fn login_help_explains_json_handoff_without_browser() {
    let text = help::help_text(HelpTopic::Login);

    assert!(text.contains("stores a private local access/refresh pair"));
    assert!(text.contains("refresh local auth once after an expired-token response"));
    assert!(text.contains("--json prints the auth handoff without opening a browser."));
}

#[test]
fn watch_help_explains_websocket_ticket_flow() {
    let text = help::help_text(HelpTopic::Watch);

    assert!(text.contains("logbrew watch --json"));
    assert!(text.contains("logbrew watch issues [--json]"));
    assert!(text.contains("logbrew watch --severity error,critical --json"));
    assert!(text.contains("Aliases: tail, follow, and stream use the same live watch flow."));
    assert!(text.contains("Live watch uses a short-lived feed ticket and WebSocket stream."));
    assert!(text.contains("Transient disconnects reconnect with a fresh ticket and backoff."));
}

#[test]
fn explain_help_explains_pasted_id_inference() {
    let text = help::help_text(HelpTopic::Explain);

    assert!(text.contains("logbrew explain <issue_id_or_trace_id> [--json]"));
    assert!(text.contains("logbrew issue <issue_id> explain [--json]"));
    assert!(text.contains("logbrew trace <trace_id> explain [--json]"));
    assert!(text.contains("logbrew <issue_id_or_trace_id> explain [--json]"));
    assert!(text.contains("Pasted UUID/issue_* values are treated as issues"));
    assert!(text.contains("32-hex/trace_* values are treated as traces"));
}

#[test]
fn parses_read_logs_help_as_agent_friendly_topic() {
    let command = parse_command(["logbrew", "read", "logs", "--help"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::ReadLogs,
            json: false
        }
    );
}

#[test]
fn parses_help_read_logs_as_real_user_topic() {
    let command = parse_command(["logbrew", "help", "read", "logs"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::ReadLogs,
            json: false
        }
    );
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
        let command = parse_command(args.iter().copied()).expect("singular list help parses");

        assert_eq!(command, Command::Help { topic, json: true });
    }
    let command =
        parse_command(["logbrew", "get", "issue", "--help", "--json"]).expect("get issue help");
    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::ReadIssue,
            json: true
        }
    );
}

#[test]
fn parses_help_logs_as_real_user_shortcut_topic() {
    let command = parse_command(["logbrew", "help", "logs"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::ReadLogs,
            json: false
        }
    );
}

#[test]
fn parses_help_releases_json_as_agent_shortcut_topic() {
    let command = parse_command(["logbrew", "help", "releases", "--json"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::ReadReleases,
            json: true
        }
    );
}

#[test]
fn parses_common_help_terms_as_real_user_topics() {
    for (args, topic) in [
        (
            ["logbrew", "help", "traces", "--json"],
            HelpTopic::ReadTrace,
        ),
        (["logbrew", "help", "spans", "--json"], HelpTopic::ReadTrace),
        (
            ["logbrew", "help", "errors", "--json"],
            HelpTopic::ReadIssues,
        ),
        (
            ["logbrew", "help", "action", "--json"],
            HelpTopic::ReadActions,
        ),
        (
            ["logbrew", "help", "events", "--json"],
            HelpTopic::ReadActions,
        ),
        (
            ["logbrew", "help", "environments", "--json"],
            HelpTopic::Read,
        ),
        (["logbrew", "help", "filters", "--json"], HelpTopic::Read),
        (["logbrew", "help", "filter", "--json"], HelpTopic::Read),
        (
            ["logbrew", "help", "project", "--json"],
            HelpTopic::Projects,
        ),
        (
            ["logbrew", "help", "projects", "--json"],
            HelpTopic::Projects,
        ),
        (["logbrew", "help", "usage", "--json"], HelpTopic::Usage),
        (["logbrew", "help", "project-id", "--json"], HelpTopic::Read),
    ] {
        let command = parse_command(args).expect("help parses");

        assert_eq!(command, Command::Help { topic, json: true });
    }
    assert_eq!(
        parse_command(["logbrew", "action", "--help"]).expect("action shortcut help parses"),
        Command::Help {
            topic: HelpTopic::ReadActions,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "traces", "--help"]).expect("trace shortcut help parses"),
        Command::Help {
            topic: HelpTopic::ReadTrace,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "help", "read", "traces"]).expect("read trace help parses"),
        Command::Help {
            topic: HelpTopic::ReadTrace,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "help", "read", "action"]).expect("read action help parses"),
        Command::Help {
            topic: HelpTopic::ReadActions,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "show", "logs", "--help"]).expect("show logs help parses"),
        Command::Help {
            topic: HelpTopic::ReadLogs,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "help", "get", "issue"]).expect("get issue help parses"),
        Command::Help {
            topic: HelpTopic::ReadIssue,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "filters", "--help"]).expect("filters help parses"),
        Command::Help {
            topic: HelpTopic::Read,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "project", "--help"]).expect("project help parses"),
        Command::Help {
            topic: HelpTopic::Projects,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "help", "read", "project"]).expect("read project help parses"),
        Command::Help {
            topic: HelpTopic::Read,
            json: false
        }
    );
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
        let command = parse_command(args).expect("filter discovery help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Read,
                json: true
            }
        );
    }
}

#[test]
fn parses_project_and_usage_terms_as_backend_owned_help() {
    for args in [
        &["logbrew", "project", "--json"][..],
        &["logbrew", "projects", "--json"],
        &["logbrew", "--json", "projects"],
        &["logbrew", "projects", "create", "checkout", "--json"],
    ] {
        let command = parse_command(args.iter().copied()).expect("project discovery help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Projects,
                json: true
            }
        );
    }

    for args in [
        &["logbrew", "usage", "--json"][..],
        &["logbrew", "--json", "usage"],
        &["logbrew", "account", "usage", "--json"],
    ] {
        let command = parse_command(args.iter().copied()).expect("usage discovery help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Usage,
                json: true
            }
        );
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
fn parses_bare_trace_terms_as_top_level_discovery_help() {
    for args in [
        &["logbrew", "trace", "--json"][..],
        &["logbrew", "traces", "--json"],
        &["logbrew", "span", "--json"],
        &["logbrew", "spans", "--json"],
        &["logbrew", "traces"],
        &["logbrew", "--json", "spans"],
    ] {
        let command = parse_command(args.iter().copied()).expect("trace discovery help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::ReadTrace,
                json: args.contains(&"--json")
            }
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
        let command = parse_command(args.iter().copied()).expect("auth help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Auth,
                json: false
            }
        );
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

    assert_eq!(
        parse_command(["logbrew", "auth", "--json"]).expect("auth json help parses"),
        Command::Help {
            topic: HelpTopic::Auth,
            json: true
        }
    );
    assert_eq!(
        parse_command(["logbrew", "--json", "auth"]).expect("global json auth help parses"),
        Command::Help {
            topic: HelpTopic::Auth,
            json: true
        }
    );
    assert_eq!(
        parse_command(["logbrew", "--json", "token"]).expect("global json token help parses"),
        Command::Help {
            topic: HelpTopic::Auth,
            json: true
        }
    );
}

#[test]
fn parses_auth_namespace_as_token_safe_command_aliases() {
    for args in [
        &["logbrew", "auth", "status"][..],
        &["logbrew", "auth", "whoami"],
        &["logbrew", "auth", "me"],
    ] {
        let command = parse_command(args.iter().copied()).expect("auth status alias parses");

        assert_eq!(command, Command::Status { json: false });
    }

    for args in [
        &["logbrew", "auth", "status", "--json"][..],
        &["logbrew", "--json", "auth", "status"],
        &["logbrew", "auth", "--json", "status"],
        &["logbrew", "auth", "whoami", "--json"],
    ] {
        let command = parse_command(args.iter().copied()).expect("auth json status alias parses");

        assert_eq!(command, Command::Status { json: true });
    }

    assert_eq!(
        parse_command(["logbrew", "auth", "login", "--no-open"]).expect("auth login parses"),
        Command::Login {
            open_browser: false,
            json: false
        }
    );
    assert_eq!(
        parse_command(["logbrew", "auth", "login", "--json"]).expect("auth login json parses"),
        Command::Login {
            open_browser: false,
            json: true
        }
    );
    assert_eq!(
        parse_command(["logbrew", "auth", "--json", "login"]).expect("auth json login parses"),
        Command::Login {
            open_browser: false,
            json: true
        }
    );
    assert_eq!(
        parse_command(["logbrew", "auth", "logout", "--json"]).expect("auth logout parses"),
        Command::Logout { json: true }
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
        let command = parse_command(args.iter().copied()).expect("auth subcommand help parses");

        assert_eq!(command, Command::Help { topic, json: false });
    }

    assert_eq!(
        parse_command(["logbrew", "auth", "help", "status", "--json"])
            .expect("auth help status json parses"),
        Command::Help {
            topic: HelpTopic::Status,
            json: true
        }
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
        let command = parse_command(args.iter().copied()).expect("json help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Json,
                json: false
            }
        );
    }

    let text = help::help_text(HelpTopic::Json);
    assert!(text.contains("logbrew --json status"));
    assert!(text.contains("logbrew status --json"));
    assert!(text.contains("Stable JSON keeps server response shapes"));
    assert!(text.contains("Errors include ok, error, message, and next."));

    assert_eq!(
        parse_command(["logbrew", "--json", "help", "json"]).expect("global json help parses"),
        Command::Help {
            topic: HelpTopic::Json,
            json: true
        }
    );
}

#[test]
fn parses_subcommand_resource_help_as_real_user_topics() {
    let cases = [
        (vec!["logbrew", "watch", "logs", "--help"], HelpTopic::Watch),
        (
            vec!["logbrew", "help", "watch", "actions"],
            HelpTopic::Watch,
        ),
        (vec!["logbrew", "watch", "help", "logs"], HelpTopic::Watch),
        (
            vec!["logbrew", "watch", "event", "--help"],
            HelpTopic::Watch,
        ),
        (vec!["logbrew", "help", "watch", "events"], HelpTopic::Watch),
        (
            vec!["logbrew", "explain", "trace", "--help"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "help", "explain", "issue"],
            HelpTopic::Explain,
        ),
        (
            vec!["logbrew", "explain", "help", "trace"],
            HelpTopic::Explain,
        ),
        (vec!["logbrew", "set", "issue", "--help"], HelpTopic::Set),
        (vec!["logbrew", "help", "set", "issue"], HelpTopic::Set),
        (vec!["logbrew", "set", "help", "issue"], HelpTopic::Set),
        (vec!["logbrew", "help", "resolve", "issue"], HelpTopic::Set),
        (vec!["logbrew", "help", "close", "issue"], HelpTopic::Set),
        (vec!["logbrew", "help", "ignore", "issue"], HelpTopic::Set),
        (vec!["logbrew", "help", "reopen", "issue"], HelpTopic::Set),
    ];

    for (args, topic) in cases {
        let command = parse_command(args).expect("help parses");

        assert_eq!(command, Command::Help { topic, json: false });
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
        let command = parse_command(args).expect("watch alias help parses");

        assert_eq!(
            command,
            Command::Help {
                topic: HelpTopic::Watch,
                json: false
            }
        );
    }
}

#[test]
fn parses_recency_read_help_as_real_user_topics() {
    let cases = [
        (
            vec!["logbrew", "latest", "logs", "--help"],
            HelpTopic::ReadLogs,
        ),
        (
            vec!["logbrew", "recent", "issues", "--help"],
            HelpTopic::ReadIssues,
        ),
        (
            vec!["logbrew", "last", "action", "checkout_failed", "--help"],
            HelpTopic::ReadActions,
        ),
        (
            vec!["logbrew", "newest", "release", "--help"],
            HelpTopic::ReadReleases,
        ),
        (
            vec!["logbrew", "help", "latest", "logs"],
            HelpTopic::ReadLogs,
        ),
        (
            vec!["logbrew", "help", "recent", "issues"],
            HelpTopic::ReadIssues,
        ),
        (
            vec!["logbrew", "help", "last", "action"],
            HelpTopic::ReadActions,
        ),
        (
            vec!["logbrew", "help", "newest", "release"],
            HelpTopic::ReadReleases,
        ),
        (
            vec!["logbrew", "latest", "help", "logs"],
            HelpTopic::ReadLogs,
        ),
        (
            vec!["logbrew", "recent", "help", "issues"],
            HelpTopic::ReadIssues,
        ),
        (
            vec!["logbrew", "last", "help", "action"],
            HelpTopic::ReadActions,
        ),
        (
            vec!["logbrew", "newest", "help", "release"],
            HelpTopic::ReadReleases,
        ),
    ];

    for (args, topic) in cases {
        let command = parse_command(args).expect("recency read help parses");

        assert_eq!(command, Command::Help { topic, json: false });
    }
}

#[test]
fn parses_prefix_help_words_as_real_user_topics() {
    let cases = [
        (vec!["logbrew", "read", "help", "logs"], HelpTopic::ReadLogs),
        (
            vec!["logbrew", "read", "help", "issues"],
            HelpTopic::ReadIssues,
        ),
        (
            vec!["logbrew", "read", "help", "trace"],
            HelpTopic::ReadTrace,
        ),
        (vec!["logbrew", "show", "help", "logs"], HelpTopic::ReadLogs),
        (
            vec!["logbrew", "list", "help", "issues"],
            HelpTopic::ReadIssues,
        ),
        (
            vec!["logbrew", "get", "help", "issue"],
            HelpTopic::ReadIssue,
        ),
        (vec!["logbrew", "logs", "help"], HelpTopic::ReadLogs),
        (vec!["logbrew", "issues", "help"], HelpTopic::ReadIssues),
        (vec!["logbrew", "actions", "help"], HelpTopic::ReadActions),
        (vec!["logbrew", "releases", "help"], HelpTopic::ReadReleases),
        (vec!["logbrew", "trace", "help"], HelpTopic::ReadTrace),
        (vec!["logbrew", "issue", "help"], HelpTopic::ReadIssue),
        (vec!["logbrew", "resolve", "help"], HelpTopic::Set),
        (vec!["logbrew", "close", "help"], HelpTopic::Set),
        (vec!["logbrew", "ignore", "help"], HelpTopic::Set),
        (vec!["logbrew", "reopen", "help"], HelpTopic::Set),
        (vec!["logbrew", "resolve", "issue", "help"], HelpTopic::Set),
        (vec!["logbrew", "close", "issue", "help"], HelpTopic::Set),
        (vec!["logbrew", "ignore", "issue", "help"], HelpTopic::Set),
        (vec!["logbrew", "reopen", "issue", "help"], HelpTopic::Set),
    ];

    for (args, topic) in cases {
        let command = parse_command(args).expect("prefix help parses");

        assert_eq!(command, Command::Help { topic, json: false });
    }

    assert_eq!(
        parse_command(["logbrew", "--json", "read", "help", "logs"])
            .expect("global json prefix help parses"),
        Command::Help {
            topic: HelpTopic::ReadLogs,
            json: true
        }
    );
    assert_eq!(
        parse_command(["logbrew", "resolve", "help", "--json"])
            .expect("shortcut prefix help json parses"),
        Command::Help {
            topic: HelpTopic::Set,
            json: true
        }
    );
    assert!(
        parse_command(["logbrew", "resolve", "help", "issue_123"]).is_err(),
        "issue ids after shortcut help words remain errors"
    );
}

#[test]
fn parses_login_no_open_for_agent_auth_handoff() {
    let command = parse_command(["logbrew", "login", "--no-open", "--json"]).expect("command");

    assert_eq!(
        command,
        Command::Login {
            open_browser: false,
            json: true
        }
    );
}

#[test]
fn parses_login_json_as_agent_auth_handoff_without_browser() {
    let command = parse_command(["logbrew", "login", "--json"]).expect("command");

    assert_eq!(
        command,
        Command::Login {
            open_browser: false,
            json: true
        }
    );
}

#[test]
fn parses_global_json_login_as_agent_auth_handoff_without_browser() {
    for args in [
        &["logbrew", "--json", "login"][..],
        &["logbrew", "--json", "auth", "login"][..],
    ] {
        let command = parse_command(args.iter().copied()).expect("command");

        assert_eq!(
            command,
            Command::Login {
                open_browser: false,
                json: true
            }
        );
    }
}

#[test]
fn parses_read_actions_help_with_json_flag() {
    let command =
        parse_command(["logbrew", "read", "actions", "--help", "--json"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::ReadActions,
            json: true
        }
    );
}

#[test]
fn parses_top_level_releases_help_as_real_user_shortcut() {
    let command = parse_command(["logbrew", "releases", "--help"]).expect("help parses");

    assert_eq!(
        command,
        Command::Help {
            topic: HelpTopic::ReadReleases,
            json: false
        }
    );
}

#[test]
fn parses_agent_friendly_read_actions() {
    let command = parse_command([
        "logbrew",
        "read",
        "actions",
        "--name",
        "checkout_failed",
        "--since",
        "24h",
        "--json",
    ])
    .expect("command parses");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: Some("checkout_failed".to_owned()),
                service: None,
                since: Some("24h".to_owned()),
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
                environment: None,
                status: None,
                limit: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        command.http_path().expect("read actions has endpoint"),
        "/api/telemetry/actions?name=checkout_failed&since=24h"
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
    let logs = parse_command([
        "logbrew",
        "logs",
        "--env=production",
        "--project-id=checkout",
        "--trace-id=trace_123",
        "--json",
    ])
    .expect("alias filters parse");

    assert_eq!(
        logs,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: Some("trace_123".to_owned()),
                level: None,
                search: None,
                project: Some("checkout".to_owned()),
                release: None,
                environment: Some("production".to_owned()),
                status: None,
                limit: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        logs.http_path().expect("read logs has endpoint"),
        "/api/logs?trace_id=trace_123&project_id=checkout&environment=production"
    );

    let actions = parse_command([
        "logbrew",
        "actions",
        "--distinct-id=user_123",
        "--env=production",
        "--json",
    ])
    .expect("action alias filters parse");

    assert_eq!(
        actions,
        Command::Read {
            target: ReadTarget::Actions,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: Some("user_123".to_owned()),
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
                environment: Some("production".to_owned()),
                status: None,
                limit: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        actions.http_path().expect("read actions has endpoint"),
        "/api/telemetry/actions?distinct_id=user_123&environment=production"
    );
}

#[test]
fn parses_release_filter_for_logs() {
    let command = parse_command([
        "logbrew",
        "read",
        "logs",
        "--release",
        "api@1.2.3",
        "--json",
    ])
    .expect("command parses");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: Some("api@1.2.3".to_owned()),
                environment: None,
                status: None,
                limit: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        command.http_path().expect("read logs has endpoint"),
        "/api/logs?release=api%401.2.3"
    );
}

#[test]
fn parses_positive_limit_for_logs() {
    let command =
        parse_command(["logbrew", "read", "logs", "--limit", "25", "--json"]).expect("command");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::new(ReadOptions {
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
                limit: Some("25".to_owned()),
            }),
            json: true,
        }
    );
    assert_eq!(
        command.http_path().expect("read logs has endpoint"),
        "/api/logs?limit=25"
    );
}

#[test]
fn parses_log_level_and_search_filters_for_noisy_logs() {
    let command = parse_command([
        "logbrew",
        "logs",
        "--level=error",
        "--search=checkout failed",
        "--json",
    ])
    .expect("command parses");

    assert_eq!(
        command.http_path().expect("read logs has endpoint"),
        "/api/logs?severity=error&search=checkout%20failed"
    );
}

#[test]
fn parses_inline_search_value_that_looks_like_a_flag() {
    let command =
        parse_command(["logbrew", "logs", "--search=--timeout", "--json"]).expect("command parses");

    assert_eq!(
        command.http_path().expect("read logs has endpoint"),
        "/api/logs?search=--timeout"
    );
}

#[test]
fn normalizes_human_log_level_aliases() {
    let command =
        parse_command(["logbrew", "logs", "--level", "WARNING", "--json"]).expect("command parses");

    assert_eq!(
        command.http_path().expect("read logs has endpoint"),
        "/api/logs?severity=warning"
    );
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
        let command = parse_command(["logbrew", "logs", "--severity", alias, "--json"])
            .expect("command parses");

        assert_eq!(
            command.http_path().expect("read logs has endpoint"),
            format!("/api/logs?severity={canonical}")
        );
    }
}

#[test]
fn normalizes_case_insensitive_issue_status_aliases() {
    let command =
        parse_command(["logbrew", "issues", "--status", "Open", "--json"]).expect("command");

    assert_eq!(
        command.http_path().expect("read issues has endpoint"),
        "/api/telemetry/issues?status=unresolved"
    );

    let closed =
        parse_command(["logbrew", "issues", "--status", "Closed", "--json"]).expect("command");

    assert_eq!(
        closed.http_path().expect("read issues has endpoint"),
        "/api/telemetry/issues?status=resolved"
    );
}

#[test]
fn parses_release_summaries_with_environment_filter() {
    let command = parse_command([
        "logbrew",
        "read",
        "releases",
        "--environment",
        "production",
        "--json",
    ])
    .expect("command parses");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Releases,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
                environment: Some("production".to_owned()),
                status: None,
                limit: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        command.http_path().expect("read releases has endpoint"),
        "/api/telemetry/releases?environment=production"
    );
}

#[test]
fn parses_top_level_releases_shortcut_with_environment_filter() {
    let command = parse_command([
        "logbrew",
        "releases",
        "--environment",
        "production",
        "--json",
    ])
    .expect("command parses");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Releases,
            options: Box::new(ReadOptions {
                name: None,
                service: None,
                since: None,
                user: None,
                trace: None,
                level: None,
                search: None,
                project: None,
                release: None,
                environment: Some("production".to_owned()),
                status: None,
                limit: None,
            }),
            json: true,
        }
    );
    assert_eq!(
        command
            .http_path()
            .expect("top-level releases has endpoint"),
        "/api/telemetry/releases?environment=production"
    );
}

#[test]
fn parses_read_trace_as_singular_target() {
    let command =
        parse_command(["logbrew", "read", "trace", "trace-123", "--json"]).expect("command");

    assert_eq!(
        command,
        Command::Read {
            target: ReadTarget::Trace("trace-123".to_owned()),
            options: Box::new(ReadOptions {
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
            }),
            json: true,
        }
    );
    assert_eq!(
        command.http_path().expect("read trace has endpoint"),
        "/api/telemetry/traces/trace-123"
    );
}

#[test]
fn parses_json_before_read_resource_and_detail_id() {
    let read_logs = parse_command(["logbrew", "read", "--json", "logs"]).expect("read parses");
    assert_eq!(
        read_logs,
        Command::Read {
            target: ReadTarget::Logs,
            options: Box::default(),
            json: true,
        }
    );

    let trace =
        parse_command(["logbrew", "read", "trace", "--json", "trace-123"]).expect("trace parses");
    assert_eq!(
        trace,
        Command::Read {
            target: ReadTarget::Trace("trace-123".to_owned()),
            options: Box::default(),
            json: true,
        }
    );

    let issue =
        parse_command(["logbrew", "read", "issue", "--json", "issue_123"]).expect("issue parses");
    assert_eq!(
        issue,
        Command::Read {
            target: ReadTarget::Issue("issue_123".to_owned()),
            options: Box::default(),
            json: true,
        }
    );
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
        "/api/telemetry/traces/trace-123"
    );
}

#[test]
fn parses_json_before_explain_resource_and_id() {
    let inferred =
        parse_command(["logbrew", "explain", "--json", "issue_123"]).expect("inferred explain");
    assert_eq!(
        inferred,
        Command::Explain {
            target: logbrew_cli::ExplainTarget::Issue("issue_123".to_owned()),
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
