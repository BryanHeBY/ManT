# Source from verification scripts only; ordinary development profiles stay intact.
# One-shot gates do not benefit from retaining incremental compiler sessions.
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG-line-tables-only}"
export CARGO_PROFILE_TEST_DEBUG="${CARGO_PROFILE_TEST_DEBUG-line-tables-only}"
