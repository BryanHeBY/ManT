#!/usr/bin/env bash
# Record the existing definition row-group corpus with pristine CVS.
# --check verifies snapshots without rewriting them. Input files never change.
set -euo pipefail
exec python3 "$(dirname "$0")/record_roff_snapshots.py" --matrix definition "$@"
