//! Operations performance view contract tests.

use crate::matchers::query_param;
use crate::{Mock, MockServer, ResponseTemplate, TestResult, execute_command, rendered_json_error};
use logbrew_cli::{
    Command, HttpMethod, OperationsOptions, RuntimeError, parse_command, write_runtime_error,
};
use serde_json::{Value, json};

#[test]
fn grammar_is_closed_and_filters_are_explicit() -> TestResult {
    for (line, operation, json) in [
        ("logbrew operations", None, false),
        ("logbrew operations list", None, false),
        ("logbrew operations --json", None, true),
        ("logbrew --json operations list", None, true),
        (
            "logbrew operations show api.read --json",
            Some("api.read"),
            true,
        ),
    ] {
        let command = parse(line)?;
        assert_eq!(
            command,
            Command::Operations {
                options: OperationsOptions {
                    operation: operation.map(String::from),
                    ..OperationsOptions::default()
                },
                json
            },
            "{line}"
        );
        assert_eq!(command.http_method(), Some(HttpMethod::Get));
    }
    assert_eq!(
        parse(
            "logbrew operations list --since 7d --until 2026-10-29T12:00:00Z --env production --release=api@1"
        )?,
        Command::Operations {
            options: OperationsOptions {
                operation: None,
                since: Some("7d".into()),
                until: Some("2026-10-29T12:00:00Z".into()),
                environment: Some("production".into()),
                release: Some("api@1".into()),
            },
            json: false
        }
    );
    for (line, code) in [
        ("logbrew operations nope", "invalid_operations_command"),
        (
            "logbrew operations list extra",
            "invalid_operations_command",
        ),
        (
            "logbrew operations list --since",
            "invalid_operations_command",
        ),
        (
            "logbrew operations list --since 7d --since 1d",
            "invalid_operations_command",
        ),
        (
            "logbrew operations list --json --json",
            "invalid_operations_command",
        ),
        (
            "logbrew operations list --json=true",
            "invalid_operations_command",
        ),
        (
            "logbrew operations list --release=",
            "invalid_operations_command",
        ),
        (
            "logbrew operations show Hostile_Secret",
            "invalid_operation_id",
        ),
    ] {
        let (body, text) = rendered_json_error(&line.split_whitespace().collect::<Vec<_>>())?;
        assert_eq!(body["error"], code, "{line}");
        assert!(!text.contains("Hostile"), "{line} echoed its input");
    }
    Ok(())
}

