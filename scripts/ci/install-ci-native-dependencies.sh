#!/usr/bin/env bash
# Ensure native builds and the maintainer-only pinned CVS replay can run in CI.

set -euo pipefail

probe_dir=$(mktemp -d)
trap 'rm -rf -- "$probe_dir"' EXIT

native_dependencies_available() {
  command -v cc >/dev/null 2>&1 &&
    command -v ar >/dev/null 2>&1 &&
    command -v cvs >/dev/null 2>&1 &&
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

install_native_packages() {
  timeout --signal=TERM --kill-after=10s 90s "${apt_get[@]}" "$@" update &&
    timeout --signal=TERM --kill-after=10s 90s "${apt_get[@]}" "$@" install \
      --yes --no-install-recommends build-essential cvs zlib1g-dev
}

if ! install_native_packages; then
  # A mirror-list source can keep selecting a stalled Azure HTTP endpoint even
  # after APT retries. Scope the fallback to these commands: do not rewrite the
  # runner's /etc sources, and retain Ubuntu archive signature verification.
  native_id=$(awk -F= '$1 == "ID" { gsub(/"/, "", $2); print $2 }' /etc/os-release)
  native_codename=$(awk -F= '$1 == "VERSION_CODENAME" { gsub(/"/, "", $2); print $2 }' /etc/os-release)
  if [[ $native_id != ubuntu || ! $native_codename =~ ^[a-z][a-z0-9]*$ ]]; then
    echo "native dependency installation failed; no safe Ubuntu mirror fallback is available" >&2
    exit 1
  fi
  case $(dpkg --print-architecture) in
    amd64|i386)
      native_archive=https://archive.ubuntu.com/ubuntu
      native_security=https://security.ubuntu.com/ubuntu
      ;;
    arm64|armhf)
      native_archive=https://ports.ubuntu.com/ubuntu-ports
      native_security=$native_archive
      ;;
    *) echo "no checked Ubuntu archive fallback for this architecture" >&2; exit 1 ;;
  esac
  native_sources="$probe_dir/ubuntu.list"
  for native_suite in "$native_codename" "$native_codename-updates"; do
    printf 'deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] %s %s main universe\n' \
      "$native_archive" "$native_suite"
  done > "$native_sources"
  printf 'deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] %s %s-security main universe\n' \
    "$native_security" "$native_codename" >> "$native_sources"
  echo "retrying native dependencies with Ubuntu official HTTPS archives" >&2
  install_native_packages \
    -o "Dir::Etc::sourcelist=$native_sources" -o "Dir::Etc::sourceparts=-"
fi

if ! native_dependencies_available; then
  echo "installed packages did not provide a working C toolchain and zlib" >&2
  exit 1
fi
