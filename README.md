<p align="center">
  <img src="https://raw.githubusercontent.com/LogBrewCo/sdk/895b55798587a9eca5cabf8ebc2bbe8b4d55bd12/assets/brand/logbrew-logo-transparent-512.png" alt="LogBrew" width="128">
</p>

# LogBrew CLI

Public command-line interface for LogBrew. The npm package is `logbrew-cli`,
the crates.io crate is `logbrew-cli`, and the installed command is `logbrew`.

The CLI is built for humans and coding agents: stable JSON output, readable
human output, clear `Next:` recovery steps, and token-safe diagnostics.

## Install or update

For a new package-manager installation:

```bash
cargo install logbrew-cli
npm install -g logbrew-cli@latest
brew install LogBrewCo/tap/logbrew
```

To run the CLI once without a global install, use the exact npm package name:

```bash
npx logbrew-cli@latest --version
```

The npm package has no install scripts, so it works when npm blocks install
scripts or runs with `--ignore-scripts`. npm installs the native binary from one
optional platform package: `@logbrew/cli-darwin-arm64`,
`@logbrew/cli-darwin-x64`, `@logbrew/cli-linux-arm64`, `@logbrew/cli-linux-x64`
(glibc), or `@logbrew/cli-win32-x64`. Do not install with `--omit=optional`.

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

