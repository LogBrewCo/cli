#!/usr/bin/env bash
set -euo pipefail

[[ "$RELEASE_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || exit 1

for _ in {1..40}; do
  release="$(gh release view "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" --json targetCommitish,isDraft,isPrerelease 2>/dev/null || true)"
  [[ -n "$release" ]] && break
  sleep 1
done
target="$(jq -r 'select(.isDraft == false) | .targetCommitish' <<<"$release")"
[[ "$target" =~ ^[0-9a-f]{40}$ ]] || exit 1

if [[ "$GITHUB_EVENT_NAME" == "workflow_dispatch" ]]; then
  [[ "$(jq -r '.isPrerelease' <<<"$release")" == "false" ]] || exit 1
  run="$(gh run view "$ARTIFACTS_RUN_ID" --repo "$GITHUB_REPOSITORY" --json name,event,headBranch,headSha,conclusion)"
  jq -e --arg target "$target" '.name == "Release" and .event == "workflow_dispatch" and .headBranch == "main" and .headSha == $target and .conclusion == "success"' <<<"$run" >/dev/null || exit 1
else
  [[ "$GITHUB_EVENT_NAME" == "push" && "$target" == "$GITHUB_SHA" ]] || exit 1
fi
printf 'EXPECTED_VERSION=%s\n' "${RELEASE_TAG#v}" >> "$GITHUB_ENV"
