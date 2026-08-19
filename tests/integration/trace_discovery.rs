//! Recent trace discovery command and rendering contracts.

use logbrew_cli::{
    CliEnvironment, Command, execute_command, help, parse_command, write_cli_error,
    write_runtime_error,
};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

#[test]
fn parses_recent_trace_discovery_with_exact_query_keys() {
    let command = parse_command([
        "logbrew",
        "traces",
        "--project-id",
        PROJECT_ID,
        "--service-name",
        "checkout-api",
        "--release",
        "checkout@1.2.3",
        "--env",
        "production",
        "--status",
        "ERROR",
        "--since",
        "24h",
        "--min-duration-ms",
        "500",
        "--limit",
        "25",
        "--json",
    ])
    .expect("trace discovery parses");

    assert_eq!(
        command.http_path().expect("trace discovery has endpoint"),
        "/api/telemetry/traces?project_id=123e4567-e89b-12d3-a456-426614174000&service_name=checkout-api&release=checkout%401.2.3&environment=production&status=error&since=24h&min_duration_ms=500&limit=25"
    );

    for args in [
        &[
            "logbrew",
            "read",
            "traces",
            "--since",
            "2026-05-01T00:00:00Z",
            "--json",
        ][..],
        &["logbrew", "spans", "--min-duration-ms=0", "--json"],
        &["logbrew", "latest", "5", "traces", "--json"],
    ] {
        let command = parse_command(args.iter().copied()).expect("trace alias parses");
        let path = command.http_path().expect("trace alias has endpoint");

        assert!(path.starts_with("/api/telemetry/traces"));
    }

    let timestamp = parse_command([
        "logbrew",
        "read",
        "traces",
        "--since",
        "2026-05-01T00:00:00Z",
        "--json",
    ])
    .expect("RFC3339 trace discovery parses");
    assert_eq!(
        timestamp.http_path().expect("trace discovery has endpoint"),
        "/api/telemetry/traces?since=2026-05-01T00%3A00%3A00Z"
    );

    let recent = parse_command(["logbrew", "latest", "5", "traces", "--json"])
        .expect("recency trace discovery parses");
    assert_eq!(
        recent.http_path().expect("trace discovery has endpoint"),
        "/api/telemetry/traces?limit=5"
    );
}

#[test]
fn trace_discovery_help_is_distinct_from_trace_detail_help() {
    let list = parse_command(["logbrew", "traces", "--help"]).expect("trace list help parses");
    let detail = parse_command(["logbrew", "trace", "--help"]).expect("trace detail help parses");

    let Command::Help {
        topic: list_topic, ..
    } = list
    else {
        panic!("plural traces should open list help");
    };
    let Command::Help {
        topic: detail_topic,
        ..
    } = detail
    else {
        panic!("singular trace should open detail help");
    };
    let list_text = help::help_text(list_topic);
    let detail_text = help::help_text(detail_topic);

    assert!(list_text.contains("logbrew traces"));
    assert!(list_text.contains("--status <error|ok>"));
    assert!(list_text.contains("--min-duration-ms <milliseconds>"));
    assert!(list_text.contains("--since <24h|7d|RFC3339>"));
    assert!(detail_text.contains("logbrew trace <trace_id>"));
    assert!(!detail_text.contains("--min-duration-ms"));
}

#[test]
fn plural_trace_help_routes_non_ids_to_discovery_guidance() {
    for args in [
        &["logbrew", "traces", "checkout-api", "--help"][..],
        &["logbrew", "read", "traces", "checkout-api", "--help"],
        &["logbrew", "show", "traces", "checkout-api", "--help"],
    ] {
        let command = parse_command(args.iter().copied()).expect("trace list help parses");
        let Command::Help { topic, .. } = command else {
            panic!("non-id plural trace help should remain discovery help");
        };

        assert_eq!(topic, logbrew_cli::HelpTopic::ReadTraces);
    }

    let copied_id = parse_command(["logbrew", "traces", "trace_123", "--help"])
        .expect("copied trace id help parses");
    assert_eq!(
        copied_id,
        Command::Help {
            topic: logbrew_cli::HelpTopic::ReadTrace,
            json: false,
        }
    );
}

