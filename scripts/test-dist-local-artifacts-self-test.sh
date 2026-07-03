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
package_fixture="$tmp_dir/package"
package_dir="$package_fixture/logbrew-cli-${host_target}"
archive="logbrew-cli-${host_target}.tar.xz"
checksum="${archive}.sha256"

write_logbrew_stub() {
  local human_version="$1"
  local json_version="$2"

  cat >"$package_dir/logbrew" <<STUB
#!/usr/bin/env bash
set -euo pipefail

if [[ "\${1:-}" == "--version" ]]; then
  printf 'logbrew %s\\n' "${human_version}"
  exit 0
fi

if [[ "\${1:-}" == "--json" && "\${2:-}" == "version" ]]; then
  printf '{"arch":"test","binary":"native","name":"logbrew","ok":true,"os":"test","version":"%s"}\\n' "${json_version}"
  exit 0
fi

printf 'unexpected logbrew args: %s\\n' "\$*" >&2
exit 1
STUB
  chmod +x "$package_dir/logbrew"
}

pack_archive() {
  (cd "$package_fixture" && tar -cJf "$artifact_fixture/$archive" "logbrew-cli-${host_target}")
}

make_fixture() {
  local human_version="${1:-$crate_version}"
  local json_version="${2:-$human_version}"

  rm -rf "$artifact_fixture" "$package_fixture"
  mkdir -p "$artifact_fixture" "$package_dir"
  write_logbrew_stub "$human_version" "$json_version"
  printf 'readme\n' >"$package_dir/README.md"
  printf 'license\n' >"$package_dir/LICENSE"
  pack_archive
  printf 'abc123 *%s\n' "$archive" >"$artifact_fixture/$checksum"
}

run_artifact_check() {
  LOGBREW_DIST_LOCAL_ARTIFACTS_DIR="$artifact_fixture" \
    LOGBREW_DIST_LOCAL_TARGET="$host_target" \
    bash scripts/test-dist-local-artifacts.sh "v${crate_version}" >"$output_file" 2>&1
}

expect_failure() {
  local expected_line="$1"

  : >"$output_file"
  if run_artifact_check; then
    printf 'expected dist local artifact check to fail\n' >&2
    exit 1
  fi

  if ! grep -Fq "$expected_line" "$output_file"; then
    printf 'expected dist local artifact output to contain: %s\n' "$expected_line" >&2
    printf 'actual output:\n' >&2
    cat "$output_file" >&2
    exit 1
  fi
}

make_fixture
: >"$output_file"
if ! run_artifact_check; then
  printf 'expected dist local artifact fixture to pass\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

if ! grep -Fq "Dist local artifacts check passed for ${host_target}." "$output_file"; then
  printf 'expected dist local artifact success output\n' >&2
  printf 'actual output:\n' >&2
  cat "$output_file" >&2
  exit 1
fi

make_fixture
rm "$artifact_fixture/$archive"
expect_failure "Dist local artifacts check failed: missing generated artifact ${archive}"

make_fixture
printf 'abc123 *wrong-artifact.tar.xz\n' >"$artifact_fixture/$checksum"
expect_failure "Dist local artifacts check failed: ${checksum} must contain *${archive}"

make_fixture
rm "$package_dir/logbrew"
pack_archive
expect_failure 'Dist local artifacts check failed: archive must contain logbrew binary'

make_fixture "0.0.0"
expect_failure "Dist local artifacts check failed: archive logbrew binary must report version ${crate_version}"

make_fixture "$crate_version" "0.0.0"
expect_failure "Dist local artifacts check failed: archive logbrew version JSON must expose native binary metadata for ${crate_version}"

printf 'Dist local artifacts self-test passed.\n'
