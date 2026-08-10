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
        HelpTopic::ReadTraces => READ_TRACES_HELP,
        HelpTopic::ReadTrace => READ_TRACE_HELP,
        HelpTopic::ReadIssue => READ_ISSUE_HELP,
        HelpTopic::Watch => WATCH_HELP,
        HelpTopic::Explain => EXPLAIN_HELP,
        HelpTopic::Deploy => DEPLOY_HELP,
        HelpTopic::Analytics => ANALYTICS_HELP,
        HelpTopic::AnalyticsOverview => ANALYTICS_OVERVIEW_HELP,
        HelpTopic::AnalyticsProperties => ANALYTICS_PROPERTIES_HELP,
        HelpTopic::AnalyticsCompare => ANALYTICS_COMPARE_HELP,
        HelpTopic::AnalyticsPaths => ANALYTICS_PATHS_HELP,
        HelpTopic::AnalyticsFunnel => ANALYTICS_FUNNEL_HELP,
        HelpTopic::AnalyticsRetention => ANALYTICS_RETENTION_HELP,
        HelpTopic::AnalyticsLifecycle => ANALYTICS_LIFECYCLE_HELP,
        HelpTopic::Investigate => INVESTIGATE_HELP,
        HelpTopic::NativeDebugArtifacts => NATIVE_DEBUG_ARTIFACTS_HELP,
        HelpTopic::Set => SET_HELP,
        HelpTopic::Support => SUPPORT_HELP,
    }
}

/// Root command help text.
const ROOT_HELP: &str = "\
LogBrew CLI

Usage:
  logbrew login [--provider github|gitlab|bitbucket] [--no-open] [--json]
  logbrew logout [--json]
  logbrew setup [--auto] [--yes] [--json]
  logbrew projects [--json]
  logbrew projects create <name> --ingest-key-file <path> [--runtime <runtime>] \
                         [--environment <environment>] [--json]
  logbrew projects keys create <project_id> --ingest-key-file <path> [--label <label>] \
                         [--kind sdk|browser|server|cli] [--json]
  logbrew projects archive <project_id> --yes [--json]
  logbrew usage [--json]
  logbrew status [--json]
  logbrew health [--json]
  logbrew doctor [--json]
  logbrew doctor --project <project_id> [--json]
  logbrew whoami [--json]
  logbrew me [--json]
  logbrew version [--json]
  logbrew support create --category <category> --title <title> --description <description> [--json]
  logbrew support list [--status <status>] [--category <category>] [--json]
  logbrew support show <ticket_id> [--json]
  logbrew support context <ticket_id> [--json]
  logbrew support reply <ticket_id> --context <text> --retry-key <key> [--diagnostics] [--json]
  logbrew support close <ticket_id> [--json]
  logbrew support reopen <ticket_id> [--json]
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
  logbrew traces [--service <service_name>] [--status error] [--since 24h] [--json]
  logbrew read trace <trace_id> [--release <release>] [--environment production] [--json]
  logbrew trace <trace_id> [--release <release>] [--environment production] [--json]
  logbrew issue <issue_id> [--json]
  logbrew issue <issue_id> explain [--occurrence <recommended|first|latest|occurrence_id>] [--json]
  logbrew trace <trace_id> explain [--json]
  logbrew explain issue <issue_id> [--occurrence <recommended|first|latest|occurrence_id>] [--json]
  logbrew explain trace <trace_id> [--json]
  logbrew explain span <trace_id> <span_id> --project <project_id> --environment <environment> \
                            --release <release> [--json]
  logbrew deploy <deployment_id> --project <project_id> --release <release> --environment \
                            <environment> --service <service_name> --status <succeeded|failed> \
                            --started-at <rfc3339> --finished-at <rfc3339> \
                            [--commit-sha <sha>] [--json]
  logbrew explain <issue_id_or_trace_id> [--json]
  logbrew analytics overview --project <project_id> --since 24h [--json]
  logbrew analytics properties --project <project_id> --since 24h [--limit 20] [--json]
  logbrew analytics compare --project <project_id> --since 7d --target-kind interaction \
                         --target-event checkout_completed --segment old=Old-release \
                         --segment new=New-release --segment-release old=1.0.0 \
                         --segment-release new=1.1.0 [--json]
  logbrew analytics paths following --project <project_id> --since 24h --anchor-kind page-view \
                         --anchor-event /pricing [--json]
  logbrew analytics funnel --project <project_id> --since 24h --step page-view /pricing \
                         --step interaction signup_completed [--json]
  logbrew analytics retention --project <project_id> --since 30d --start-kind page-view \
                         --start-event /signup --return-kind interaction \
                         --return-event dashboard_opened [--json]
  logbrew analytics lifecycle --project <project_id> --since 30d --event-kind interaction \
                         --event dashboard_opened [--json]
  logbrew investigate issue <issue_id> [--occurrence <recommended|first|latest|occurrence_id>] \
                         [--json]
  logbrew debug-artifacts upload <path> --project <project_id> --release <release> --environment \
                         <environment> --service <service> [--expect-image-uuid <uuid>]... \
                         [--dry-run] [--json]
  logbrew debug-artifacts lookup --project <project_id> --release <release> --environment \
                         <environment> --service <service> --image-uuid <uuid> --architecture \
                         <architecture> [--json]
  logbrew <issue_id_or_trace_id> explain [--json]
  logbrew set issue <issue_id> resolved [--json]
  logbrew resolve <issue_id> [--json]
  logbrew close <issue_id> [--json]
  logbrew ignore <issue_id> [--json]
  logbrew reopen <issue_id> [--json]

