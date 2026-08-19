#!/usr/bin/env bash
# Ensure the Linux C toolchain and zlib development interface used by libmandoc.

set -euo pipefail

probe_dir=$(mktemp -d)
trap 'rm -rf -- "$probe_dir"' EXIT

native_dependencies_available() {
  command -v cc >/dev/null 2>&1 &&
    command -v ar >/dev/null 2>&1 &&
    printf '%s\n' \
      '#include <zlib.h>' \
      'int main(void) { return zlibVersion() == 0; }' |
      cc -x c - -o "$probe_dir/zlib-probe" -lz >/dev/null 2>&1
}

if native_dependencies_available; then
  echo "Linux native build dependencies are already available"
  exit 0
fi

if ! command -v apt-get >/dev/null 2>&1 || ! command -v sudo >/dev/null 2>&1; then
  echo "Linux native build dependencies are missing and apt-get is unavailable" >&2
  exit 1
fi

# Hosted runner mirrors occasionally accept a connection and then stop
# transferring package indexes. Bound both APT's individual requests and the
# whole command so a transient mirror failure cannot consume the job timeout.
apt_get=(
  sudo env DEBIAN_FRONTEND=noninteractive apt-get
  -o Acquire::Retries=2
  -o Acquire::http::Timeout=15
  -o Acquire::https::Timeout=15
  -o Dpkg::Use-Pty=0
)

timeout --signal=TERM --kill-after=10s 90s "${apt_get[@]}" update
timeout --signal=TERM --kill-after=10s 90s "${apt_get[@]}" install \
  --yes \
  --no-install-recommends \
  build-essential \
  zlib1g-dev

if ! native_dependencies_available; then
  echo "installed packages did not provide a working C toolchain and zlib" >&2
  exit 1
fi
