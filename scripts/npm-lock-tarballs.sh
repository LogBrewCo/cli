#!/usr/bin/env bash
# Writes package.json and package-lock.json for an npm project in the current
# directory that installs the given package tarballs. Each tarball is copied
# into the project and locked by the sha512 of its bytes, so `npm ci` installs
# exactly the packages under test and refuses a tarball with other contents.
# The project takes nothing from a registry. Usage:
#   bash scripts/npm-lock-tarballs.sh TARBALL...
set -euo pipefail

[[ $# -ge 1 ]] || { printf 'usage: bash scripts/npm-lock-tarballs.sh TARBALL...\n' >&2; exit 1; }
for tarball in "$@"; do
  [[ -f "$tarball" ]] || { printf 'missing tarball: %s\n' "$tarball" >&2; exit 1; }
  cp "$tarball" .
done

node - "$@" <<'JS'
const { execFileSync } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const lockedFields = ["license", "dependencies", "optionalDependencies", "bin", "engines", "os", "cpu", "libc"];
const root = { name: "npm-install-check", version: "1.0.0", private: true, dependencies: {} };
const packages = { "": { name: root.name, version: root.version, dependencies: root.dependencies } };
for (const tarball of process.argv.slice(2)) {
  const file = path.basename(tarball);
  const manifest = JSON.parse(execFileSync("tar", ["-xzOf", file, "package/package.json"], { encoding: "utf8" }));
  const entry = {
    version: manifest.version,
    resolved: `file:${file}`,
    integrity: `sha512-${crypto.createHash("sha512").update(fs.readFileSync(file)).digest("base64")}`,
  };
  for (const field of lockedFields) {
    if (manifest[field] !== undefined) entry[field] = manifest[field];
  }
  root.dependencies[manifest.name] = `file:${file}`;
  packages[`node_modules/${manifest.name}`] = entry;
}
// A platform package that is not here stays out of the install: npm skips an
// optional dependency whose os or cpu does not match this machine.
for (const [key, entry] of Object.entries(packages)) {
  for (const [name, version] of Object.entries(entry.optionalDependencies ?? {})) {
    const match = /^@logbrew\/cli-(darwin|linux|win32)-(arm64|x64)$/.exec(name);
    if (key === "" || packages[`node_modules/${name}`] || !match) continue;
    packages[`node_modules/${name}`] = { version, optional: true, os: [match[1]], cpu: [match[2]] };
  }
}
const lock = { name: root.name, version: root.version, lockfileVersion: 3, requires: true, packages };
fs.writeFileSync("package.json", `${JSON.stringify(root, null, 2)}\n`);
fs.writeFileSync("package-lock.json", `${JSON.stringify(lock, null, 2)}\n`);
JS
