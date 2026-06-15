//! CLI help text discovery tests.

use logbrew_cli::{HelpTopic, help};

#[test]
fn root_help_surfaces_release_environment_pairing() {
    let text = help::help_text(HelpTopic::Root);

    assert!(text.contains("logbrew read logs [--severity error] [--search checkout]"));
    assert!(text.contains("logbrew logs checkout failed [--severity error]"));
    assert!(text.contains("logbrew logs error checkout failed [--release <release>]"));
    assert!(text.contains("logbrew search checkout [--release <release>]"));
    assert!(text.contains("logbrew find checkout [--release <release>]"));
    assert!(text.contains("logbrew grep checkout [--release <release>]"));
    assert!(text.contains("logbrew latest logs [--limit 20] [--json]"));
    assert!(text.contains("logbrew last 10 logs [--json]"));
    assert!(text.contains("logbrew last 5 open issues [--json]"));
    assert!(text.contains("[--environment production] [--since 24h] [--json]"));
    assert!(text.contains("logbrew read issues [--release <release>] [--environment production]"));
    assert!(text.contains("logbrew issues open [--release <release>]"));
    assert!(text.contains("logbrew issue open [--release <release>]"));
    assert!(text.contains("logbrew open issues [--release <release>]"));
    assert!(text.contains("logbrew open issue [--release <release>]"));
    assert!(text.contains("logbrew errors closed [--release <release>]"));
    assert!(text.contains("logbrew read actions [--release <release>] [--environment production]"));
    assert!(text.contains("logbrew events checkout_failed"));
    assert!(text.contains(
        "logbrew read trace <trace_id> [--release <release>] [--environment production]"
    ));
    assert!(text.contains("logbrew trace <trace_id>"));
    assert!(text.contains("logbrew issue <issue_id>"));
    assert!(text.contains("logbrew explain trace <trace_id> [--json]"));
    assert!(text.contains("logbrew explain <issue_id_or_trace_id> [--json]"));
    assert!(text.contains("logbrew resolve <issue_id> [--json]"));
    assert!(text.contains("logbrew close <issue_id> [--json]"));
    assert!(text.contains("logbrew whoami [--json]"));
    assert!(text.contains("logbrew health [--json]"));
    assert!(text.contains("logbrew doctor [--json]"));
    assert!(text.contains(
        "Setup aliases (non-mutating plan): logbrew init, logbrew install, logbrew configure, \
         logbrew sdk."
    ));
    assert!(text.contains("logbrew projects [--json]"));
    assert!(text.contains("logbrew usage [--json]"));
    assert!(text.contains("Popular terms: auth, status, health, setup, projects, usage"));
    assert!(text.contains("Shortcuts: logbrew auth, logbrew whoami, logbrew me"));
    assert!(text.contains("Health aliases: logbrew status, logbrew health, logbrew ping"));
    assert!(text.contains("logbrew errors"));
    assert!(text.contains("logbrew logs checkout failed"));
    assert!(text.contains("logbrew logs error checkout"));
    assert!(text.contains("logbrew events"));
    assert!(text.contains("Read verbs: logbrew show logs, logbrew latest logs"));
    assert!(text.contains("logbrew last 10 logs"));
    assert!(text.contains(
        "Singular read aliases: logbrew read log, read release, show log, list issue, get release."
    ));
    assert!(text.contains("Pasted IDs: logbrew issue_123 or logbrew <trace_id>."));
    assert!(text.contains("JSON mode: logbrew --json status and logbrew status --json both work"));
    assert!(text.contains(
        "Topic help: logbrew logs --help, logbrew help logs, logbrew help read logs, or logbrew \
         help json."
    ));
    assert!(text.contains("Examples: logbrew examples."));
}

