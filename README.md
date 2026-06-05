# LogBrew CLI

Public command-line interface for LogBrew.

The CLI is built for humans and coding agents: stable JSON output, readable
human output, clear `Next:` recovery steps, and token-safe diagnostics.

## Install From Source

```bash
cargo install --git https://github.com/LogBrewCo/cli logbrew-cli
```

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
bash scripts/confidentiality-check.sh
bash scripts/check-all.sh
```

Public-repo rule: do not add private backend code, private hostnames, private
IP addresses, secrets, deployment files, database configuration, or private
operational details here. Keep backend implementation in the private LogBrew
repo.
