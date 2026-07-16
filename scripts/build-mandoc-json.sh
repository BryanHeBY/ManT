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
WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/mant-mandoc.XXXXXX")
CC_BIN=${CC:-cc}

cleanup() {
  rm -rf "$WORKDIR"
}
trap cleanup EXIT HUP INT TERM

if [ "$CC_BIN" = cc ] && [ -x /usr/bin/cc ]; then
  CC_BIN=/usr/bin/cc
fi

curl --fail --location --silent --show-error "$MANDOC_URL" \
  --output "$WORKDIR/mandoc.tar.gz"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256=$(sha256sum "$WORKDIR/mandoc.tar.gz" | awk '{print $1}')
else
  ACTUAL_SHA256=$(shasum -a 256 "$WORKDIR/mandoc.tar.gz" | awk '{print $1}')
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
