<p align="center">
  <img src="https://raw.githubusercontent.com/LogBrewCo/sdk/895b55798587a9eca5cabf8ebc2bbe8b4d55bd12/assets/brand/logbrew-logo-transparent-512.png" alt="LogBrew" width="128">
</p>

# LogBrew CLI

Public command-line interface for LogBrew.

The CLI is built for humans and coding agents: stable JSON output, readable
human output, clear `Next:` recovery steps, and token-safe diagnostics.

## Install or update

For a new package-manager installation:

```bash
cargo install logbrew-cli
npm install -g logbrew-cli@latest
brew install LogBrewCo/tap/logbrew
```

Existing installations do not update themselves. Refresh through the same
package manager before relying on a newly documented setup capability:

```bash
cargo install --locked logbrew-cli
npm install -g logbrew-cli@latest
brew update && brew upgrade LogBrewCo/tap/logbrew
```

The native GitHub Release installers can be run again to replace their prior
binary:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/LogBrewCo/cli/releases/latest/download/logbrew-cli-installer.sh | sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/LogBrewCo/cli/releases/latest/download/logbrew-cli-installer.ps1 | iex"
```

Windows users can also install the latest MSI from the GitHub Release assets.
After any update, compare `logbrew version --json` with the
[latest public release](https://github.com/LogBrewCo/cli/releases/latest).
CMake setup planning requires CLI 0.1.39 or newer; correct Objective-C
classification for XcodeGen projects requires CLI 0.1.40 or newer.

Cargo installs and source builds require Rust 1.87 or newer. The npm,
Homebrew, shell, PowerShell, and MSI installers use native release artifacts and
do not require a local Rust toolchain.

For development from this public repository:

```bash
cargo install --git https://github.com/LogBrewCo/cli logbrew-cli
```

## Distribution

LogBrew CLI is a Rust native binary. Cargo builds a platform-native `logbrew`
executable for the selected target; npm, Homebrew, shell, PowerShell, and MSI
installers are wrappers around those native release artifacts.

Release publishing is handled by GitHub Actions:

- GitHub Releases: native archives for Linux x64/ARM64, macOS x64/ARM64, and
  Windows x64 on GitHub-hosted runners.
- Installers: shell, PowerShell, npm package, Homebrew formula, and Windows MSI.
- Package managers: crates.io and npm via trusted publishing/OIDC, and Homebrew
  via the `LogBrewCo/homebrew-tap` formula repository.

The CLI package surface is intentionally separate from the language and
framework SDK packages in `LogBrewCo/sdk`. SDK packages such as JavaScript
framework integrations, Python framework middleware, Swift, .NET, Go, Java,
Kotlin, Ruby, Rust SDK crates, and Unity packages remain SDK-owned. This repo
only publishes the `logbrew` CLI binary and its install wrappers.

Trusted publishing requires the npm package and crates.io crate to already
exist, so brand-new package names need one manual first publish before CI release
tags can publish future versions without long-lived registry tokens. Homebrew
publishing requires the GitHub Actions secret `HOMEBREW_TAP_TOKEN`.

Before pushing a release tag, run the release preflight:

```bash
bash scripts/release-preflight.sh vX.Y.Z
```

The preflight checks the tag/version match, clean synced `main`, public
crates.io/npm package bootstrap and version collisions, the public Homebrew tap
repository, green CI, required GitHub Actions secret names, and existing
release/tag collisions.

## Basic Usage

```bash
logbrew examples
logbrew status
logbrew login
logbrew login --provider gitlab
logbrew whoami
logbrew logout
logbrew logs --release checkout@1 --environment production
logbrew issues open --json
logbrew explain issue issue_123
logbrew deploy ci-run-42 --project <project_id> --release checkout@1 --environment production --service checkout-api --status succeeded --started-at 2026-08-10T12:00:00Z --finished-at 2026-08-10T12:02:00Z --json
logbrew explain release checkout@1 --project <project_id> --environment production --service checkout-api
logbrew watch --json
logbrew watch --severity error,critical --json
```

Run `logbrew examples` for a compact first-run, troubleshooting, live watch, and
agent JSON workflow guide.

The default API URL is `https://api.logbrew.co`. Override it with
`LOGBREW_API_URL` when testing against another LogBrew API.

