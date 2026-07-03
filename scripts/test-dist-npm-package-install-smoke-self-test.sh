#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp_dir="$(mktemp -d)"
output_file="$(mktemp)"
trap 'rm -rf "$tmp_dir" "$output_file"' EXIT

crate_version="$(
  cargo metadata --no-deps --format-version=1 |
    jq -r '.packages[] | select(.name == "logbrew-cli").version'
)"

if [[ -z "$crate_version" || "$crate_version" == "null" ]]; then
  printf 'could not read logbrew-cli version from Cargo metadata\n' >&2
  exit 1
fi

host_target="$(rustc -vV | awk '/^host:/ {print $2}')"
if [[ -z "$host_target" ]]; then
  printf 'could not read rustc host target\n' >&2
  exit 1
fi

artifact_fixture="$tmp_dir/artifacts"
native_fixture="$tmp_dir/native"
npm_fixture="$tmp_dir/npm"
native_dir="$native_fixture/logbrew-cli-${host_target}"
archive="logbrew-cli-${host_target}.tar.xz"
npm_package="logbrew-cli-npm-package.tar.gz"

write_logbrew_stub() {
  local version="$1"

  cat >"$native_dir/logbrew" <<STUB
#!/usr/bin/env bash
set -euo pipefail

if [[ "\${1:-}" == "--version" || "\${1:-}" == "version" ]]; then
  printf 'logbrew %s\\n' "${version}"
  exit 0
fi

if [[ "\${1:-}" == "--json" && "\${2:-}" == "version" ]]; then
  printf '{"arch":"test","binary":"native","name":"logbrew","ok":true,"os":"test","version":"%s"}\\n' "${version}"
  exit 0
fi

printf 'unexpected logbrew args: %s\\n' "\$*" >&2
exit 1
STUB
  chmod +x "$native_dir/logbrew"
}

write_npm_package_fixture() {
  mkdir -p "$npm_fixture/package"

  cat >"$npm_fixture/package/package.json" <<JSON
{
  "name": "logbrew-cli",
  "version": "${crate_version}",
  "bin": {
    "logbrew": "run-logbrew.js"
  },
  "scripts": {
    "postinstall": "node ./install.js"
  },
  "artifactDownloadUrls": [
    "http://example.invalid"
  ],
  "supportedPlatforms": {
    "${host_target}": {
      "artifactName": "${archive}",
      "bins": {
        "logbrew": "logbrew"
      },
      "zipExt": ".tar.xz"
    }
  }
}
JSON

  cat >"$npm_fixture/package/install.js" <<'NODE'
#!/usr/bin/env node

const fs = require("fs");
const http = require("http");
const https = require("https");
const path = require("path");
const { spawnSync } = require("child_process");

const pkg = require("./package.json");
const platform = Object.values(pkg.supportedPlatforms)[0];
const baseUrl = pkg.artifactDownloadUrls[0];
const artifactUrl = `${baseUrl}/${platform.artifactName}`;
const installDir = path.join(__dirname, "node_modules", ".bin_real");
const tempFile = path.join(__dirname, platform.artifactName);

fs.rmSync(installDir, { recursive: true, force: true });
fs.mkdirSync(installDir, { recursive: true });

const client = artifactUrl.startsWith("https:") ? https : http;
const request = client.get(artifactUrl, (response) => {
  if (response.statusCode !== 200) {
    console.error(`download failed with HTTP ${response.statusCode}`);
    process.exit(1);
  }

  const file = fs.createWriteStream(tempFile);
  response.pipe(file);
  file.on("finish", () => {
    file.close();
    const result = spawnSync(
      "tar",
      ["xf", tempFile, "--strip-components", "1", "-C", installDir],
      { stdio: "inherit" },
    );
    process.exit(result.status === null ? 1 : result.status);
  });
});

request.on("error", (error) => {
  console.error(error.message);
  process.exit(1);
});
NODE

  cat >"$npm_fixture/package/run-logbrew.js" <<'NODE'
#!/usr/bin/env node

const path = require("path");
const { spawnSync } = require("child_process");

const binary = path.join(__dirname, "node_modules", ".bin_real", "logbrew");
const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status === null ? 1 : result.status);
NODE

  chmod +x "$npm_fixture/package/install.js" "$npm_fixture/package/run-logbrew.js"
  (cd "$npm_fixture" && tar -czf "$artifact_fixture/$npm_package" package)
}

make_fixture() {
  local binary_version="${1:-$crate_version}"

  rm -rf "$artifact_fixture" "$native_fixture" "$npm_fixture"
  mkdir -p "$artifact_fixture" "$native_dir"
  write_logbrew_stub "$binary_version"
  printf 'readme\n' >"$native_dir/README.md"
  printf 'license\n' >"$native_dir/LICENSE"
  (cd "$native_fixture" && tar -cJf "$artifact_fixture/$archive" "logbrew-cli-${host_target}")
  write_npm_package_fixture
}

run_smoke() {
  LOGBREW_DIST_NPM_ARTIFACTS_DIR="$artifact_fixture" \
    LOGBREW_DIST_NPM_TARGET="$host_target" \
    bash scripts/test-dist-npm-package-install-smoke.sh "v${crate_version}" >"$output_file" 2>&1
}

expect_failure() {
  local expected_line="$1"

  : >"$output_file"
  if run_smoke; then
    printf 'expected dist npm package install smoke to fail\n' >&2
    exit 1
  fi

  if ! grep -Fq "$expected_line" "$output_file"; then
    printf 'expected dist npm package install output to contain: %s\n' "$expected_line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

make_fixture
: >"$output_file"
if ! run_smoke; then
  printf 'expected dist npm package install fixture to pass\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Dist npm package install smoke passed for ${host_target}." "$output_file"; then
  printf 'expected dist npm package install success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
rm "$artifact_fixture/$npm_package"
expect_failure "Dist npm package install smoke failed: missing generated artifact ${npm_package}"

make_fixture
rm "$artifact_fixture/$archive"
expect_failure "Dist npm package install smoke failed: missing generated artifact ${archive}"

make_fixture "0.0.0"
expect_failure "Dist npm package install smoke failed: npm-installed logbrew must report version ${crate_version}"

printf 'Dist npm package install smoke self-test passed.\n'