Popular terms: auth, status, health, setup, projects, usage, logs, issues, errors, traces, spans, \
                         metrics, actions, events, releases, environments, support.
Health aliases: logbrew status, logbrew health, logbrew ping, logbrew doctor.
Setup aliases (non-mutating plan): logbrew init, logbrew install, logbrew configure, logbrew sdk.
Authenticated project creation: logbrew projects create <name> --ingest-key-file <path>.
After CLI authentication, project creation requires no dashboard sign-in; the one-time ingest key \
is stored in the chosen owner-only file and never printed.
Existing-project key creation: logbrew projects keys create <project_id> --ingest-key-file <path>.
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
  logbrew login [--provider github|gitlab|bitbucket] [--no-open] [--json]

Starts browser login for the native CLI and stores a private local access/refresh pair.
Authenticated commands refresh local auth once after an expired-token response.
Use --provider github|gitlab|bitbucket to choose the account provider (default: github).
Use --no-open to print the URL without opening a browser.
--json prints the auth handoff without opening a browser.";

/// Logout command help text.
const LOGOUT_HELP: &str = "\
Usage:
  logbrew logout [--json]

Attempts to revoke the stored server session, then always removes both local CLI credentials.
If LOGBREW_TOKEN is set, unset it separately to fully log out.";

/// Setup command help text.
const SETUP_HELP: &str = "\
Usage:
  logbrew setup [--auto] [--yes] [--json]

Detects supported project manifests and prints a non-mutating SDK setup plan.
No files are changed. SwiftPM planning uses compatible releases from 0.1.6 for detected SwiftPM \
or XcodeGen projects.
Python planning detects Django, Flask, and FastAPI from bounded local project metadata. It emits \
the public core and framework packages, the detected pip, uv, poetry, or pipenv command, and \
explicit Python/framework compatibility requirements.
Authenticated project creation (no dashboard sign-in):
  logbrew projects create <name> --ingest-key-file <path> [--runtime <runtime>] \
                           [--environment <environment>] [--json]
Existing-project key creation (no dashboard sign-in):
  logbrew projects keys create <project_id> --ingest-key-file <path> [--label <label>] \
                           [--kind sdk|browser|server|cli] [--json]
