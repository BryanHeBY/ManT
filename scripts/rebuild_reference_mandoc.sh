#!/usr/bin/env bash
# Rebuild the pinned CVS reference binary used by behavioral gates.
#
# The reference lives under target/ (build output), so any `cargo clean`
# deletes it together with the workspace artifacts. Rebuild it from the
# pinned vendor source in a scratch copy — never inside the vendor tree,
# whose contents must stay exactly as committed.
#
# The committed vendor sources carry the crate's shim includes. For a
# standalone binary the output hooks inline to stdio, and the bundle
# reader gets a minimal ENOENT stub: the -Tascii probes the gates run
# never touch the bundle. The recipe reproduces the reference output
# byte-for-byte on the full 54-case definition matrix.
set -euo pipefail
cd "$(dirname "$0")/.."

DEST="target/mandoc-migration/reference/mandoc"
VENDOR="crates/libmandoc-rs/vendor/mandoc-cvs-20260927T130954Z"
SHIM="crates/libmandoc-rs/shim"
[ -d "$VENDOR" ] || { echo "pinned vendor source not found: $VENDOR" >&2; exit 1; }

if [ -x "$DEST" ]; then
    echo "reference already present: $DEST"
    exit 0
fi

SCRATCH="$(mktemp -d /tmp/mant-reference.XXXXXX)"
trap 'rm -rf "$SCRATCH"' EXIT
cp -a "$VENDOR/." "$SCRATCH/"
cp "$SHIM/mant_mandoc_shim.h" "$SCRATCH/"
cat > "$SCRATCH/mant_mandoc_output.h" <<'HEADER'
#ifndef MANT_MANDOC_OUTPUT_H
#define MANT_MANDOC_OUTPUT_H
#include <stdio.h>
#include <wchar.h>
#include <stddef.h>
static inline void mant_mandoc_output_write(const void *buf, size_t sz) { fwrite(buf, 1, sz, stdout); }
static inline void mant_mandoc_output_utf8(int c) { putchar(c); }
static inline int mant_mandoc_utf8_width(int c) { return wcwidth((wchar_t)c); }
#endif
HEADER
printf '#include <errno.h>\nint mant_mandoc_read_bundle(void *c, const char *f){(void)c;(void)f;errno=ENOENT;return -1;}\n' \
    > "$SCRATCH/mant_stub.c"

cd "$SCRATCH"
./configure >/dev/null 2>&1
cc -c mant_stub.c -o mant_stub.o
printf 'LDADD += mant_stub.o\n' >> Makefile.local
make mandoc >/dev/null 2>&1
[ -x mandoc ] || { echo "reference build failed" >&2; exit 1; }

cd - >/dev/null
mkdir -p "$(dirname "$DEST")"
cp "$SCRATCH/mandoc" "$DEST"
chmod +x "$DEST"
echo "reference rebuilt: $DEST"