Authentication uses either `LOGBREW_TOKEN` or the secured local access/refresh
pair created by `logbrew login`. Interactive login supports GitHub (the
default), GitLab, and Bitbucket through
`logbrew login --provider github|gitlab|bitbucket`, receives the result on a
loopback-only callback, and stores the pair under `~/.logbrew`. `logbrew
whoami` and `logbrew me` return the authenticated account identity.
Authenticated commands rotate local credentials once after an expired-token
response; environment tokens are never persisted or refreshed. `--json` and
`--no-open` remain non-mutating handoff modes. CLI output never prints token
material. `logbrew logout` attempts to revoke the stored refresh-backed server
session and always removes local credentials; `LOGBREW_TOKEN` must be unset
separately.

### Plan SDK installation without changing files

Run `logbrew setup --json` from a project directory to detect nearby manifests
and receive a stable, non-mutating installation plan. SwiftPM and XcodeGen
projects receive the released Swift package declaration. Python projects
receive a package-index plan for the core SDK or the detected Django, Flask, or
FastAPI integration, including the pip, uv, poetry, or pipenv command. Bundler
projects receive the released Ruby package plan, and Rails applications are
identified from `config/application.rb` without reading application source.
SvelteKit projects receive the public core, browser, and Svelte integration
packages with the detected npm, pnpm, yarn, or bun command. Detection requires
an exact `@sveltejs/kit` dependency in a standard `package.json` dependency map.
React and Express dependencies receive the released browser/React or
Node/Express packages. Mixed projects receive one deduplicated install command
plus separate browser and server key scopes and service-name requirements.

Python plans report the public package compatibility requirements separately.
Review them before running the command, especially when a library supports
older Python or framework versions. The CLI reads only bounded local metadata,
does not install a package, does not change a manifest, and does not require
authentication for this plan.

### Create a project without dashboard sign-in

After `logbrew login`, the same authenticated CLI session can create an
account-owned project and its first project-scoped ingest key. No additional
dashboard or browser sign-in is required.

Choose a new file inside an existing owner-only directory. On macOS and Linux,
the owner-only directory created by CLI login is a suitable destination:

```bash
logbrew projects create "FastAPI Template" \
  --runtime python \
  --environment development \
  --ingest-key-file "$HOME/.logbrew/fastapi-template.key" \
  --json
```

The command writes the one-time ingest key to that file before reporting
success. It never prints the key or its path. Its JSON response contains the
new project ID; use that ID with `logbrew doctor --project <project_id>` to
inspect setup readiness. Run `logbrew projects --help` for the complete
security and retry contract.

### Create an ingest key for an existing project

Use the authenticated CLI when a new app or service needs its own key but the
LogBrew project already exists:

```bash
logbrew projects keys create <project_id> \
  --kind sdk \
  --label "Mobile SDK key" \
  --ingest-key-file "$HOME/.logbrew/mobile-sdk.key" \
  --json
```

The default kind is `sdk`; supported kinds are `sdk`, `browser`, `server`, and
`cli`. The command sends one idempotent account-authenticated request and
stores the one-time key in a new owner-only file before reporting success. It
never prints the key or its path and does not create a duplicate project. An
ambiguous retry reuses the same request and idempotency key. Use
`--abandon-retry` only to intentionally discard that pending attempt.

### Archive an inactive project safely

Archive an account-owned project only after reviewing its UUID:

```bash
logbrew projects archive <project_id> --yes --json
```

The command requires explicit `--yes`, sends one account-authenticated archive
request, and reports success only after an empty `204` response. The project
then leaves the active catalog and its project-scoped ingest keys no longer
authorize new ingestion. This is a soft archive; the CLI makes no hard-delete
or restoration promise.

For AI sessions, the default mode should be checking only when requested because
it uses fewer AI tokens. `logbrew watch --json` opens a live WebSocket stream for
the current session, and `logbrew watch --severity error,critical --json`
filters live logs/issues client-side to actionable severities. The watch stream
reconnects after transient disconnects with a fresh feed ticket and backoff.

## Development

```bash
bash scripts/pre-commit.sh
```

Public-repo rule: keep this repository CLI-only. Do not add backend code,
hostnames, IP addresses, secrets, deployment files, database configuration, or
operational details here.
