#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TAG="${1:-}"
tmp_dir="$(mktemp -d)"
server_pid=""

cleanup() {
  if [[ -n "$server_pid" ]]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

fail() {
  printf 'Dist npm package install smoke failed: %s\n' "$1" >&2
  printf 'Next: fix cargo-dist npm package installability, then rerun bash scripts/test-dist-npm-package-install-smoke.sh %s.\n' "${TAG:-v<version>}" >&2
  exit 1
}

fail_missing_command() {
  local command_name="$1"

  printf "Dist npm package install smoke failed: missing required command '%s'\n" "$command_name" >&2
  case "$command_name" in
    dist)
      printf 'Next: install cargo-dist with:\n' >&2
      printf "  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v%s/cargo-dist-installer.sh | sh\n" "$dist_version" >&2
      printf 'Then rerun bash scripts/test-dist-npm-package-install-smoke.sh %s.\n' "${TAG:-v<version>}" >&2
      ;;
    *)
      printf "Next: install '%s' so it is on PATH, then rerun bash scripts/test-dist-npm-package-install-smoke.sh %s.\n" "$command_name" "${TAG:-v<version>}" >&2
      ;;
  esac
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail_missing_command "$1"
  fi
}

require_file() {
  local file="$1"
  local name="$2"

  if [[ ! -s "$file" ]]; then
    fail "missing generated artifact ${name}"
  fi
}

dist_version="$(
  sed -n 's/^cargo-dist-version = "\(.*\)"/\1/p' dist-workspace.toml
)"

if [[ -z "$dist_version" ]]; then
  fail "could not read cargo-dist version from dist-workspace.toml"
fi

require_command cargo
require_command curl
require_command jq
require_command node
require_command npm
require_command python3
require_command rustc
require_command tar

crate_version="$(
  cargo metadata --no-deps --format-version=1 |
    jq -r '.packages[] | select(.name == "logbrew-cli").version'
)"

if [[ -z "$crate_version" || "$crate_version" == "null" ]]; then
  fail "could not read logbrew-cli version from Cargo metadata"
fi

if [[ -z "$TAG" ]]; then
  TAG="v${crate_version}"
fi

tag_version="${TAG#v}"
if [[ "$tag_version" != "$crate_version" ]]; then
  fail "tag ${TAG} does not match Cargo.toml version ${crate_version}"
fi

target="${LOGBREW_DIST_NPM_TARGET:-$(rustc -vV | awk '/^host:/ {print $2}')}"
if [[ -z "$target" ]]; then
  fail "could not read rustc host target"
fi

if ! grep -Fq "\"${target}\"" dist-workspace.toml; then
  fail "target ${target} is not in the cargo-dist target matrix"
fi

case "$target" in
  *windows*)
    archive="logbrew-cli-${target}.zip"
    ;;
  *)
    archive="logbrew-cli-${target}.tar.xz"
    ;;
esac
npm_package="logbrew-cli-npm-package.tar.gz"

if [[ -n "${LOGBREW_DIST_NPM_ARTIFACTS_DIR:-}" ]]; then
  artifact_dir="$LOGBREW_DIST_NPM_ARTIFACTS_DIR"
else
  require_command dist
  artifact_dir="$ROOT_DIR/target/distrib"
  rm -rf "$artifact_dir"
  if ! dist build --tag "$TAG" --artifacts=global --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist global artifacts"
  fi
  if ! dist build --tag "$TAG" --artifacts=local --target "$target" --output-format=json --no-local-paths >/dev/null; then
    fail "could not build cargo-dist local artifact for ${target}"
  fi
fi

if [[ ! -d "$artifact_dir" ]]; then
  fail "generated artifact directory does not exist"
fi

require_file "$artifact_dir/$npm_package" "$npm_package"
require_file "$artifact_dir/$archive" "$archive"

port="$(
  python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"

if [[ -z "$port" ]]; then
  fail "could not choose local artifact server port"
fi

python3 -m http.server "$port" --bind 127.0.0.1 --directory "$artifact_dir" >"$tmp_dir/http.log" 2>&1 &
server_pid="$!"
artifact_base_url="http://127.0.0.1:${port}"
archive_url="${artifact_base_url}/${archive}"

server_ready=0
for _ in {1..30}; do
  if curl --silent --fail --head "$archive_url" >/dev/null; then
    server_ready=1
    break
  fi
  if ! kill -0 "$server_pid" >/dev/null 2>&1; then
    sed -n '1,120p' "$tmp_dir/http.log" >&2 || true
    fail "local artifact server stopped before serving artifacts"
  fi
  sleep 0.2
done

if [[ "$server_ready" != "1" ]]; then
  sed -n '1,120p' "$tmp_dir/http.log" >&2 || true
  fail "could not start local artifact server"
fi

package_work="$tmp_dir/package-work"
mkdir -p "$package_work"
if ! tar -xzf "$artifact_dir/$npm_package" -C "$package_work"; then
  fail "could not extract ${npm_package}"
fi

npm_package_json="$package_work/package/package.json"
require_file "$npm_package_json" "npm package package.json"

if ! jq --arg url "$artifact_base_url" '.artifactDownloadUrls = [$url]' "$npm_package_json" >"$npm_package_json.tmp"; then
  fail "could not rewrite npm package artifactDownloadUrls"
fi
mv "$npm_package_json.tmp" "$npm_package_json"

local_npm_package="$tmp_dir/$npm_package"
(cd "$package_work" && tar -czf "$local_npm_package" package)

npm_project="$tmp_dir/npm-project"
mkdir -p "$npm_project"
printf '{"private":true}\n' >"$npm_project/package.json"

if ! (cd "$npm_project" && npm install --no-audit --no-fund --loglevel=error "$local_npm_package" >"$tmp_dir/npm-install.log" 2>&1); then
  sed -n '1,160p' "$tmp_dir/npm-install.log" >&2 || true
  fail "npm install from generated package tarball failed"
fi

binary="$npm_project/node_modules/.bin/logbrew"
if [[ ! -x "$binary" ]]; then
  fail "npm install did not create executable logbrew bin link"
fi

if ! human_output="$("$binary" --version)"; then
  fail "npm-installed logbrew must support --version"
fi

if [[ "$human_output" != "logbrew ${crate_version}" ]]; then
  fail "npm-installed logbrew must report version ${crate_version}"
fi

if ! json_output="$("$binary" --json version)"; then
  fail "npm-installed logbrew must support version --json"
fi

if ! jq -e --arg version "$crate_version" '
  .ok == true and
  .name == "logbrew" and
  .version == $version and
  (.binary | type == "string" and length > 0) and
  (.os | type == "string" and length > 0) and
  (.arch | type == "string" and length > 0)
' <<<"$json_output" >/dev/null; then
  fail "npm-installed logbrew version JSON must expose native binary metadata for ${crate_version}"
fi

printf 'Dist npm package install smoke passed for %s.\n' "$target"
