#!/usr/bin/env bash
# Restore the registered pristine CVS oracle, never the patched vendor build.
set -euo pipefail
exec python3 "$(dirname "$0")/rebuild_reference_mandoc.py" "$@"
