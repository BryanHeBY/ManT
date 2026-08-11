# Integration tests

Rust integration tests in this directory exercise public parser behavior that
does not belong in unit tests. Parser regressions should embed or load the
smallest useful roff input and make an explicit assertion; merely placing a
`.roff` file here does not register a test.

The vendor synchronization script separately verifies that `vendor/` is the
exact result of applying `patches/series` to the pinned upstream snapshot. It
does not run parser fixtures.
