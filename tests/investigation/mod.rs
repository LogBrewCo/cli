//! Public CLI investigation contracts.

mod api_rendering;
mod cursor_pagination;
mod explain_contracts;
mod issue_investigation;
mod project_create;
mod span_investigation;
use crate::support::*;
use crate::{TestResult, rendered_json_error};

const CLI_CURSOR_RECOVERY: &str = "use --pagination cursor alone for the first page, then use \
    --cursor-time and --cursor-id together from next_cursor";

fn assert_cursor_flag_errors(resource: &str, time: &str, id: &str, code: &str) -> TestResult {
    let label = resource
        .strip_suffix('s')
        .ok_or("cursor resource must be plural")?;
    let first = format!("invalid {label} cursor: cursor fields require --pagination cursor");
    let pair =
        format!("invalid {label} cursor: --cursor-time and --cursor-id must be used together");
    for (flags, expected_code, message) in [
        (
            &["--cursor-time", time, "--cursor-id", id][..],
            code,
            first.as_str(),
        ),
        (
            &["--pagination", "cursor", "--cursor-time", time],
            code,
            pair.as_str(),
        ),
        (
            &["--pagination", "secret-pagination-sentinel"],
            "unknown_pagination",
            "unknown pagination mode",
        ),
    ] {
        let mut args = vec!["logbrew", resource];
        args.extend_from_slice(flags);
        args.push("--json");
        let (body, text) = rendered_json_error(&args)?;
        assert_eq!(body["ok"], false, "{args:?}");
        assert_eq!(body["error"], expected_code, "{args:?}");
        assert_eq!(body["message"], message, "{args:?}");
        assert_eq!(body["next"], CLI_CURSOR_RECOVERY, "{args:?}");
        assert!(
            !text.contains("secret-pagination-sentinel"),
            "cursor error must not echo the supplied value"
        );
    }
    Ok(())
}

fn assert_cursor_help(resource: &str, cursor_id: &str) -> TestResult {
    let command = logbrew_cli::parse_command(["logbrew", resource, "--help"])?;
    let logbrew_cli::Command::Help { topic, .. } = command else {
        return Err("cursor help must resolve to a help topic".into());
    };
    let text = logbrew_cli::help::help_text(topic);
    for expected in [
        "--pagination cursor",
        "--cursor-time <RFC3339>",
        cursor_id,
        "next_cursor",
        "Keep the same active filters",
    ] {
        assert!(
            text.contains(expected),
            "{resource} help is missing {expected}"
        );
    }
    Ok(())
}
