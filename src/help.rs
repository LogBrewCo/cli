//! CLI help text kept separate from command parsing.

use crate::HelpTopic;

/// Returns user-facing help for a topic.
#[must_use]
pub const fn help_text(topic: HelpTopic) -> &'static str {
    match topic {
        HelpTopic::Root => ROOT_HELP,
        HelpTopic::Login => LOGIN_HELP,
        HelpTopic::Logout => LOGOUT_HELP,
        HelpTopic::Setup => SETUP_HELP,
        HelpTopic::Status => STATUS_HELP,
        HelpTopic::Version => VERSION_HELP,
        HelpTopic::Auth => AUTH_HELP,
        HelpTopic::Json => JSON_HELP,
        HelpTopic::Examples => EXAMPLES_HELP,
        HelpTopic::Projects => PROJECTS_HELP,
        HelpTopic::Usage => USAGE_HELP,
        HelpTopic::Read => READ_HELP,
        HelpTopic::ReadLogs => READ_LOGS_HELP,
        HelpTopic::ReadIssues => READ_ISSUES_HELP,
        HelpTopic::ReadActions => READ_ACTIONS_HELP,
        HelpTopic::ReadReleases => READ_RELEASES_HELP,
        HelpTopic::ReadTrace => READ_TRACE_HELP,
        HelpTopic::ReadIssue => READ_ISSUE_HELP,
        HelpTopic::Watch => WATCH_HELP,
        HelpTopic::Explain => EXPLAIN_HELP,
        HelpTopic::Set => SET_HELP,
    }
}

/// Root command help text.
const ROOT_HELP: &str = "\
LogBrew CLI

Usage:
  logbrew login [--no-open] [--json]
  logbrew logout [--json]
  logbrew setup [--auto] [--yes] [--json]
  logbrew projects [--json]
  logbrew usage [--json]
  logbrew status [--json]
  logbrew health [--json]
  logbrew doctor [--json]
  logbrew whoami [--json]
  logbrew me [--json]
  logbrew version [--json]
  logbrew read logs [--severity error] [--search checkout] [--release <release>] [--environment \
                         production] [--since 24h] [--json]
  logbrew logs checkout failed [--severity error] [--release <release>] [--environment \
                         production] [--json]
  logbrew logs error checkout failed [--release <release>] [--environment production] [--json]
  logbrew search checkout [--release <release>] [--environment production] [--json]
  logbrew find checkout [--release <release>] [--environment production] [--json]
  logbrew grep checkout [--release <release>] [--environment production] [--json]
  logbrew show logs [--release <release>] [--environment production] [--json]
  logbrew latest logs [--limit 20] [--json]
  logbrew last 10 logs [--json]
  logbrew last 5 open issues [--json]
  logbrew list issues [--status unresolved] [--json]
  logbrew issues open [--release <release>] [--environment production] [--json]
  logbrew issue open [--release <release>] [--environment production] [--json]
  logbrew open issues [--release <release>] [--environment production] [--json]
  logbrew open issue [--release <release>] [--environment production] [--json]
  logbrew errors closed [--release <release>] [--environment production] [--json]
  logbrew get issue <issue_id> [--json]
  logbrew read issues [--release <release>] [--environment production] [--status unresolved] \
                         [--json]
  logbrew read actions [--release <release>] [--environment production] [--name checkout_failed] \
                         [--json]
  logbrew events checkout_failed [--release <release>] [--environment production] [--json]
  logbrew read releases [--environment production] [--json]
  logbrew read trace <trace_id> [--release <release>] [--environment production] [--json]
  logbrew trace <trace_id> [--release <release>] [--environment production] [--json]
  logbrew issue <issue_id> [--json]
  logbrew issue <issue_id> explain [--json]
  logbrew trace <trace_id> explain [--json]
  logbrew explain issue <issue_id> [--json]
  logbrew explain trace <trace_id> [--json]
  logbrew explain <issue_id_or_trace_id> [--json]
  logbrew <issue_id_or_trace_id> explain [--json]
  logbrew set issue <issue_id> resolved [--json]
  logbrew resolve <issue_id> [--json]
  logbrew close <issue_id> [--json]
  logbrew ignore <issue_id> [--json]
  logbrew reopen <issue_id> [--json]

Popular terms: auth, status, health, setup, projects, usage, logs, issues, errors, traces, spans, \
                         actions, events, releases, environments.
