#!/usr/bin/env bash
# Regenerate the definition row-machine matrix snapshots from the pinned
# CVS reference binary. Expectations must come from this run, never by hand
# (see tests/roff_lowering/definition_matrix.rs for the row-grouping rule).
set -euo pipefail
REF="${MANT_REFERENCE:-target/mandoc-migration/reference/mandoc}"
[ -x "$REF" ] || { echo "reference binary not found: $REF" >&2; exit 1; }
cd "$(dirname "$0")/.."
# Gate: expectations may only be regenerated from the registered pristine
# oracle. MANT_REFERENCE still overrides the binary location, but the binary
# it names must pass the full preflight (active registry attestation, binary
# hash, ascii/utf8/html profiles) — there is no bypass path.
python3 - "$REF" <<'PREFLIGHT'
import sys
from pathlib import Path

sys.path.insert(0, str(Path("scripts").resolve()))
import mandoc_oracle
import rebuild_reference_mandoc

root = Path.cwd().resolve()
binary = Path(sys.argv[1]).resolve()
try:
    attestation, value = rebuild_reference_mandoc.active_attestation(root)
    archive = mandoc_oracle.repository_path(
        root, value["source"]["archive"]["path"], "source archive")
    rebuild_reference_mandoc.verify_all(root, binary, archive, attestation, value["identity"])
except (OSError, ValueError, KeyError) as error:
    print(f"oracle preflight rejected {binary}: {error}", file=sys.stderr)
    raise SystemExit(1)
print(f"oracle preflight passed: {value['identity']}", file=sys.stderr)
PREFLIGHT
for source in crates/mant-engine/tests/roff_lowering/definition_matrix/cases/*.1; do
  name=$(basename "$source" .1)
  "$REF" -Tascii "$source" | sed -e ':a' -e 's/\(.\)\x08\1/\1/g' -e 's/_\x08\(.\)/\1/g' -e 'ta' \
    -e 's/^[[:space:]]*//' -e 's/[[:space:]]\+/ /g' -e 's/[[:space:]]*$//' \
    | grep -v '^$' | grep -v '(1)$' | grep -Ev '^[A-Z][A-Z ]*$' \
    | grep -Ev '^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
    | grep -Ev '^(Linux|macOS|Darwin|Apple|Ubuntu|Debian|NetBSD|FreeBSD|OpenBSD|AT&T|GNU) ' \
    > "crates/mant-engine/tests/roff_lowering/definition_matrix/cases/$name.expected"
done
echo "snapshots regenerated under crates/mant-engine/tests/roff_lowering/definition_matrix/cases/"
