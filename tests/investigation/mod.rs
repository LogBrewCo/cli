//! Public CLI investigation contracts.

mod api_rendering;
mod cursor_pagination;
mod explain_contracts;
mod issue_investigation;
mod project_create;
mod span_investigation;
pub(crate) use crate::support::*;

const CLI_CURSOR_RECOVERY: &str = "use --pagination cursor alone for the first page, then use \
    --cursor-time and --cursor-id together from next_cursor";

fn assert_cursor_flag_errors(resource: &str, time: &str, id: &str, code: &str) {
    let label = resource
        .strip_suffix('s')
        .expect("cursor resource is plural");
    let first = format!("invalid {label} cursor: cursor fields require --pagination cursor");
    let pair =
        format!("invalid {label} cursor: --cursor-time and --cursor-id must be used together");
    let check = |args, expected_code, message| {
        let error = logbrew_cli::parse_command(args).expect_err("cursor input fails");
        let mut output = Vec::new();
        logbrew_cli::write_cli_error(&error, true, &mut output).expect("error writes");
        let body: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], expected_code);
        assert_eq!(body["message"], message);
        assert_eq!(body["next"], CLI_CURSOR_RECOVERY);
        assert!(!String::from_utf8_lossy(&output).contains("secret-pagination-sentinel"));
    };
    check(
        vec![
            "logbrew",
            resource,
            "--cursor-time",
            time,
            "--cursor-id",
            id,
            "--json",
        ],
        code,
        first.as_str(),
    );
    check(
        vec![
            "logbrew",
            resource,
            "--pagination",
            "cursor",
            "--cursor-time",
            time,
            "--json",
        ],
        code,
        pair.as_str(),
    );
    check(
        vec![
            "logbrew",
            resource,
            "--pagination",
            "secret-pagination-sentinel",
            "--json",
        ],
        "unknown_pagination",
        "unknown pagination mode",
    );
}

fn assert_cursor_help(resource: &str, cursor_id: &str) {
    let command =
        logbrew_cli::parse_command(["logbrew", resource, "--help"]).expect("cursor help parses");
    let logbrew_cli::Command::Help { topic, .. } = command else {
        panic!("cursor help should resolve");
    };
    let text = logbrew_cli::help::help_text(topic);
    for expected in [
        "--pagination cursor",
        "--cursor-time <RFC3339>",
        cursor_id,
        "next_cursor",
        "Keep the same active filters",
    ] {
        assert!(text.contains(expected));
    }
}