Health aliases: logbrew status, logbrew health, logbrew ping, logbrew doctor.
Setup aliases (non-mutating plan): logbrew init, logbrew install, logbrew configure, logbrew sdk.
Shortcuts: logbrew auth, logbrew whoami, logbrew me, logbrew log, logbrew logs, logbrew issues, \
                         logbrew logs checkout failed, logbrew logs error checkout, logbrew \
                         search checkout, logbrew find checkout, logbrew grep checkout, logbrew \
                         errors, logbrew actions, logbrew events, logbrew events checkout_failed, \
                         logbrew release, logbrew releases, logbrew trace <id>, logbrew issue \
                         <id>, logbrew resolve <id>, logbrew close <id>, logbrew ignore <id>, \
                         logbrew reopen <id>.
Read verbs: logbrew show logs, logbrew latest logs, logbrew last 10 logs, logbrew recent issues, \
                         logbrew list issues, logbrew get issue <id>.
Singular read aliases: logbrew read log, read release, show log, list issue, get release.
Pasted IDs: logbrew issue_123 or logbrew <trace_id>.
Examples: logbrew examples.
Topic help: logbrew logs --help, logbrew help logs, logbrew help read logs, or logbrew help json.
JSON mode: logbrew --json status and logbrew status --json both work.
Use --json for stable machine-readable output.";

/// Login command help text.
const LOGIN_HELP: &str = "\
Usage:
  logbrew login [--no-open] [--json]

Starts browser login for the native CLI. Use --no-open to print the URL without opening a browser.
--json prints the auth handoff without opening a browser.";

/// Logout command help text.
const LOGOUT_HELP: &str = "\
Usage:
  logbrew logout [--json]

Removes the local CLI token. If LOGBREW_TOKEN is set, unset it to fully log out.";

/// Setup command help text.
const SETUP_HELP: &str = "\
Usage:
  logbrew setup [--auto] [--yes] [--json]

Detects supported project manifests and prints a non-mutating SDK setup plan.
No files are changed. Install: not ready.
Aliases (same non-mutating plan): logbrew init, logbrew install, logbrew configure, logbrew sdk.
Options: --auto records automatic detection preference; --yes records confirmation preference; \
                          --json prints stable setup JSON.
Supported manifests: package.json, pyproject.toml, Pipfile, Cargo.toml, Package.swift, \
                          project.yml, project.yaml, .xcodeproj, .xcworkspace, go.mod, \
                          composer.json.
Package managers: npm, pnpm, yarn, bun, pip, uv, poetry, pipenv, cargo, SwiftPM, XcodeGen, Go, \
                          Composer.";

/// Status command help text.
const STATUS_HELP: &str = "\
Usage:
  logbrew status [--json]
  logbrew health [--json]
  logbrew ping [--json]
  logbrew doctor [--json]
  logbrew whoami [--json]
  logbrew me [--json]
  logbrew auth status [--json]

Checks local auth and API reachability.
Identity aliases: logbrew whoami, logbrew me, logbrew auth status.";

/// Version command help text.
const VERSION_HELP: &str = "\
Usage:
  logbrew version [--json]
  logbrew --version [--json]

Prints the installed CLI version.
The CLI is a native Rust binary.";

/// Auth workflow help text.
const AUTH_HELP: &str = "\
Usage:
  logbrew login [--no-open] [--json]
  logbrew auth login [--no-open] [--json]
  logbrew status [--json]
  logbrew auth status [--json]
  logbrew auth whoami [--json]
  logbrew auth me [--json]
  logbrew whoami [--json]
  logbrew me [--json]
  logbrew logout [--json]
  logbrew auth logout [--json]

Use login once, status/whoami/me to verify API/auth state, and logout to remove the local token.
Use --json for agent-readable auth checks.";

/// JSON output help text.
const JSON_HELP: &str = "\
Usage:
  logbrew --json status
  logbrew status --json
  logbrew logs --json
  logbrew help json --json

Use --json before or after commands for stable machine-readable output.
Stable JSON keeps server response shapes for reads and mutations.
Errors include ok, error, message, and next.
Parse errors and CLI runtime errors include next_action.
Local auth and setup JSON include next_action.
API runtime errors include api_next_action when the server provides it.";

/// First-run examples and common workflows.
const EXAMPLES_HELP: &str = "\
Usage:
  logbrew examples
  logbrew help examples