Run either command after logbrew login. Each stores the one-time ingest key in the chosen \
owner-only file and never prints the key or its path.
logbrew setup --create-project shows secure project-creation help and does not create a project.
Package: https://github.com/LogBrewCo/sdk.git
Product: LogBrew
Dependency: .package(url: \"https://github.com/LogBrewCo/sdk.git\", from: \"0.1.6\")
Python packages: logbrew-sdk, logbrew-django, logbrew-flask, logbrew-fastapi.
Other detected runtimes use released SDK guidance until a structured CLI install plan is enabled.
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
  logbrew doctor --project <project_id> [--json]
  logbrew whoami [--json]
  logbrew me [--json]
  logbrew auth status [--json]

Status checks API reachability and authentication.
Whoami/me return the authenticated account identity.";

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
  logbrew login [--provider github|gitlab|bitbucket] [--no-open] [--json]
  logbrew auth login [--provider github|gitlab|bitbucket] [--no-open] [--json]
  logbrew status [--json]
  logbrew auth status [--json]
  logbrew auth whoami [--json]
  logbrew auth me [--json]
  logbrew whoami [--json]
  logbrew me [--json]
  logbrew logout [--json]
  logbrew auth logout [--json]

Use login once, status to verify API/auth state, whoami/me to inspect the authenticated account,
and logout to revoke the stored server session when possible and always remove local credentials.
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
Errors include ok, error, message, and next.";

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

Release comparison:
  logbrew deploy ci-run-42 --project <project_id> --release checkout@1 --environment production \
                           --service checkout-api --status succeeded \
                           --started-at 2026-08-10T12:00:00Z \
                           --finished-at 2026-08-10T12:02:00Z --json
  logbrew explain release checkout@1 --project <project_id> --environment production \
                           --service checkout-api --json

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
  logbrew projects create <name> --ingest-key-file <path> [--runtime <runtime>] \
                           [--environment <environment>] [--abandon-retry] [--json]
  logbrew projects keys create <project_id> --ingest-key-file <path> [--label <label>] \
                           [--kind sdk|browser|server|cli] [--abandon-retry] [--json]
  logbrew projects archive <project_id> --yes [--json]
  logbrew setup --create-project [--json]
  logbrew projects setup <project_id> [--runtime <runtime>] [--source api|cli|sdk] \
[--environment <environment>] [--json]

Reads the authenticated active project catalog without mutating project state.
Human output is bounded to project identity, setup status, and latest activity. JSON preserves the \
exact validated bare array.
Project creation, setup status, and project-scoped ingest credentials are backend-owned.
An authenticated CLI can create a project without dashboard sign-in or additional browser auth.
It can also issue a key for an existing project without creating a duplicate project or opening \
the dashboard.
logbrew setup --create-project shows this help and never creates a project.
Project creation stores the one-time ingest key in a new owner-only file before reporting success;
it never prints the one-time ingest key or its file path. An ambiguous attempt reuses the pending retry key only for the exact same request; --abandon-retry starts a new explicit attempt.
Existing-project key creation uses the same storage and retry guarantees with independent private \
retry state.
Builds that cannot prove owner-only file permissions fail before sending the create request.
No local install, quota, or usage state is created.
Project setup uses POST /api/projects/{project_id}/setup/seen and preserves backend setup status JSON.
Project archival requires explicit --yes, removes it from the active project catalog, and returns \
success only for an empty 204 response.
Project-scoped ingest keys stop authorizing new ingestion after archival.
This soft-archive command does not claim hard deletion or restoration.
Project-scoped SDK/ingest credentials are shown only when backend returns one-time credentials.
Never use an account bearer token as SDK or ingest configuration.
Next: run logbrew setup for the current non-mutating local plan.";

/// Backend-owned usage and quota help text.
const USAGE_HELP: &str = "\
Usage:
  logbrew usage [--json]
  logbrew account usage [--json]

Reads authenticated account usage, configured limits, quota state, and reset dates without mutating \
account or billing state.
Human output is bounded to plan, state, totals, limits, the driving limit, and one next step. JSON \
preserves the exact validated account-usage object for agents.
The CLI does not calculate or persist usage/quota state from local files.
Next: run logbrew usage to inspect current account usage.";

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
  logbrew read traces [filters] [--json]
  logbrew read trace <trace_id> [--json]
  logbrew read issue <issue_id> [--json]

Reads historical observability data for agents and developers.
Singular read aliases: logbrew read log, read release, show log, list issue, get release.
Recency counts are limit shortcuts: logbrew last 10 logs or logbrew recent 5 issues.
Use --environment <environment> with logs, issues, actions, releases, or traces.
Use --service <service_name> with logs, issues, actions, or releases.
Filter aliases: --service-name, --env, --project-id, --trace-id, and --distinct-id.";

/// Read logs help text.
const READ_LOGS_HELP: &str = "\
Usage:
  logbrew read logs [--severity error] [--search checkout] [--release <release>] [--environment \
                              production] [--service <service_name>] [--since 24h] [--trace \
                              <trace_id>] [--project <project_id>] [--pagination cursor] [--limit \
                              100] [--json]
  logbrew read logs [filters] --pagination cursor --cursor-time <RFC3339> --cursor-id <uuid> \
                              [--limit 100] [--json]
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
--service-name <service_name> is accepted as an alias for --service <service_name>.
Cursor pagination preserves JSON as {logs,next_cursor}; next_cursor is either {time,id} or null.
Use --pagination cursor alone for the first page. Continue with --cursor-time and --cursor-id from \
                              next_cursor.
Keep the same active filters and pagination limit on every continuation page.
Limit must be a positive whole number.";

/// Read issues help text.
const READ_ISSUES_HELP: &str = "\
Usage:
  logbrew read issues [--release <release>] [--environment production] [--status unresolved] \
                                [--service <service_name>] [--since <24h|7d|RFC3339>] [--project \
                                <project_id>] [--pagination cursor] [--limit 100] [--json]
  logbrew read issues [filters] --pagination cursor --cursor-time <RFC3339> --cursor-id <uuid> \
                                [--limit 100] [--json]
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
--service-name <service_name> is accepted as an alias for --service <service_name>.
Since accepts positive compact durations such as 24h or 7d, or an RFC3339 timestamp such as \
                                2026-05-01T00:00:00Z.
Cursor pagination preserves JSON as {issues,next_cursor}; next_cursor is either {time,id} or null.
Use --pagination cursor alone for the first page. Continue with --cursor-time and --cursor-id from \
                                next_cursor.
Keep the same active filters and pagination limit on every continuation page.
Limit must be a positive whole number.";

/// Read actions help text.
const READ_ACTIONS_HELP: &str = "\
Usage:
  logbrew read actions [--release <release>] [--environment production] [--name checkout_failed] \
                                 [--user <distinct_id>] [--service <service_name>] [--since 24h] \
                                 [--project <project_id>] [--pagination cursor] [--limit 100] \
                                 [--json]
  logbrew read actions [filters] --pagination cursor --cursor-time <RFC3339> --cursor-id <uuid> \
                                 [--limit 100] [--json]
  logbrew events checkout_failed [--release <release>] [--environment production] [--json]

Reads product actions. Use distinct_id to follow one actor or session.
Action/event aliases accept one positional name as the same filter as --name.
--service-name <service_name> is accepted as an alias for --service <service_name>.
Cursor pagination preserves JSON as {actions,next_cursor}; next_cursor is either {time,id} or null.
Use --pagination cursor alone for the first page. Continue with --cursor-time and --cursor-id from \
                                 next_cursor.
Keep the same active filters and pagination limit on every continuation page.
Limit must be a positive whole number.";

/// Read releases help text.
const READ_RELEASES_HELP: &str = "\
Usage:
  logbrew read releases [--release <release>] [--environment production] [--service \
                                  <service_name>] [--since <24h|7d|RFC3339>] [--project \
                                  <project_id>] [--limit 100] [--json]

Reads release summaries with counts for issues, logs, trace spans, and actions.
--service-name <service_name> is accepted as an alias for --service <service_name>.
Since accepts positive compact durations such as 24h or 7d, or an RFC3339 timestamp such as \
                                  2026-05-01T00:00:00Z.
Limit must be a positive whole number.";

/// Recent trace discovery help text.
const READ_TRACES_HELP: &str = "\
Usage:
  logbrew traces [--project <project_id>] [--service <service_name>] [--release <release>] \
                               [--environment <environment>] [--status <error|ok>] \
                               [--since <24h|7d|RFC3339>] \
                               [--min-duration-ms <milliseconds>] [--limit 100] [--json]
  logbrew spans [filters] [--json]
  logbrew latest traces [--limit 100] [--json]

Lists recent distributed traces for incident investigation. JSON preserves the backend bare array.
Status accepts error or ok, case-insensitively. Minimum duration is a non-negative whole number.
Since accepts positive compact durations such as 24h or 7d, or an RFC3339 timestamp such as \
                               2026-05-01T00:00:00Z.
The backend defaults limit to 100 and clamps it to 1..500.
CLI aliases --project-id, --service-name, and --env still serialize only canonical API query keys.
Next: run logbrew trace <trace_id> or logbrew explain trace <trace_id>.";

/// Read trace help text.
const READ_TRACE_HELP: &str = "\
Usage:
  logbrew read trace <trace_id> [--release <release>] [--environment production] [--project \
                               <project_id>] [--json]
  logbrew trace <trace_id> [--release <release>] [--environment production] [--project \
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
  logbrew explain issue <issue_id> [--occurrence <recommended|first|latest|occurrence_id>] [--json]
  logbrew explain log <log_id> [--json]
  logbrew explain action <action_id> [--json]
  logbrew explain trace <trace_id> [--json]
  logbrew explain span <trace_id> <span_id> --project <project_id> --environment <environment> --release <release> [--json]
  logbrew explain release <release> --project <project_id> --environment <environment> --service \
                            <service_name> [--json]
  logbrew explain metric <name> --project <project_id> --since <24h|RFC3339> [--until <RFC3339>] \
                            [--interval <auto|1m|5m|15m|1h|6h|1d>] \
                            [--group-by <none|service_name|release|environment>] \
                            [--service <service_name>] [--release <release>] \
                            [--environment <environment>] [--series-limit <1-20>] [--json]
  logbrew explain <issue_id_or_trace_id> [--json]
  logbrew issue <issue_id> explain [--occurrence <recommended|first|latest|occurrence_id>] [--json]
  logbrew trace <trace_id> explain [--json]
  logbrew <issue_id_or_trace_id> explain [--json]

Fetches bounded failure, product-action status, correlation, timeline, evidence, or metric-investigation context for humans and \
                            AI agents.
Action explanations preserve privacy-safe subject classification and session-presence evidence, \
                            connect exact trace/span and nearby signals, and never return raw actor or session identifiers.
Span explanations bind a non-zero trace/span pair to an exact project, environment, and release; \
                            they include selected-branch topology, a retained same-release peer baseline, \
                            exact-span logs, same-trace signals, ordered evidence, and explicit limitations without claiming root cause.
Metric explanations preserve gauge/counter/histogram semantics, compare the immediately preceding \
                            equal window, expose the latest privacy-bounded raw sample with SDK, runtime context, \
                            structured metadata, exact trace/span links when captured, and nearby completed deployments, \
                            and never claim that an observed change or deployment proves an anomaly or root cause.
Issue explanations use the backend-recommended context-rich retained occurrence by default. Use \
                            --occurrence first, latest, recommended, or a retained occurrence UUID \
                            from a previous occurrence receipt to inspect another exact event. JSON emits the exact validated \
                            schema-version-4 issue response with explicit selection, candidate \
                            coverage, status activity, server-observed regression evidence, a \
                            zero-filled occurrence trend, and bounded release, environment, service, and SDK distributions.
Metric explanations preserve gauge, delta-counter, histogram, and cumulative-stream semantics; \
                            they never invent reset-unsafe rates.
Release investigation requires the exact project, environment, and service identity returned by \
                            logbrew read releases. Release actions separate approximate typed-user, \
                            anonymous-subject, and session cardinalities from exact subject-kind \
                            capture coverage; legacy, missing, and historical events remain visible. \
                            The newest exact deployment boundary is aligned to the prior successful \
                            distinct release when captured, with raw signal-count deltas, trace error-rate \
                            direction, observation windows, and explicit capture/retry guidance. These \
                            observations are not traffic-normalized and never claim deployment causality.
Pasted UUID/issue_* values are treated as issues; 32-hex/trace_* values are treated as traces.";

/// Completed deployment capture help text.
const DEPLOY_HELP: &str = "\
Usage:
  logbrew deploy <deployment_id> --project <project_id> --release <release> --environment \
                           <environment> --service <service_name> --status <succeeded|failed> \
                           --started-at <rfc3339> --finished-at <rfc3339> [--commit-sha <sha>] \
                           [--json]

Records one completed deployment boundary for release timelines and before/after comparisons.
Use the exact project, release, environment, and service values sent by runtime telemetry.
deployment_id is caller-owned and idempotent: retry the exact same record safely; use a new id \
                           for different content.
The command requires account authentication, never accepts a project ingest key, and validates \
                           the complete versioned receipt before printing it.
Run this from release automation after the deployment attempt reaches succeeded or failed.";

/// Product-analytics command overview.
const ANALYTICS_HELP: &str = "\
Usage:
  logbrew analytics overview --help
  logbrew analytics properties --help
  logbrew analytics compare --help
  logbrew analytics paths --help
  logbrew analytics funnel --help
  logbrew analytics retention --help
  logbrew analytics lifecycle --help

Overview discovers captured activity, exact event names, surfaces, capture quality, and analysis \
readiness before a more specific query.
Properties discovers privacy-safe typed-context and custom-tag keys, aggregate capture coverage, \
and migration gaps without exposing property values or identities.
Compare measures one exact outcome across two through four named service, release, environment, or \
exact-property segments, with the first segment as a descriptive baseline.
Paths shows the most common aggregate journeys around one exact event without returning session \
or user identifiers.
Funnels measure exact ordered conversion and drop-off across two through eight product events \
using explicit session or typed opaque user boundaries.
Retention measures whether typed opaque users return after one exact start event, with \
maturity-aware denominators that do not classify unobservable users as churned.
Lifecycle classifies typed opaque users as new in observed history, returning, resurrected, or \
dormant for one exact event, with explicit history and capture bounds.
All commands return bounded human guidance and exact validated JSON contracts for AI agents. \
Overview uses schema version 2 for exhaustive subject-kind coverage; the other commands use \
schema version 1.
Next: start with overview, inspect properties before property-based comparisons, then choose compare \
for context differences, paths for journeys, funnels for conversion, retention for return behavior, \
or lifecycle for population change.";

/// Product-analytics project overview help text.
const ANALYTICS_OVERVIEW_HELP: &str = "\
Usage:
  logbrew analytics overview --project <project_id> --since <24h|RFC3339> [options] [--json]

Options:
  --until <RFC3339>       Exclusive upper time bound.
  --interval <value>      auto, 1m, 5m, 15m, 1h, 6h, or 1d (default: auto).
  --service <name>        Exact service context.
  --release <release>     Exact release context.
  --environment <name>   Exact environment context.
  --top-limit <1-20>      Action, surface, and exact-event rows per ranking (default: 10).

Shows bounded action volume, active identified users, typed anonymous subjects, explicit sessions, \
classified page views, screen views and interactions, time coverage, top surfaces, and exact event \
names.
Capture-quality counts disclose exhaustive subject-kind coverage: typed users, typed anonymous \
subjects, legacy-untyped IDs, missing context, historical-unindexed events, unsessionized events, \
untraced events, unnamed events, and unclassified activity remain separate.
Unique user, anonymous-subject, session, name, and surface counts are approximate; event and \
coverage totals are exact within the selected window.
Human output is terminal-safe and recommends the next useful capture or analysis action.
JSON emits the exact validated schema-version-2 response without user or session identifiers.
Next: use exact event names from the overview with analytics compare, paths, funnel, retention, or \
lifecycle.";

/// Product-analytics privacy-safe property catalog help text.
const ANALYTICS_PROPERTIES_HELP: &str = "\
Usage:
  logbrew analytics properties --project <project_id> --since <24h|RFC3339> [options] [--json]

Options:
  --until <RFC3339>       Exclusive upper time bound.
  --service <name>        Exact service context.
  --release <release>     Exact release context.
  --environment <name>   Exact environment context.
  --limit <1-50>          Highest-volume safe property keys (default: 20).

Shows bounded standard runtime, framework, operating-system, device, and application keys plus \
non-sensitive tag.* keys captured on classified product events. Results include exact per-key event \
coverage, approximate distinct-value counts, current-index and historical migration coverage, \
privacy filtering, truncation, and the next useful action.
Property values and user, session, subject, trace, network-address, or credential identifiers are \
never returned by this command. Use a value already known by your application with \
--segment-property <segment-key>:<property-key>=<exact-value> in analytics compare.
Human output is terminal-safe. JSON emits the exact validated schema-version-1 aggregate response.
Next: copy an exact returned key into an analytics compare property predicate; verify value spelling \
and case locally because catalog discovery intentionally does not reveal values.";

/// Product-analytics context segment comparison help text.
const ANALYTICS_COMPARE_HELP: &str = "\
Usage:
  logbrew analytics compare --project <project_id> --since <24h|RFC3339> \
      --target-kind <page-view|screen-view|interaction> --target-event <name> \
      --segment <key>=<label> --segment <key>=<label> [--segment <key>=<label>]... \
      [options] [--json]

Options:
  --until <RFC3339>                    Exclusive upper time bound.
  --interval <value>                   auto, 1m, 5m, 15m, 1h, 6h, or 1d (default: auto).
  --unit <session|identified-user>     Eligibility and reach boundary (default: session).
  --segment-service <key>=<value>      Exact service filter for one declared segment.
  --segment-release <key>=<value>      Exact release filter for one declared segment.
  --segment-environment <key>=<value>  Exact environment filter for one declared segment.
  --segment-property <segment>:<key>=<value>
                                        Exact safe property predicate; repeat up to four per segment.

Repeat --segment two through four times in comparison order. The first segment is the descriptive \
baseline. Keys use lowercase letters, numbers, underscore, or hyphen; keyed filter flags can \
appear before or after their matching declaration.
Property predicates are exact case-sensitive bounded string matches and combine with logical AND. \
Copy keys from analytics properties; property values remain application-known and are not suggested. \
Each segment must have a unique exact service, release, environment, and property-filter \
combination. Segments are \
evaluated independently and may overlap, so their totals must not be added as a population split.
Reach is the fraction of eligible explicit sessions or opaque subjects explicitly typed as users \
that performed the exact target. Unique-unit counts are approximate; event and coverage totals are \
exact within the fully evaluated window. Relative lift is descriptive only: no causal inference or \
statistical significance test is claimed.
Human output shows segment reach, baseline differences, missing-key coverage versus nonmatching-value \
coverage, capture and trace-link coverage, bounded \
time-series evidence, interpretation limits, and the next useful action. JSON emits the exact \
validated schema-version-1 response without raw session or subject identifiers.
Next: choose an exact captured target from Product Analytics overview, compare meaningful exact \
contexts, then inspect paths and correlated traces in the weakest segment.";

/// Product-analytics path exploration help text.
const ANALYTICS_PATHS_HELP: &str = "\
Usage:
  logbrew analytics paths following --project <project_id> --since <24h|RFC3339> \
      --anchor-kind <page-view|screen-view|interaction> --anchor-event <name> [options] [--json]
  logbrew analytics paths preceding --project <project_id> --since <24h|RFC3339> \
      --anchor-kind <page-view|screen-view|interaction> --anchor-event <name> [options] [--json]

Options:
  --until <RFC3339>       Exclusive upper time bound.
  --service <name>        Exact service context.
  --release <release>     Exact release context.
  --environment <name>   Exact environment context.
  --property <key=value>  Exact safe anchor predicate; repeat up to four times (logical AND).
  --depth <1-8>           Adjacent named events (default: 4).
  --path-limit <1-20>     Highest-volume aggregate paths (default: 10).
  --keep-repeated         Keep consecutive identical events instead of collapsing them.

Shows the most common named product-event journeys immediately after or before one exact anchor.
Aliases after/before map to following/preceding; page_view and screen_view are accepted kind aliases.
Property predicates are exact case-sensitive matches applied only to the anchor occurrence. Copy \
keys from analytics properties; values remain application-known and are not suggested or repeated \
in human output. Surrounding events remain unfiltered so the session journey stays truthful.
Results use explicit opaque session boundaries and never return session or user identifiers.
Human output highlights represented sessions, property-ready versus missing-key and value-mismatch \
populations, trace-link coverage, bounded trace investigation commands, capture gaps, truncation, \
and the next useful action. Trace exemplars are same-trace evidence, never a root-cause claim, and \
their usefulness depends on retained spans.
JSON emits the exact validated schema-version-1 response for AI agents.
Next: choose an exact captured page, screen, or interaction from Product Analytics, optionally \
classify its anchor with safe properties, then inspect a returned trace exemplar.";

/// Product-analytics ordered funnel help text.
const ANALYTICS_FUNNEL_HELP: &str = "\
Usage:
  logbrew analytics funnel --project <project_id> --since <24h|RFC3339> \
      --step <page-view|screen-view|interaction> <name> \
      --step <page-view|screen-view|interaction> <name> [--step <kind> <name>]... \
      [options] [--json]

Options:
  --until <RFC3339>                 Exclusive upper time bound.
  --service <name>                  Exact service context.
  --release <release>               Exact release context.
  --environment <name>             Exact environment context.
  --unit <session|identified-user>  Counting boundary (default: session).
  --conversion-window <duration>   First-to-final window as seconds, 30m, 1h, or 1d.

Measures where explicit sessions or opaque subjects explicitly typed as users enter, progress, \
complete, and drop off across two through eight exact classified events. Repeat --step in the \
required order. \
page_view, screen_view, identified_user, page, screen, and user are accepted aliases.
Every later step must have a strictly greater timestamp than the prior match and the final step \
must remain inside the conversion window from the first match. One event cannot satisfy multiple \
steps.
Session funnels count visits or app sessions, not people. Identified-user funnels require stable \
application-supplied opaque subject IDs with context.subject.kind=user and can connect separate \
sessions. Anonymous, legacy-untyped, historical-unindexed, and missing subjects are reported as \
capture gaps. Raw IDs are never returned.
Human output shows candidate, entered, and completed units; every step's conversion and drop-off; \
capture coverage; interpretation limits; and the next useful action. JSON emits the exact \
validated schema-version-1 response for AI agents.
Next: use exact captured event names from Product Analytics overview and inspect correlated traces \
around the earliest material drop-off.";

/// Product-analytics retention help text.
const ANALYTICS_RETENTION_HELP: &str = "\
Usage:
  logbrew analytics retention --project <project_id> --since <24h|RFC3339> \
      --start-kind <page-view|screen-view|interaction> --start-event <name> \
      --return-kind <page-view|screen-view|interaction> --return-event <name> [options] [--json]

Options:
  --until <RFC3339>                 Exclusive upper time bound.
  --service <name>                  Exact service context.
  --release <release>               Exact release context.
  --environment <name>             Exact environment context.
  --interval <hour|day|week|thirty-day>
                                     Fixed period and cohort width (default: day).
  --interval-count <1-31>           Zero-based periods to evaluate (default: 10).
  --mode <return-on|return-on-or-after>
                                     Exact-period or rolling retention (default: return-on).
  --cohort-mode <first-in-range>    First matching start inside the query range (default).

Returns a typed-user retention curve and a query-relative cohort matrix for two exact classified \
events. page_view, screen_view, exact, rolling, 1h, 1d, 1w, and 30d are accepted aliases.
Only stable opaque subject IDs with context.subject.kind=user qualify. Anonymous, legacy-untyped, \
historical-unindexed, and missing subjects are reported as capture gaps. Raw IDs are never \
returned.
Returns must occur strictly after each subject's start anchor. Maturity-aware denominators exclude \
subjects whose selected period cannot yet be observed instead of reporting them as churned.
thirty-day is a fixed 30-day duration, not a calendar month. first-in-range does not prove a \
subject's first-ever historical start.
Human output shows headline return rate, every eligible period, cohort maturity, capture gaps, and \
the next useful action. JSON emits the exact validated schema-version-1 response for AI agents.
Next: use exact captured start and return event names from Product Analytics overview.";

/// Product-analytics lifecycle help text.
const ANALYTICS_LIFECYCLE_HELP: &str = "\
Usage:
  logbrew analytics lifecycle --project <project_id> --since <24h|RFC3339> \
      --event-kind <page-view|screen-view|interaction> --event <name> [options] [--json]

Options:
  --until <RFC3339>                 Exclusive upper time bound.
  --service <name>                  Exact service context.
  --release <release>               Exact release context.
  --environment <name>             Exact environment context.
  --interval <hour|day|week|thirty-day>
                                     Fixed lifecycle period; the backend chooses by range when omitted.
  --history-periods <2-31>          Complete periods before since (default: 2; max history: 62 days).

Returns lifecycle state for stable opaque subjects explicitly typed as users performing one exact \
classified event.
New in observed history means active now with no earlier matching event between history_since and \
the current bucket; it does not prove a lifetime-new user. Returning means active in the current \
and immediately previous fixed period. Resurrected means active now after a gap. Dormant means \
active in the previous period but not the current period.
States are disjoint, the bounded history window is always visible, and an incomplete final bucket \
is marked provisional. thirty-day is a fixed 30-day duration, not a calendar month.
Typed-user identity, event-name, session, trace, and history coverage qualify every conclusion. \
Anonymous, legacy-untyped, historical-unindexed, and missing subjects remain excluded. Raw subject \
IDs are never returned.
Human output shows every lifecycle bucket, population change, capture gaps, partial-period status, \
and the next useful action. JSON emits the exact validated schema-version-1 response for AI agents.
Next: choose one exact captured page, screen, or interaction name from Product Analytics overview.";

/// Server-directed issue investigation help text.
const INVESTIGATE_HELP: &str = "\
Usage:
  logbrew investigate issue <issue_id> [--occurrence <recommended|first|latest|occurrence_id>] \
                         [--json]

Reads one schema-version-4 bounded issue investigation with explicit selected, first, latest, and \
recommended occurrence receipts; exception, frames, breadcrumbs, typed runtime context, honest \
cause and fix assessments, approximate affected-user \
coverage and limitations, trace, related logs, actions, metric exemplars, release scope, evidence \
completeness, bounded status activity and server-observed regression evidence, and prioritized next \
actions. It also returns an exact zero-filled occurrence trend and bounded release, environment, \
service, and SDK distributions with explicit availability and truncation receipts.
The default is the bounded context-rich recommendation. --occurrence accepts first, latest, \
recommended, or an exact retained occurrence UUID copied from a previous occurrence receipt.
The command is read-only and uses the same contract as logbrew explain issue.
Human output is bounded and marks application telemetry as untrusted evidence. JSON emits the exact \
validated schema-version-4 response for AI agents.";

/// Apple native debug-artifact command help text.
const NATIVE_DEBUG_ARTIFACTS_HELP: &str = "\
Usage:
  logbrew debug-artifacts upload <path> --project <project_id> --release <release> \
                         --environment <environment> --service <service> \
                         [--expect-image-uuid <uuid>]... [--dry-run] [--json]
  logbrew debug-artifacts lookup --project <project_id> --release <release> \
                         --environment <environment> --service <service> --image-uuid <uuid> \
                         --architecture <arm64|arm64e|x86_64> [--json]

Discovers and validates Apple dSYM, ZIP, or Mach-O debug objects locally, uploads every supported \
exact identity, and verifies each identity with an authenticated lookup. Optional repeated \
--expect-image-uuid values require an exact discovered UUID set for release automation. --dry-run \
performs the same local validation without authentication or network access. Upload limits are \
256 MiB per thin debug object and 512 MiB per source file or resumable upload session. Large \
uploads use fixed 4 MiB resumable chunks. Local paths and filenames are never included in output \
or API metadata.";

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

/// Support-ticket workflow help text.
const SUPPORT_HELP: &str = "\
Usage:
  logbrew support create --category <category> --title <title> --description <description> \
                           [--project <project_id>] [--environment <environment>] \
                           [--runtime <runtime>] [--framework <framework>] \
                           [--sdk-package <package>] [--sdk-version <version>] \
                           [--release <release>] [--trace-id <trace_id>] [--event-id <event_id>] \
                           [--diagnostics] [--json]
  logbrew support list [--project <project_id>] [--status <status>] [--source <source>] \
                         [--category <category>] [--release <release>] [--limit 100] \
                         [--pagination cursor] [--json]
  logbrew support list [filters] --pagination cursor --cursor-time <RFC3339> \
                         --cursor-id <ticket_id> [--json]
  logbrew support show <ticket_id> [--json]
  logbrew support context <ticket_id> [--json]
  logbrew support reply <ticket_id> --context <text> --retry-key <key> [--diagnostics] [--json]
  logbrew support close <ticket_id> [--json]
  logbrew support reopen <ticket_id> [--json]

Creates, reads, adds requested context to, closes, and reopens authenticated account support \
                         tickets. Creation source is \
                         always cli.
Categories: sdk_install_failure, ingest_failure, auth_failure, project_setup, dashboard_issue, \
                         docs_confusion, cli_issue, mobile_issue, billing_question, other.
--diagnostics adds only binary, CLI version, operating system, and architecture. It never reads \
                         arbitrary environment variables or files.
Cursor continuations repeat --pagination cursor, all active filters, and the paired cursor from \
                         next_cursor. JSON preserves the server response exactly.
Context replies require a retry key. Reuse the key only when retrying the exact same context. \
                         Chat, messages, and internal notes are not part of this command.";
