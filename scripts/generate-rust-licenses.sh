#!/usr/bin/env bash
# Regenerate the distributable notice for Rust dependencies.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

output=${1:-$ROOT/THIRD_PARTY_LICENSES.html}
license_tmp=$(mktemp)
trap 'rm -f "$license_tmp"' EXIT

command -v cargo-about >/dev/null 2>&1 || {
  printf 'cargo-about is required; install version 0.9.1\n' >&2
  exit 1
}

about_version=$(cargo about --version)
[[ $about_version == "cargo-about 0.9.1" ]] || {
  printf 'cargo-about 0.9.1 is required, found: %s\n' "$about_version" >&2
  exit 1
}

cargo about generate \
  --frozen \
  --all-features \
  --fail \
  --manifest-path crates/mant/Cargo.toml \
  --output-file "$license_tmp" \
  licenses/about.hbs

# Upstream license files may use CRLF or carry insignificant trailing spaces.
# Normalize the generated report so identical dependency metadata produces a
# clean, byte-stable repository artifact on every host.
sed -e 's/\r$//' -e 's/[[:blank:]]*$//' "$license_tmp" > "$output"