First run:
  logbrew status
  logbrew login
  logbrew setup

Troubleshoot:
  logbrew logs error checkout failed --release checkout@1 --environment production
  logbrew issues open --release checkout@1 --environment production
  logbrew issue issue_123
  logbrew explain issue issue_123
  logbrew trace <trace_id>

Live:
  logbrew watch --json
  logbrew watch --severity error,critical --json

Agent JSON:
  logbrew --json status
  logbrew logs checkout failed --json
  logbrew explain trace <trace_id> --json

More help:
  logbrew help logs
  logbrew help issues
  logbrew help watch
  logbrew help json";

/// Backend-owned project setup help text.
const PROJECTS_HELP: &str = "\
Usage:
  logbrew projects [--json]
  logbrew project [--json]
  logbrew projects create <name> [--json]
  logbrew setup --create-project [--json]
  logbrew projects setup <project_id> [--runtime <runtime>] [--source api|cli|sdk] \
[--environment <environment>] [--json]

Project creation, setup status, and project-scoped ingest credentials are backend-owned.
Current mode: projects setup marks backend-owned setup as seen; project creation remains help only.
No local project, install, quota, or usage state is created.
Project setup uses POST /api/projects/{project_id}/setup/seen and preserves backend setup status JSON.
Project-scoped SDK/ingest credentials are shown only when backend returns one-time credentials.
Never use an account bearer token as SDK or ingest configuration.
Next: run logbrew setup for the current non-mutating local plan.";

/// Backend-owned usage and quota help text.
const USAGE_HELP: &str = "\
Usage:
  logbrew usage [--json]
  logbrew account usage [--json]

Account usage, plan limits, quota state, reset dates, and per-project or per-stream breakdowns are \
backend-owned.
Current mode: help only. The CLI does not calculate or persist usage/quota state from local files.
When backend usage is available, the CLI will read the backend account usage contract and preserve \
stable JSON for agents.
Next: run logbrew status to verify API and auth state.";

/// Read command help text.
const READ_HELP: &str = "\
Usage:
  logbrew read logs [filters] [--json]
  logbrew read log [filters] [--json]
  logbrew show logs [filters] [--json]
  logbrew list issues [filters] [--json]
  logbrew get issue <issue_id> [--json]
  logbrew read issues [filters] [--json]
  logbrew read actions [filters] [--json]
  logbrew read releases [filters] [--json]
  logbrew read release [filters] [--json]
  logbrew read trace <trace_id> [--json]
  logbrew read issue <issue_id> [--json]

Reads historical observability data for agents and developers.
Singular read aliases: logbrew read log, read release, show log, list issue, get release.
Recency counts are limit shortcuts: logbrew last 10 logs or logbrew recent 5 issues.
Use --environment <environment> with logs, issues, actions, releases, or traces.
Filter aliases: --env, --project-id, --trace-id, and --distinct-id.";

/// Read logs help text.
const READ_LOGS_HELP: &str = "\
Usage:
  logbrew read logs [--severity error] [--search checkout] [--release <release>] [--environment \
                              production] [--since 24h] [--trace <trace_id>] [--project \
                              <project_id>] [--limit 100] [--json]
  logbrew logs checkout failed [--severity error] [--release <release>] [--environment \
                              production] [--json]
  logbrew logs error checkout failed [--release <release>] [--environment production] [--json]

Reads structured logs. Severity values are info, warning, error, and critical.
Legacy severity aliases are accepted on input and normalized.
Severity matching is case-insensitive. --level is accepted as a compatibility alias for \
                              --severity.
The logs shortcut accepts obvious multi-word search text, such as logbrew logs checkout failed.
Shortcut levels can include search text, such as logbrew logs error checkout failed.
Recency counts are limit shortcuts, such as logbrew last 10 logs.
Explicit filters accept unquoted search text too, such as logbrew logs --severity warning checkout \
                              failed or logbrew logs --search checkout failed.
Use -- before literal flag-looking search text, such as logbrew logs -- --timeout --json.
Filter by severity, message search, release, or trace_id to correlate logs with spans.
Limit must be a positive whole number.";

