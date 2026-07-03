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
native_dir="$native_fixture/logbrew-cli-${host_target}"
archive="logbrew-cli-${host_target}.tar.xz"

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

write_installer_stub() {
  cat >"$artifact_fixture/logbrew-cli-installer.sh" <<STUB
#!/usr/bin/env bash
set -euo pipefail

archive="${archive}"
download_url="\${LOGBREW_CLI_DOWNLOAD_URL:-}"
install_root="\${LOGBREW_CLI_INSTALL_DIR:-\${CARGO_DIST_FORCE_INSTALL_DIR:-}}"

if [[ -z "\$download_url" ]]; then
  printf 'missing LOGBREW_CLI_DOWNLOAD_URL\\n' >&2
  exit 1
fi

if [[ -z "\$install_root" ]]; then
  printf 'missing install dir override\\n' >&2
  exit 1
fi

if [[ "\${LOGBREW_CLI_NO_MODIFY_PATH:-}" != "1" ]]; then
  printf 'missing LOGBREW_CLI_NO_MODIFY_PATH=1\\n' >&2
  exit 1
fi

if [[ "\${LOGBREW_CLI_DISABLE_UPDATE:-}" != "1" ]]; then
  printf 'missing LOGBREW_CLI_DISABLE_UPDATE=1\\n' >&2
  exit 1
fi

tmp_dir="\$(mktemp -d)"
trap 'rm -rf "\$tmp_dir"' EXIT
mkdir -p "\$tmp_dir/extract" "\$install_root/bin"
curl -sSfL "\${download_url}/\${archive}" -o "\$tmp_dir/\${archive}"
tar xf "\$tmp_dir/\${archive}" --strip-components 1 -C "\$tmp_dir/extract"
cp "\$tmp_dir/extract/logbrew" "\$install_root/bin/logbrew"
chmod +x "\$install_root/bin/logbrew"
STUB
}

make_fixture() {
  local binary_version="${1:-$crate_version}"

  rm -rf "$artifact_fixture" "$native_fixture"
  mkdir -p "$artifact_fixture" "$native_dir"
  write_logbrew_stub "$binary_version"
  printf 'readme\n' >"$native_dir/README.md"
  printf 'license\n' >"$native_dir/LICENSE"
  (cd "$native_fixture" && tar -cJf "$artifact_fixture/$archive" "logbrew-cli-${host_target}")
  write_installer_stub
}

run_smoke() {
  LOGBREW_DIST_SHELL_ARTIFACTS_DIR="$artifact_fixture" \
    LOGBREW_DIST_SHELL_TARGET="$host_target" \
    bash scripts/test-dist-shell-installer-smoke.sh "v${crate_version}" >"$output_file" 2>&1
}

expect_failure() {
  local expected_line="$1"

  : >"$output_file"
  if run_smoke; then
    printf 'expected dist shell installer smoke to fail\n' >&2
    exit 1
  fi

  if ! grep -Fq "$expected_line" "$output_file"; then
    printf 'expected dist shell installer output to contain: %s\n' "$expected_line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

make_fixture
: >"$output_file"
if ! run_smoke; then
  printf 'expected dist shell installer fixture to pass\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Dist shell installer smoke passed for ${host_target}." "$output_file"; then
  printf 'expected dist shell installer success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
rm "$artifact_fixture/logbrew-cli-installer.sh"
expect_failure "Dist shell installer smoke failed: missing generated artifact logbrew-cli-installer.sh"

make_fixture
rm "$artifact_fixture/$archive"
expect_failure "Dist shell installer smoke failed: missing generated artifact ${archive}"

make_fixture "0.0.0"
expect_failure "Dist shell installer smoke failed: shell-installed logbrew must report version ${crate_version}"

printf 'Dist shell installer smoke self-test passed.\n'
