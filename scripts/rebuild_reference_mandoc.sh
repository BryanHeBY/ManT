#!/usr/bin/env bash
# Restore the registered pristine CVS oracle, never the patched vendor build.
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exec python3 "$ROOT/scripts/_run_module.py" scripts.roff.oracle.rebuild_reference_mandoc "$@"