/// Read issues help text.
const READ_ISSUES_HELP: &str = "\
Usage:
  logbrew read issues [--release <release>] [--environment production] [--status unresolved] \
                                [--project <project_id>] [--limit 100] [--json]
  logbrew issues open [--release <release>] [--environment production] [--json]
  logbrew issue open [--release <release>] [--environment production] [--json]
  logbrew open issues [--release <release>] [--environment production] [--json]
  logbrew open issue [--release <release>] [--environment production] [--json]
  logbrew last 5 open issues [--json]
  logbrew errors closed [--release <release>] [--environment production] [--json]

Reads grouped issues across releases and environments.
Status accepts unresolved/open, resolved/closed, or ignored, case-insensitively.
Issue shortcuts accept status words, such as logbrew issues open, logbrew issue open, logbrew open \
                                issues, logbrew open issue, or logbrew errors closed.
Recency issue shortcuts can include status and count, such as logbrew last 5 open issues.
Limit must be a positive whole number.";

/// Read actions help text.
const READ_ACTIONS_HELP: &str = "\
Usage:
  logbrew read actions [--release <release>] [--environment production] [--name checkout_failed] \
                                 [--user <distinct_id>] [--since 24h] [--project <project_id>] \
                                 [--limit 100] [--json]
  logbrew events checkout_failed [--release <release>] [--environment production] [--json]

Reads product actions. Use distinct_id to follow one actor or session.
Action/event aliases accept one positional name as the same filter as --name.
Limit must be a positive whole number.";

/// Read releases help text.
const READ_RELEASES_HELP: &str = "\
Usage:
  logbrew read releases [--release <release>] [--environment production] [--project <project_id>] \
                                  [--limit 100] [--json]

Reads release summaries with counts for issues, logs, trace spans, and actions.
Limit must be a positive whole number.";

/// Read trace help text.
const READ_TRACE_HELP: &str = "\
Usage:
  logbrew read trace <trace_id> [--release <release>] [--environment production] [--project \
                               <project_id>] [--json]

Reads spans for one distributed trace.";

/// Read issue help text.
const READ_ISSUE_HELP: &str = "\
Usage:
  logbrew read issue <issue_id> [--json]

Reads one grouped issue with status, release, environment, and occurrence counts.";

/// Watch command help text.
const WATCH_HELP: &str = "\
Usage:
  logbrew watch --json
  logbrew watch logs [--json]
  logbrew watch issues [--json]
  logbrew watch actions [--json]
  logbrew watch --severity error,critical --json

Aliases: tail, follow, and stream use the same live watch flow.
Live watch uses a short-lived feed ticket and WebSocket stream.
Transient disconnects reconnect with a fresh ticket and backoff.
Server-side live filters are not sent yet; severity filtering is applied client-side.";

/// Explain command help text.
const EXPLAIN_HELP: &str = "\
Usage:
  logbrew explain issue <issue_id> [--json]
  logbrew explain trace <trace_id> [--json]
  logbrew explain <issue_id_or_trace_id> [--json]
  logbrew issue <issue_id> explain [--json]
  logbrew trace <trace_id> explain [--json]
  logbrew <issue_id_or_trace_id> explain [--json]

Fetches enough context for an AI agent to explain what happened.
Pasted UUID/issue_* values are treated as issues; 32-hex/trace_* values are treated as traces.";

/// Set command help text.
const SET_HELP: &str = "\
Usage:
  logbrew set issue <issue_id> unresolved [--json]
  logbrew set issue <issue_id> resolved [--json]
  logbrew set issue <issue_id> ignored [--json]
  logbrew resolve <issue_id> [--json]
  logbrew close <issue_id> [--json]
  logbrew ignore <issue_id> [--json]
  logbrew reopen <issue_id> [--json]
  logbrew issue <issue_id> resolve [--json]
  logbrew issue <issue_id> close [--json]
  logbrew issue <issue_id> ignore [--json]
  logbrew issue <issue_id> reopen [--json]
  logbrew <issue_id> resolve [--json]
  logbrew resolved <issue_id> [--json]
  logbrew closed <issue_id> [--json]
  logbrew ignored <issue_id> [--json]
  logbrew open <issue_id> [--json]
  logbrew unresolved <issue_id> [--json]

Updates grouped issue status. Resolve/close map to resolved; ignore maps to ignored; reopen maps \
                        to unresolved.
Close is an alias for resolved.
Issue-first, pasted-ID, and status-first aliases are useful after reading issue detail.
Status values are case-insensitive.";