GitHub Release assets published after CLI 0.1.79 have a signed
build-provenance attestation. To verify a downloaded asset before you run it,
use the [GitHub CLI](https://cli.github.com/):

```bash
gh attestation verify logbrew-cli-x86_64-unknown-linux-gnu.tar.xz --repo LogBrewCo/cli
```

After any update, compare `logbrew version --json` with the
[latest public release](https://github.com/LogBrewCo/cli/releases/latest).

Cargo installs and source builds require Rust 1.87 or newer. The npm,
Homebrew, shell, PowerShell, and MSI installers use native release artifacts and
do not require a local Rust toolchain.

For development from this public repository:

```bash
cargo install --git https://github.com/LogBrewCo/cli logbrew-cli
```

## Distribution

LogBrew CLI is a Rust native binary. Cargo builds a platform-native `logbrew`
executable for the selected target. The npm platform packages, Homebrew, shell,
PowerShell, and MSI installers ship those native release binaries.

Release publishing is handled by GitHub Actions:

- GitHub Releases: native archives for Linux x64/ARM64, macOS x64/ARM64, and
  Windows x64 on GitHub-hosted runners.
- Installers: shell, PowerShell, Homebrew formula, and Windows MSI.
- npm: `logbrew-cli`, a launcher with no install scripts, and five
  `@logbrew/cli-<os>-<cpu>` packages that each hold one native binary. The
  release packs them with `scripts/npm-packages.sh` and publishes the platform
  packages before `logbrew-cli`.
- Package managers: crates.io and npm via trusted publishing/OIDC, and Homebrew
  via the `LogBrewCo/homebrew-tap` formula repository.

The CLI package surface is intentionally separate from the language and
framework SDK packages in `LogBrewCo/sdk`. SDK packages such as JavaScript
framework integrations, Python framework middleware, Swift, .NET, Go, Java,
Kotlin, Ruby, Rust SDK crates, and Unity packages remain SDK-owned. This repo
only publishes the `logbrew` CLI binary and its install wrappers.

## Basic Usage

```bash
logbrew examples
logbrew status
logbrew login
logbrew login --provider gitlab
logbrew login --device
logbrew whoami
logbrew logout
logbrew projects repositories --json
logbrew projects repositories discover --provider github --repository <repository_id> --json
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
pair created by `logbrew login`. Interactive login uses a sign-in provider
your LogBrew server offers: GitHub by default, or another one through
`logbrew login --provider <name>`. When the server does not offer the one you
pick, login stops and names the ones it offers. Login receives the result on a
loopback-only callback and stores the pair under `~/.logbrew`. `logbrew
whoami` and `logbrew me` return the authenticated account identity.
Authenticated commands rotate local credentials once after an expired-token
response; environment tokens are never persisted or refreshed.

Where no browser can open, such as in a container, a CI job, or a remote shell,
`logbrew login` signs you in from another device: it prints one link and one
code, and waits while you open the link anywhere, check the code, and approve.
Run `logbrew login --device` to choose this yourself. `logbrew login --json`
does the same for agents, printing the link and code as one JSON line and the
result as a second line. Device sign-in stores the same pair as browser login.
CLI output never prints token material. `logbrew logout` attempts to revoke the stored refresh-backed server
session and always removes local credentials; `LOGBREW_TOKEN` must be unset
separately.

### Set up a project with one command

Run this from your project directory:

```bash
npx logbrew-cli@latest setup
```

Setup detects the framework, signs you in if necessary, and lets you select or
create a project. It installs the SDK with the project's package manager,
writes `LOGBREW_SERVER_API_KEY` to the framework's env file (`.env`, or
`.env.local` for Symfony) without replacing existing values, and adds the init
code where the framework expects it. Then it sends a test event with the
project's ingest key, LogBrew reads it back, and setup prints the command that
reads it again:

```bash
logbrew read logs --project <project_id> --trace <trace_id>
```

Setup shows each file change as a diff and asks before it applies them.
It changes only files in the project directory and adds the env file to
`.gitignore`. You can run it again at any time; it keeps what is already set up.

Coding agents and scripts use `logbrew setup --yes --json`. It applies the same
steps without questions and lists every step and file change. Add `--mcp` to
add the hosted MCP server to the project config of detected agent clients
(Claude Code, Cursor, and VS Code), `--project <project_id>` to use an existing
project, and `--sample-data` to load a removable sample trace, logs, actions,
and issue into a project that has no events yet. Sample data is never billed.

### Plan SDK installation without changing files

Without `--yes`, `logbrew setup --json` detects nearby manifests and returns a
stable, non-mutating installation plan. SwiftPM and XcodeGen
projects receive the released Swift package declaration. Python projects
receive a package-index plan for the core SDK or the detected Django, Flask, or
FastAPI integration, including the pip, uv, poetry, or pipenv command. Bundler
projects receive the released Ruby package plan, and Rails applications are
identified from `config/application.rb` without reading application source.
SvelteKit projects receive the public core, browser, and Svelte integration
packages with the detected npm, pnpm, yarn, or bun command. Detection requires
an exact `@sveltejs/kit` dependency in a standard `package.json` dependency map.
An Express server receives the Node and Express packages, and setup adds the
Express middleware where the app is created. React and Express dependencies
receive the released browser/React or Node/Express packages. Mixed projects
receive one deduplicated install command plus separate browser and server key
scopes and explicit service, environment, and release requirements on both
surfaces.

Python plans report the public package compatibility requirements separately.
Review them before running the command, especially when a library supports
older Python or framework versions. The CLI reads only bounded local metadata,
does not install a package, does not change a manifest, and does not require
authentication for this plan.

### Create a project without dashboard sign-in

After `logbrew login`, the same authenticated CLI session can create an
account-owned project and its first project-scoped ingest key. No additional
dashboard or browser sign-in is required.

For a connected source-control provider, list repository candidates and run
bounded component discovery only for the repository you select:

```bash
logbrew projects repositories --json
logbrew projects repositories discover \
  --provider github --repository <repository_id> --json
```

Discovery reports its separate read-only contents authorization state and
never grants permission silently. A complete response supplies one expiring
discovery ID and component IDs. Repeat `--component` to create the selected
services under one project:

```bash
logbrew projects create "Checkout Platform" \
  --provider github --repository <repository_id> \
  --discovery <discovery_id> --component <component_id> \
  --ingest-key-file "$HOME/.logbrew/checkout-platform.key" --json
```

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

The default kind is `sdk`; supported kinds are `sdk`, `browser`, `server`,
`cli`, `artifact`, and `read`. An artifact key is project-scoped and
write-only. It can upload release source maps and native debug files but
cannot read artifact metadata or submit telemetry. A read key is
project-scoped and read-only: set `LOGBREW_TOKEN` to it to read that project's
issues, traces, logs, and other telemetry, for example from a coding agent or
a CI job. It cannot submit telemetry, change issues, manage projects, or read
another project. The command sends one idempotent
account-authenticated request and
stores the one-time key in a new owner-only file before reporting success. It
never prints the key or its path and does not create a duplicate project. An
ambiguous retry reuses the same request and idempotency key. Use
`--abandon-retry` only to intentionally discard that pending attempt.

### Revoke a project key

```bash
logbrew projects keys revoke <project_id> <key_id> --yes --json
```

The key stops working at its next request. Use the key ID from
`logbrew projects keys list <project_id>` or from the JSON output of
`logbrew projects keys create`.

### List a project's keys

```bash
logbrew projects keys list <project_id> --json
```

Shows each key's ID, label, kind, token prefix, status, and creation, expiry,
and revocation times. It never shows a full key.

### Rename a project

```bash
logbrew projects rename <project_id> --name "Checkout Web" --json
```

Changes the display name of an active project the account owns. The service
name that telemetry reports stays the same. A name has 1 to 120 characters.

### Archive an inactive project safely

Archive an account-owned project only after reviewing its UUID:

```bash
logbrew projects archive <project_id> --yes --json
```

The command requires explicit `--yes`, sends one account-authenticated archive
request, and reports success only after an empty `204` response. The project
then leaves the active catalog and its project-scoped ingest keys no longer
authorize new ingestion. This is a soft archive, not a deletion.

### Restore an archived project

```bash
logbrew projects archived --json
logbrew projects restore <project_id> --json
```

`archived` lists the newest 100 archived projects with their IDs, names, and
archive times. `restore` returns one to the active project list. A project
scheduled for permanent deletion cannot be restored.

### Upgrade the plan or manage billing

Payment stays with the account owner. These commands return one short-lived
Stripe link to open in a browser:

```bash
logbrew billing upgrade pro --json
logbrew billing upgrade business --json
logbrew billing portal --json
```

`upgrade` opens checkout for the chosen plan. `portal` opens billing
management for an account that already has a paid plan: change the plan, the
payment method, or download invoices. Run `logbrew usage` to see the plan
after paying.

### Review and revoke connected MCP clients

List the AI clients that you connected to the LogBrew MCP server:

```bash
logbrew mcp grants --json
```

Each grant shows its client name (or the client ID when the grant has no
name), the projects that you approved, when it was last used, and when it
expires. A connected client refreshes its access about once
an hour while you use it. A grant ends after 30 days without use, and after 90
days in all. To disconnect a client:

```bash
logbrew mcp revoke <grant_id> --yes
```

The client loses access at its next request. To connect again, it must sign in
again.

Coding agents use fewer tokens when they read on request instead of streaming.
`logbrew watch --json` opens a live WebSocket stream for
the current session, and `logbrew watch --severity error,critical --json`
filters live logs/issues client-side to actionable severities. The watch stream
reconnects after transient disconnects with a fresh feed ticket and backoff.

## GitHub Action

This repository is also a GitHub Action. In a deploy workflow, it uploads
JavaScript source maps and native debug files, then records the deploy, so
LogBrew can map stack frames and compare the release with the one before it.

Create an artifact key for the project and save it as the repository secret
`LOGBREW_ARTIFACT_KEY`:

```bash
logbrew projects keys create <project_id> --kind artifact --label "GitHub deploys" \
  --ingest-key-file ~/.logbrew/github-deploys.artifact
```

Add a step after the deploy step. Pin the action to a release tag; the tag
also selects the CLI version it installs:

```yaml
- uses: LogBrewCo/cli@v<version>
  if: always()
  with:
    token: ${{ secrets.LOGBREW_ARTIFACT_KEY }}
    project: <project_id>
    release: checkout@${{ github.sha }}
    environment: production
    service: checkout-web
    sourcemaps: dist
    debug-files: build/App.dSYM
    status: ${{ job.status == 'success' && 'succeeded' || 'failed' }}
```

| Input | Default | Meaning |
|---|---|---|
| `token` | required | Artifact key for the project |
| `project`, `release`, `environment`, `service` | required | The same values your app sends at runtime |
| `sourcemaps` | empty | Build directory with the manifest from the LogBrew Vite plugin; `@logbrew/sdk` must be installed |
| `sourcemaps-manifest` | `<sourcemaps>/logbrew-release-artifacts.json` | Manifest path |
| `debug-files` | empty | dSYM, ZIP, Mach-O, or Android ELF path |
| `deploy` | `true` | Record a deploy marker |
| `deployment-id` | `github-<run_id>-<run_attempt>` | Deploy ID; the same ID with the same values is safe to send again |
| `status` | `succeeded` | `succeeded` or `failed` |
| `started-at` | when the step started | RFC 3339 start time of the deploy |
| `commit-sha` | `github.sha` | Commit of the deployed build |
| `cli-version` | the tag version, or `latest` | CLI version to install from npm |
| `working-directory` | `.` | Directory to run in |

The action runs on Linux, macOS, and Windows runners with Node.js. It sets the
`deployment-id` output. An artifact key can upload artifacts and record deploys
for its project only. It cannot read data or send telemetry.

## Development

```bash
bash scripts/pre-commit.sh
```
