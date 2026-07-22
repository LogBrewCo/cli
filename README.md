# LogBrew CLI

Public command-line interface for LogBrew.

The CLI is built for humans and coding agents: stable JSON output, readable
human output, clear `Next:` recovery steps, and token-safe diagnostics.

## Install

Use one of the published package-manager installs:

```bash
cargo install logbrew-cli
npm install -g logbrew-cli
brew install LogBrewCo/tap/logbrew
```

Or install the native GitHub Release artifact directly:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/LogBrewCo/cli/releases/latest/download/logbrew-cli-installer.sh | sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/LogBrewCo/cli/releases/latest/download/logbrew-cli-installer.ps1 | iex"
```

Windows users can also download the latest MSI from the GitHub Release assets.

Cargo installs and source builds require Rust 1.87 or newer. The npm,
Homebrew, shell, PowerShell, and MSI installers use native release artifacts and
do not require a local Rust toolchain.

For development from the public repository:

```bash
cargo install --git https://github.com/LogBrewCo/cli logbrew-cli
```

## Distribution

LogBrew CLI is a Rust native binary. Cargo builds a platform-native `logbrew`
executable for the selected target; npm, Homebrew, shell, PowerShell, and MSI
installers are wrappers around those native release artifacts.

Release publishing is handled by GitHub Actions:

- GitHub Releases: native archives for Linux x64/ARM64, macOS x64/ARM64, and
  Windows x64 on Blacksmith runners.
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
logbrew logs --release checkout@1 --environment production
logbrew issues open --json
logbrew explain issue issue_123
logbrew watch --json
logbrew watch --severity error,critical --json
```

Run `logbrew examples` for a compact first-run, troubleshooting, live watch, and
agent JSON workflow guide.

The default API URL is `https://api.logbrew.co`. Override it with
`LOGBREW_API_URL` when testing against another LogBrew API.

Authentication uses either `LOGBREW_TOKEN` or the secured local access/refresh
pair created by `logbrew login`. Interactive login opens GitHub, receives the
result on a loopback-only callback, and stores the pair under `~/.logbrew`.
Authenticated commands rotate local credentials once after an expired-token
response; environment tokens are never persisted or refreshed. `--json` and
`--no-open` remain non-mutating handoff modes. CLI output never prints token
material.

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
