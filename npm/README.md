# logbrew-cli

The official [LogBrew](https://logbrew.co) command-line interface. The npm
package is `logbrew-cli`, and it installs the `logbrew` command.

Read logs, issues, traces, and releases from your terminal. Use `--json` for
stable output to scripts and AI coding agents.

## Install

```bash
npm install -g logbrew-cli
logbrew --version
```

Run it once without a global install:

```bash
npx logbrew-cli@latest --version
```

The package has no install scripts. npm installs one native binary for your
platform from an `@logbrew/cli-<platform>` package: macOS arm64 and x64, Linux
arm64 and x64 (glibc), and Windows x64. Do not use `--omit=optional`, because
it skips the binary.

## Get started

```bash
logbrew examples
logbrew login
logbrew status
```

Documentation: <https://github.com/LogBrewCo/cli#readme>
