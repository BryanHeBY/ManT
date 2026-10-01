#!/usr/bin/env bash
# Replay the frozen shared-execution acceptance matrix: oracle acquisition
# (source-hash cached), one-process product screening, admission ledger and
# the frozen checked-in selection. Inputs are generated, never edited; pass
# --check-generation / --check-frozen for read-only verification.
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
exec python3 "$ROOT/scripts/_run_module.py" scripts.roff.fixtures.replay_roff_acceptance "$@"
