# Agent Instructions

This is the public LogBrew CLI repository. Everything committed here is public.

## Non-Negotiables

- Never commit secrets, tokens, private hostnames, private IP addresses, private
  repo names, private deployment details, database credentials, screenshots with
  secrets, or private backend implementation details.
- Keep this repo CLI-only. Do not add backend, mobile app, SDK, infrastructure,
  deployment, backup, ClickHouse schema, or private ops work here.
- Before every commit, run `bash scripts/pre-commit.sh`. This intentionally
  runs `bash scripts/confidentiality-check.sh` first, then
  `bash scripts/check-all.sh`.
- Preserve token safety: CLI output may say whether auth exists and where it
  came from, but must never print token material.
- Preserve stable JSON for agents and readable human output with concrete
  `Next:` recovery steps.
- Treat bare public discovery terms as help, not dead ends, when no required
  identifier is present. For example, `logbrew traces --json` and
  `logbrew spans --json` should return trace help while ID-bearing forms still
  read the trace.
- The CLI is a Rust native binary. Package-manager releases should publish
  native cargo-dist artifacts through Blacksmith-backed GitHub Actions: GitHub
  Releases, shell, PowerShell, npm, Homebrew, MSI, and crates.io. Registry
  publishing needs only public secret names in this repo:
  `CARGO_REGISTRY_TOKEN`, `NPM_TOKEN`, and `HOMEBREW_TAP_TOKEN`.
- Preserve native-binary introspection: `logbrew version --json` must expose
  `binary`, `os`, and `arch` without making human `logbrew version` verbose.
- The public Homebrew tap already exists at `LogBrewCo/homebrew-tap`; do not
  recreate it. Future release work should only verify tap access and the
  `HOMEBREW_TAP_TOKEN` secret name.
- Keep setup/init/install/configure/sdk non-mutating until installation is
  truly implemented. Human setup output must say `Mode: non-mutating plan`,
  `No files changed.`, and `Install: not ready`.
- Use TDD for CLI behavior changes: add or update the failing test first, run it
  to see the failure, implement, then rerun focused tests and `scripts/check-all.sh`.
- Keep dependencies current through official release notes and advisories.

## Verification

```bash
bash scripts/pre-commit.sh
```

## Public Boundary

Allowed public information:

- Public command names, flags, examples, and API paths.
- Public environment variable names: `LOGBREW_API_URL`, `LOGBREW_TOKEN`.
- Public default API URL: `https://api.logbrew.co`.

Forbidden public information:

- Private backend internals, private repo paths, private host users, private network
  addresses, deployment scripts, backup configuration, database credentials,
  production logs, or any non-public operational detail.
