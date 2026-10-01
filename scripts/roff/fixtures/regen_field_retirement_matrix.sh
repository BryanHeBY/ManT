#!/usr/bin/env bash
# Record existing field-retirement inputs with the registered pristine CVS oracle.
# --check verifies snapshots without rewriting them. Input files never change.
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
exec python3 "$ROOT/scripts/_run_module.py" scripts.roff.fixtures.record_roff_snapshots --matrix field-retirement "$@"
