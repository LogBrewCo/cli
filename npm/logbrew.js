#!/usr/bin/env node
"use strict";

// Run the native binary from the platform package that npm installed through
// optionalDependencies. This package has no install scripts.
const { spawnSync } = require("node:child_process");
const path = require("node:path");

const platform = `${process.platform}-${process.arch}`;
let binary;
try {
  const manifest = require.resolve(`@logbrew/cli-${platform}/package.json`);
  binary = path.join(path.dirname(manifest), "bin", process.platform === "win32" ? "logbrew.exe" : "logbrew");
} catch {
  process.stderr.write(
    `logbrew: no native binary installed for ${platform}.\n` +
      "Next: reinstall without --omit=optional, or run cargo install logbrew-cli --locked\n",
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  process.stderr.write(`logbrew: could not start the ${platform} binary (${result.error.code ?? "unknown"}).\n`);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
