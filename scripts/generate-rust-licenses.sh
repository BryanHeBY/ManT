#!/usr/bin/env bash
# Regenerate or verify the distributable notice for Rust dependencies.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

mode=write
if [[ ${1:-} == --check ]]; then
  mode=check
  shift
fi
if (( $# > 1 )); then
  printf 'usage: %s [--check] [OUTPUT]\n' "$0" >&2
  exit 2
fi

output=${1:-$ROOT/THIRD_PARTY_LICENSES.html}
license_tmp=$(mktemp)
normalized_tmp=$(mktemp)
trap 'rm -f "$license_tmp" "$normalized_tmp"' EXIT

command -v cargo-about >/dev/null 2>&1 || {
  printf 'cargo-about is required; install version 0.9.2\n' >&2
  exit 1
}

about_version=$(cargo about --version)
[[ $about_version == "cargo-about 0.9.2" ]] || {
  printf 'cargo-about 0.9.2 is required, found: %s\n' "$about_version" >&2
  exit 1
}

cargo about generate \
  --frozen \
  --all-features \
  --fail \
  --manifest-path crates/mant/Cargo.toml \
  --output-file "$license_tmp" \
  licenses/about.hbs

normalize_report() {
  sed -e 's/\r$//' -e 's/[[:blank:]]*$//' "$1" > "$2"
}

# Upstream license files may use CRLF or carry insignificant trailing spaces.
# Normalize the generated report so identical dependency metadata produces a
# clean, byte-stable repository artifact on every host.
if [[ $mode == check ]]; then
  normalize_report "$license_tmp" "$normalized_tmp"
  cmp "$output" "$normalized_tmp" || {
    printf '%s is stale; run scripts/generate-rust-licenses.sh\n' "$output" >&2
    exit 1
  }
else
  normalize_report "$license_tmp" "$output"
fi
