#!/usr/bin/env bash
# Record existing shared-execution inputs with the registered pristine CVS oracle.
# --check verifies snapshots without rewriting them. Input files never change.
set -euo pipefail
exec python3 "$(dirname "$0")/record_roff_snapshots.py" --matrix shared-execution "$@"