#[tokio::test]
async fn trace_discovery_preserves_bare_json_and_renders_human_triage()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let response = serde_json::json!([{
        "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
        "project_ids": [PROJECT_ID],
        "root_span_name": "POST /checkout",
        "root_service_name": "checkout-api",
        "root_operation": "http.server",
        "span_count": 12,
        "error_span_count": 2,
        "service_count": 3,
        "started_at": "2026-07-12T08:00:00Z",
        "duration_ms": 845,
        "services": ["checkout-api", "payments-api", "database"],
        "releases": ["checkout@1.2.3"],
        "environments": ["production"],
        "next_action": {
            "code": "inspect_trace",
            "target": "trace_summary"
        }
    }]);
    Mock::given(method("GET"))
        .and(path("/api/telemetry/traces"))
        .and(query_param("service_name", "checkout-api"))
        .and(query_param("status", "error"))
        .and(query_param("since", "24h"))
        .and(query_param("min_duration_ms", "500"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .mount(&server)
        .await;
    let args = [
        "logbrew",
        "traces",
        "--service",
        "checkout-api",
        "--status",
        "error",
        "--since",
        "24h",
        "--min-duration-ms",
        "500",
    ];
    let human = run_command(&server, args, "trace-discovery-human").await?;

    assert_eq!(
        human,
        "Traces (1)\n- 4bf92f3577b34da6a3ce929d0e0e4736 error POST /checkout \
         service=checkout-api operation=http.server spans=12 errors=2 services=3 duration=845ms \
         started=2026-07-12T08:00:00Z\nNext: logbrew trace <trace_id> or logbrew explain trace \
         <trace_id>\n"
    );

    let json = run_command(
        &server,
        [
            "logbrew",
            "traces",
            "--service",
            "checkout-api",
            "--status",
            "error",
            "--since",
            "24h",
            "--min-duration-ms",
            "500",
            "--json",
        ],
        "trace-discovery-json",
    )
    .await?;
    let body: serde_json::Value = serde_json::from_str(json.as_str())?;

    assert_eq!(body, response);
    assert_eq!(body[0]["next_action"]["code"], "inspect_trace");
    assert_eq!(body[0]["next_action"]["target"], "trace_summary");
    Ok(())
}

#[tokio::test]
async fn empty_trace_discovery_has_a_concrete_human_next_step()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/traces"))
        .and(query_param("min_duration_ms", "999999"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let human = run_command(
        &server,
        ["logbrew", "traces", "--min-duration-ms", "999999"],
        "empty-traces",
    )
    .await?;

    assert_eq!(
        human,
        "Traces (0)\nNo traces found.\nNext: widen --project/--service/--release/--environment/--status/--since/--min-duration-ms filters.\n"
    );
    Ok(())
}

#[test]
fn trace_discovery_validation_is_agent_actionable() {
    for (args, error, message, next) in [
        (
            &["logbrew", "traces", "--status", "degraded", "--json"][..],
            "unknown_trace_status",
            "unknown trace status: degraded",
            "use --status error or --status ok",
        ),
        (
            &["logbrew", "traces", "--min-duration-ms=-1", "--json"],
            "invalid_min_duration",
            "invalid minimum duration: -1",
            "use --min-duration-ms with a non-negative whole number",
        ),
        (
            &["logbrew", "traces", "--min-duration-ms", "-1", "--json"],
            "invalid_min_duration",
            "invalid minimum duration: -1",
            "use --min-duration-ms with a non-negative whole number",
        ),
        (
            &[
                "logbrew",
                "traces",
                "--min-duration-ms=9223372036854775808",
                "--json",
            ],
            "invalid_min_duration",
            "invalid minimum duration: 9223372036854775808",
            "use --min-duration-ms with a non-negative whole number",
        ),
        (
            &["logbrew", "traces", "--min-duration-ms", "--json"],
            "missing_flag_value",
            "missing value for --min-duration-ms",
            "provide a value after --min-duration-ms",
        ),
        (
            &["logbrew", "traces", "--search", "checkout", "--json"],
            "unsupported_flag",
            "unsupported flag for read traces: --search",
            "run logbrew read traces --help",
        ),
        (
            &[
                "logbrew",
                "trace",
                "trace_123",
                "--min-duration-ms",
                "500",
                "--json",
            ],
            "unsupported_flag",
            "unsupported flag for read trace: --min-duration-ms",
            "run logbrew read trace --help",
        ),
    ] {
        let parse_error = parse_command(args.iter().copied()).expect_err("command fails");
        let mut output = Vec::new();

        write_cli_error(&parse_error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid JSON");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], error);
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], next);
    }
}

#[test]
fn plural_trace_discovery_rejects_non_id_positionals() {
    for args in [
        &["logbrew", "traces", "checkout-api", "--json"][..],
        &["logbrew", "read", "traces", "checkout-api", "--json"],
    ] {
        let parse_error = parse_command(args.iter().copied()).expect_err("positional fails closed");
        let mut output = Vec::new();

        write_cli_error(&parse_error, true, &mut output).expect("error writes");

        let body: serde_json::Value =
            serde_json::from_slice(output.as_slice()).expect("valid JSON");
        assert_eq!(body["error"], "unexpected_argument");
        assert_eq!(
            body["message"],
            "unexpected argument for read traces: checkout-api"
        );
        assert_eq!(body["next"], "run logbrew read traces --help");
    }

    let detail = parse_command(["logbrew", "traces", "trace_123", "--json"])
        .expect("obvious copied trace id remains a detail shortcut");
    assert_eq!(
        detail.http_path().expect("trace detail has endpoint"),
        "/api/telemetry/traces/trace_123"
    );
}

#[test]
fn watch_traces_recovers_to_historical_discovery() {
    let error = parse_command(["logbrew", "watch", "traces", "--json"])
        .expect_err("live trace discovery remains unsupported");
    let mut output = Vec::new();

    write_cli_error(&error, true, &mut output).expect("error writes");

    let body: serde_json::Value = serde_json::from_slice(output.as_slice()).expect("valid JSON");
    assert_eq!(body["error"], "unknown_resource");
    assert_eq!(body["message"], "unknown resource: traces");
    assert_eq!(
        body["next"],
        "use logbrew traces for recent traces, or logbrew trace <trace_id> for one trace"
    );
}

#[tokio::test]
async fn trace_discovery_preserves_backend_validation_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/telemetry/traces"))
        .and(query_param("since", "0h"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "error": "invalid since value",
            "code": "validation_failed",
            "next": "use since=24h, since=7d, or an RFC3339 timestamp",
            "next_action": {
                "code": "fix_request",
                "target": "request"
            }
        })))
        .mount(&server)
        .await;
    let command = parse_command(["logbrew", "traces", "--since", "0h", "--json"])?;
    let env = authenticated_env(&server, "trace-since-recovery");
    let mut output = Vec::new();

    let error = execute_command(&command, &env, &mut output)
        .await
        .expect_err("invalid since fails");
    write_runtime_error(&error, true, &mut output)?;

    let body: serde_json::Value = serde_json::from_slice(output.as_slice())?;
    assert_eq!(body["status"], 422);
    assert_eq!(body["api_code"], "validation_failed");
    assert_eq!(body["api_error"], "invalid since value");
    assert_eq!(
        body["next"],
        "use since=24h, since=7d, or an RFC3339 timestamp"
    );
    let backend_body: serde_json::Value = serde_json::from_str(
        body["body"]
            .as_str()
            .expect("backend validation body remains available"),
    )?;
    assert_eq!(backend_body["next_action"]["code"], "fix_request");
    assert_eq!(backend_body["next_action"]["target"], "request");
    Ok(())
}

async fn run_command<const N: usize>(
    server: &MockServer,
    args: [&'static str; N],
    home_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let command = parse_command(args)?;
    let env = authenticated_env(server, home_name);
    let mut output = Vec::new();

    execute_command(&command, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

fn authenticated_env(server: &MockServer, home_name: &str) -> CliEnvironment {
    CliEnvironment {
        base_url: server.uri(),
        token: Some("test-token".to_owned()),
        home: Some(std::env::temp_dir().join(format!("logbrew-{home_name}"))),
        cwd: None,
    }
}
