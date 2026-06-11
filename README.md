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
logbrew status
logbrew login
logbrew logs --release checkout@1 --environment production
logbrew issues open --json
logbrew explain issue issue_123
```

The default API URL is `https://api.logbrew.co`. Override it with
`LOGBREW_API_URL` when testing against another LogBrew API.

Authentication uses either `LOGBREW_TOKEN` or the local token file created by
`logbrew login`. CLI output must never print token material.

## Development

```bash
bash scripts/pre-commit.sh
```

Public-repo rule: keep this repository CLI-only. Do not add backend code,
hostnames, IP addresses, secrets, deployment files, database configuration, or
operational details here.
