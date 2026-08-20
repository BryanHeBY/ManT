# Integration tests

Rust integration tests in this directory exercise public parser behavior that
does not belong in unit tests. Parser regressions should embed or load the
smallest useful roff input and make an explicit assertion; merely placing a
`.roff` file here does not register a test.

The vendor synchronization script separately verifies that `vendor/` is the
exact result of applying `patches/series` to the pinned upstream snapshot. It
does not run parser fixtures.

`thread_safety.rs` is different from an ordinary correctness fixture: its
tests remain ignored under `cargo test` and only become meaningful when the
repository-only `scripts/check-thread-safety` runner builds Rust, the standard
library, and vendored C with ThreadSanitizer. The runner also checks the C
archive for instrumentation so a green result cannot accidentally cover only
the Rust half of the FFI boundary. The published crate includes the ignored
test source, but not that maintenance runner; use a matching ManT repository
tag to reproduce the mixed-language sanitizer build.
