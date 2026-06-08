# LogBrew CLI

Public command-line interface for LogBrew.

The CLI is built for humans and coding agents: stable JSON output, readable
human output, clear `Next:` recovery steps, and token-safe diagnostics.

## Install

Until the first packaged release is published, install from source:

```bash
cargo install --git https://github.com/LogBrewCo/cli logbrew-cli
```

After the first release, package-manager installs should use the standard
distribution surfaces:

```bash
cargo install logbrew-cli
npm install -g logbrew-cli
brew install LogBrewCo/tap/logbrew
```

## Distribution

LogBrew CLI is a Rust native binary. Cargo builds a platform-native `logbrew`
executable for the selected target; npm, Homebrew, shell, PowerShell, and MSI
installers are wrappers around those native release artifacts.

Release publishing is handled by GitHub Actions on Blacksmith runners:

- GitHub Releases: native archives for Linux x64/ARM64, macOS x64/ARM64, and
  Windows x64.
- Installers: shell, PowerShell, npm package, Homebrew formula, and Windows MSI.
- Package managers: crates.io via `cargo publish`, npm via `npm publish`, and
  Homebrew via the `LogBrewCo/homebrew-tap` formula repository.

Publishing requires these GitHub Actions secrets before pushing a release tag:
`CARGO_REGISTRY_TOKEN`, `NPM_TOKEN`, and `HOMEBREW_TAP_TOKEN`.

Before pushing a release tag, run the release preflight:

```bash
bash scripts/release-preflight.sh v0.1.0
```

The preflight checks the tag/version match, clean synced `main`, public
crates.io/npm version collisions, the public Homebrew tap repository, green CI,
missing GitHub Actions secret names, and existing release/tag collisions.

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

Public-repo rule: do not add private backend code, private hostnames, private
IP addresses, secrets, deployment files, database configuration, or private
operational details here. Keep backend implementation in the private LogBrew
repo.