#[test]
fn project_and_usage_help_are_honest_about_backend_readiness() {
    let projects = help::help_text(HelpTopic::Projects);
    let usage = help::help_text(HelpTopic::Usage);

    assert!(projects.contains("logbrew projects create <name> [--json]"));
    assert!(projects.contains("logbrew setup --create-project [--json]"));
    assert!(projects.contains("Project creation, setup status"));
    assert!(projects.contains("Current mode: help only."));
    assert!(projects.contains("No local project, install, quota, or usage state is created."));
    assert!(projects.contains("Never use an account bearer token as SDK or ingest configuration."));

    assert!(usage.contains("logbrew usage [--json]"));
    assert!(usage.contains("logbrew account usage [--json]"));
    assert!(usage.contains("Account usage, plan limits, quota state"));
    assert!(usage.contains("Current mode: help only."));
    assert!(
        usage.contains("The CLI does not calculate or persist usage/quota state from local files.")
    );
}

#[test]
fn setup_help_advertises_first_contact_aliases() {
    let text = help::help_text(HelpTopic::Setup);

    assert!(text.contains("logbrew setup [--auto] [--yes] [--json]"));
    assert!(text.contains(
        "Aliases (same non-mutating plan): logbrew init, logbrew install, logbrew configure, \
         logbrew sdk."
    ));
    assert!(text.contains(
        "Options: --auto records automatic detection preference; --yes records confirmation \
         preference; --json prints stable setup JSON."
    ));
}

#[test]
fn read_help_advertises_singular_read_aliases() {
    let text = help::help_text(HelpTopic::Read);

    assert!(text.contains(
        "Singular read aliases: logbrew read log, read release, show log, list issue, get release."
    ));
    assert!(text.contains(
        "Recency counts are limit shortcuts: logbrew last 10 logs or logbrew recent 5 issues."
    ));
}

#[test]
fn log_help_advertises_unquoted_search_after_explicit_filters() {
    let text = help::help_text(HelpTopic::ReadLogs);

    assert!(text.contains("Recency counts are limit shortcuts, such as logbrew last 10 logs."));
    assert!(text.contains("logbrew logs --severity warning checkout failed"));
    assert!(!text.contains("logbrew logs --level error checkout failed"));
    assert!(text.contains("Severity values are info, warning, error, and critical."));
    assert!(text.contains("Legacy severity aliases are accepted on input and normalized."));
    assert!(text.contains("--level is accepted as a compatibility alias for --severity."));
    assert!(!text.contains("warn maps"));
    assert!(!text.contains("fatal maps"));
    assert!(!text.contains("debug"));
    assert!(text.contains("logbrew logs --search checkout failed"));
    assert!(text.contains("logbrew logs -- --timeout --json"));
}

#[test]
fn issue_help_advertises_status_word_shortcuts() {
    let text = help::help_text(HelpTopic::ReadIssues);

    assert!(text.contains("logbrew issues open [--release <release>]"));
    assert!(text.contains("logbrew issue open [--release <release>]"));
    assert!(text.contains("logbrew open issues [--release <release>]"));
    assert!(text.contains("logbrew open issue [--release <release>]"));
    assert!(text.contains("logbrew last 5 open issues [--json]"));
    assert!(text.contains("logbrew errors closed [--release <release>]"));
    assert!(text.contains("Issue shortcuts accept status words"));
    assert!(text.contains("Recency issue shortcuts can include status and count"));
}

#[test]
fn examples_help_gives_first_run_and_troubleshooting_workflows() {
    let text = help::help_text(HelpTopic::Examples);

    assert!(text.contains("logbrew status"));
    assert!(text.contains("logbrew login"));
    assert!(text.contains("logbrew setup"));
    assert!(text.contains("logbrew logs error checkout failed"));
    assert!(text.contains("logbrew issues open"));
    assert!(text.contains("logbrew explain issue issue_123"));
    assert!(text.contains("logbrew watch --severity error,critical --json"));
    assert!(text.contains("logbrew --json status"));
    assert!(text.contains("logbrew help json"));
}