async_test!(list_ranks_breaches_and_lists_gaps -> TestResult, {
    let server = MockServer::start().await?;
    let body = report(&[breach(), gap()]);
    Mock::auth("GET", "/api/operations", "account-token")
        .and(query_param("response_version", "1"))
        .and(query_param("since", "7d"))
        .and(query_param("environment", "production"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(2)
        .mount(&server);
    let line = "logbrew operations list --since 7d --environment production";
    assert_eq!(run(&server, &format!("{line} --json")).await?, format!("{body}\n"));
    let text = run(&server, line).await?;
    assert_eq!(
        text,
        "Operations from 2026-10-28T12:00:00.000Z to 2026-10-29T12:00:00.000Z (inventory 0123456789abcdef)\n\
         Coverage: 2 operations, 1 instrumented, 0 partial, 1 missing; 1 observed in the window\n\
         Targets: 1 breached, 0 with too few samples, 0 on stale data\n\
         Ranked:\n  \
         breach      traces.read                n=300      p99 2.0 s        target p99 <= 86 ms (api.read)\n\
         Gaps (1):\n  \
         web.page [missing]: time page loads in the browser SDK\n\
         Not in the inventory: 3 span groups\n  \
         api http.server GET /api/new (n=4)\n\
         Next: run logbrew operations show <operation_id>\n"
    );
    Ok(())
});

async_test!(show_prints_every_observation_below_100_samples -> TestResult, {
    let server = MockServer::start().await?;
    let mut few = breach();
    few["status"] = json!("unsupported");
    few["samples"] = json!(3);
    few["p99"] = Value::Null;
    few["p99_supported"] = json!(false);
    few["p50"] = json!(2);
    few["p95"] = json!(3);
    few["max"] = json!(3);
    few["errors"] = json!(0);
    few["timeouts"] = json!(0);
    few["observations"] = json!([1, 2, 3]);
    few["target_ratio"] = Value::Null;
    few["breakdown"] = json!([{
        "service": "api", "operation": "http.server", "name": "GET /api/traces", "samples": 3,
        "p50": 2, "p95": 3, "p99": null, "max": 3, "errors": 0, "timeouts": 0,
        "latest_at": "2026-10-29T11:59:00.000Z"
    }]);
    let mut body = report(&[few]);
    body["coverage"] = json!({
        "operations": 1, "instrumented": 1, "partial": 0, "missing": 0, "observed": 1,
        "breaches": 0, "unsupported": 1, "stale": 0
    });
    body["unmapped"] = json!([]);
    body["unmapped_total"] = json!(0);
    Mock::auth("GET", "/api/operations", "account-token")
        .and(query_param("operation", "traces.read"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server);
    let text = run(&server, "logbrew operations show traces.read").await?;
    for expected in [
        "traces.read (unsupported)\n",
        "Samples: 3  p50 2 ms  p95 3 ms  p99 unsupported (fewer than 100 samples)  max 3 ms\n",
        "The target needs 100 samples; it cannot pass on 3.\n",
        "Observations (ms): 1, 2, 3\n",
        "  api http.server GET /api/traces: n=3 p50 2 ms p99 unsupported max 3 ms errors 0 timeouts 0\n",
    ] {
        assert!(text.contains(expected), "{expected:?} missing from:\n{text}");
    }
    Ok(())
});

async_test!(invalid_reports_and_errors_print_fixed_guidance -> TestResult, {
    let mut estimated = breach();
    estimated["samples"] = json!(40);
    let mut unranked = report(&[gap(), breach()]);
    unranked["coverage"]["breaches"] = json!(1);
    let mut hostile = breach();
    hostile["component"] = json!("hostile\u{1b}[31m");
    let mut extra = report(&[breach(), gap()]);
    extra["private"] = json!("hostile-secret");
    for body in [report(&[estimated, gap()]), unranked, report(&[hostile, gap()]), extra] {
        let server = MockServer::start().await?;
        Mock::auth("GET", "/api/operations", "account-token")
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server);
        let error = run_error(&server, "logbrew operations list").await?;
        let text = runtime_error(&error)?;
        assert!(text.contains("operations response was invalid"), "{text}");
        assert!(!text.contains("hostile"), "{text}");
    }
    for (status, error, code) in [
        (404, "operations not found", "operations_unavailable"),
        (404, "operation not found", "operation_not_found"),
    ] {
        let server = MockServer::start().await?;
        Mock::auth("GET", "/api/operations", "account-token")
            .respond_with(ResponseTemplate::new(status).set_body_json(&json!({
                "error": error, "code": "not_found", "next": "hostile server text",
                "next_action": {"code": "check_resource", "target": "resource"}
            })))
            .mount(&server);
        let text = runtime_error(&run_error(&server, "logbrew operations show traces.read").await?)?;
        assert_eq!(serde_json::from_str::<Value>(&text)?["api_code"], code, "{text}");
        assert!(!text.contains("hostile"), "{text}");
    }
    Ok(())
});

fn breach() -> Value {
    json!({
        "id": "traces.read", "operation": "API: trace reads", "component": "trace routes",
        "start": "the API receives the request", "end": "the API sends the response",
        "workload": "every trace read", "success": "2xx response",
        "instrumentation": "instrumented", "gap": null, "source": "spans", "status": "breach",
        "unit": "ms",
        "target": {"id": "api.read", "statistic": "p99", "value": 86, "min_samples": 100},
        "samples": 300, "p50": 1, "p95": 3, "p99": 2_000, "p99_supported": true, "max": 4_000,
        "errors": 2, "timeouts": 1, "measured_since": "2026-10-28T12:00:00.000Z",
        "measured_until": "2026-10-29T12:00:00.000Z", "latest_at": "2026-10-29T11:59:00.000Z",
        "stale": false, "observations": [], "target_ratio": 23.25, "breakdown": []
    })
}

fn gap() -> Value {
    json!({
        "id": "web.page", "operation": "Web: page load", "component": "web app",
        "start": "navigation starts", "end": "the page is interactive", "workload": "every page",
        "success": "the page renders", "instrumentation": "missing",
        "gap": "time page loads in the browser SDK", "source": "none", "status": "gap",
        "unit": "ms", "target": null, "samples": 0, "p50": null, "p95": null, "p99": null,
        "p99_supported": false, "max": null, "errors": null, "timeouts": null,
        "measured_since": null, "measured_until": null, "latest_at": null, "stale": false,
        "observations": [], "target_ratio": null, "breakdown": []
    })
}

fn report(operations: &[Value]) -> Value {
    json!({
        "response_version": 1, "inventory_revision": "0123456789abcdef",
        "since": "2026-10-28T12:00:00.000Z", "until": "2026-10-29T12:00:00.000Z",
        "generated_at": "2026-10-29T12:00:00.000Z", "environment": "production", "release": null,
        "coverage": {
            "operations": 2, "instrumented": 1, "partial": 0, "missing": 1, "observed": 1,
            "breaches": 1, "unsupported": 0, "stale": 0
        },
        "operations": operations,
        "unmapped": [{"service": "api", "operation": "http.server", "name": "GET /api/new", "samples": 4}],
        "unmapped_total": 3
    })
}

fn parse(line: &str) -> Result<Command, logbrew_cli::CliError> {
    parse_command(line.split_whitespace())
}

async fn run(server: &MockServer, line: &str) -> TestResult<String> {
    let mut output = Vec::new();
    let env = super::authenticated_env(server, "account-token", None);
    execute_command(&parse(line)?, &env, &mut output).await?;
    Ok(String::from_utf8(output)?)
}

async fn run_error(server: &MockServer, line: &str) -> TestResult<RuntimeError> {
    let mut output = Vec::new();
    let env = super::authenticated_env(server, "account-token", None);
    let error = execute_command(&parse(line)?, &env, &mut output)
        .await
        .err()
        .ok_or("request must fail")?;
    assert!(output.is_empty(), "a failed request wrote output");
    Ok(error)
}

fn runtime_error(error: &RuntimeError) -> TestResult<String> {
    let mut output = Vec::new();
    write_runtime_error(error, true, &mut output)?;
    Ok(String::from_utf8(output)?)
}
