#!/usr/bin/env bash
# Find a completed full CI run for one exact commit SHA.

set -euo pipefail

sha=${1:?usage: find-successful-ci.sh COMMIT_SHA [BRANCH]}
branch=${2:-}
repo=${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}

command -v gh >/dev/null || {
  echo "gh is required" >&2
  exit 2
}
command -v jq >/dev/null || {
  echo "jq is required" >&2
  exit 2
}

runs=$(gh api --method GET \
  "repos/$repo/actions/workflows/ci.yml/runs" \
  -f head_sha="$sha" \
  -f status=success \
  -F per_page=100)

required_jobs=(
  "Supply chain"
  "Build (Linux x64)"
  "Native (macOS arm64)"
  "Native (Windows x64)"
  "Rust MSRV (1.88.0)"
  "Coverage"
)

while IFS=$'\t' read -r run_id run_branch run_event run_url; do
  if [[ "$run_event" != "push" ]]; then
    continue
  fi
  if [[ -n "$branch" && "$run_branch" != "$branch" ]]; then
    continue
  fi

  jobs=$(gh api --method GET \
    "repos/$repo/actions/runs/$run_id/jobs" \
    -F per_page=100)
  complete=true
  for name in "${required_jobs[@]}"; do
    if ! jq -e --arg name "$name" \
      '.jobs | any(.name == $name and .conclusion == "success")' \
      >/dev/null <<<"$jobs"; then
      complete=false
      break
    fi
  done

  if [[ "$complete" == true ]]; then
    printf '%s\n' "$run_url"
    exit 0
  fi
done < <(
  jq -r '.workflow_runs[] | [.id, .head_branch, .event, .html_url] | @tsv' \
    <<<"$runs"
)

echo "no complete successful CI run found for $sha${branch:+ on $branch}" >&2
exit 1
