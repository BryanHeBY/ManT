#!/bin/sh
# Build the project-owned libmandoc sidecar.  This is a build-time download;
# the resulting binary has no runtime dependency on the system mandoc package.
set -eu

MANDOC_VERSION=1.14.6
MANDOC_SHA256=8bf0d570f01e70a6e124884088870cbed7537f36328d512909eb10cd53179d9c
MANDOC_URL="https://mandoc.bsd.lv/snapshots/mandoc-${MANDOC_VERSION}.tar.gz"

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUTPUT_DIR="$ROOT/native/bin"
OUTPUT="$OUTPUT_DIR/mant-mandoc-json"

# Keep direct `bun run build:mandoc-json` calls consistent with scripts/ci.ts.
HOST_SYSTEM=$(uname -s)
case "$HOST_SYSTEM" in
  Linux*) DEFAULT_CC=gcc ;;
  Darwin*) DEFAULT_CC=clang ;;
  MINGW*|MSYS*|CYGWIN*)
    echo "native Windows builds are not supported; use WSL" >&2
    exit 1
    ;;
  *)
    echo "native Mant builds support Linux and macOS only" >&2
    exit 1
    ;;
esac
CC_BIN=${CC:-$DEFAULT_CC}

if ! command -v "$CC_BIN" >/dev/null 2>&1; then
  echo "C compiler '$CC_BIN' was not found; install it or set CC explicitly" >&2
  exit 1
fi

WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/mant-mandoc.XXXXXX")
cleanup() {
  rm -rf "$WORKDIR"
}
trap cleanup EXIT HUP INT TERM

MANDOC_TARBALL="$WORKDIR/mandoc.tar.gz"

# Download with resume and retries; curl exit 33 means the server does not
# support resuming from the current offset, in which case we restart.
for attempt in 1 2 3; do
  echo "downloading mandoc ${MANDOC_VERSION} (attempt ${attempt})..."
  if curl --fail --location --silent --show-error \
          --retry 3 --retry-delay 2 \
          --connect-timeout 15 --max-time 120 \
          --continue-at - \
          "$MANDOC_URL" --output "$MANDOC_TARBALL"; then
    break
  else
    curl_exit=$?
  fi
  if [ "$attempt" -eq 3 ]; then
    echo "failed to download mandoc source (curl exit ${curl_exit})" >&2
    exit 1
  fi
  if [ "$curl_exit" -eq 33 ]; then
    echo "server rejected resume, restarting download..."
    rm -f "$MANDOC_TARBALL"
  fi
  sleep 2
done

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256=$(sha256sum "$MANDOC_TARBALL" | awk '{print $1}')
else
  ACTUAL_SHA256=$(shasum -a 256 "$MANDOC_TARBALL" | awk '{print $1}')
fi

if [ "$ACTUAL_SHA256" != "$MANDOC_SHA256" ]; then
  echo "mandoc source checksum mismatch" >&2
  exit 1
fi

tar -xzf "$WORKDIR/mandoc.tar.gz" -C "$WORKDIR"
SOURCE_DIR="$WORKDIR/mandoc-$MANDOC_VERSION"

(
  cd "$SOURCE_DIR"
  CC="$CC_BIN" ./configure
  make CC="$CC_BIN" libmandoc.a
)

mkdir -p "$OUTPUT_DIR"
"$CC_BIN" -I"$SOURCE_DIR" \
  "$ROOT/native/mandoc-json/mant-mandoc-json.c" \
  "$SOURCE_DIR/libmandoc.a" -lz -lm \
  -o "$OUTPUT.tmp"
mv "$OUTPUT.tmp" "$OUTPUT"
chmod 755 "$OUTPUT"

echo "built $OUTPUT"
