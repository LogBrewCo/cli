# Agent Instructions

This is the public LogBrew CLI repository. Everything committed here is public.

## Non-Negotiables

- Never commit secrets, tokens, private hostnames, private IP addresses, private
  repo names, private deployment details, database credentials, screenshots with
  secrets, or private backend implementation details.
- Keep this repo CLI-only. Do not add backend, mobile app, SDK, infrastructure,
  deployment, backup, ClickHouse schema, or private ops work here.
- Before every commit, run `bash scripts/confidentiality-check.sh`.
- Preserve token safety: CLI output may say whether auth exists and where it
  came from, but must never print token material.
- Preserve stable JSON for agents and readable human output with concrete
  `Next:` recovery steps.
- Keep setup/init/install/configure/sdk non-mutating until installation is
  truly implemented. Human setup output must say `Mode: non-mutating plan`,
  `No files changed.`, and `Install: not ready`.
- Use TDD for CLI behavior changes: add or update the failing test first, run it
  to see the failure, implement, then rerun focused tests and `scripts/check-all.sh`.
- Keep dependencies current through official release notes and advisories.

## Verification

```bash
bash scripts/confidentiality-check.sh
cargo fmt --all -- --check
cargo clippy --lib --bin logbrew --all-features -- -D warnings
cargo test --all-targets --all-features
bash scripts/check-all.sh
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
