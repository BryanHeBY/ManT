# Integration tests

Rust integration tests in this directory exercise public parser behavior that
does not belong in unit tests. Parser regressions should embed or load the
smallest useful roff input and make an explicit assertion; merely placing a
`.roff` file here does not register a test.

`compatibility.rs` exercises enabled and disabled font/catalogue/equation
capabilities, the standard-font controls, native HTML and real JSON roundtrips.
The repository's `scripts.checks.check_libmandoc_features` module runs it in
thirteen isolated configurations together with private C/Rust ABI checks.
Exact inputs were first checked with the registered pristine CVS oracle; enabled
compatibility enhancements are explicitly distinguished from that baseline.

`string_lookup.rs` checks eighteen pristine-derived namespace lifecycle cases,
including live/empty/cleared values, exact key lengths, overrides, aliases,
renames, header resets and fallback behavior. Expected TEXT leaves and five
reference profile receipts live in `fixtures/string_lookup.json`. Diagnosed
inputs remain explicit negative controls rather than valid-input claims.

`display_fill_modes.rs` checks implicit and explicit display closure against
eighteen independently captured sources, including owned no-fill flags,
HEAD/BODY provenance, JSON and native renderer output. `horizontal_lines.rs`
checks eleven native drawing sources with literal, escaped and Unicode
delimiters and custom glyphs. Their exact pristine profile receipts and gold
live in `fixtures/display_modes.json` and `fixtures/horizontal_lines.json`;
optional renderer and serde assertions remain feature-gated.

The vendor synchronization script separately verifies that `vendor/` is the
exact result of applying `patches/series` to the pinned upstream snapshot. It
does not run parser fixtures.

`thread_safety.rs` is different from an ordinary correctness fixture: its
tests remain ignored under `cargo test` and only become meaningful when the
repository-only `scripts/check-thread-safety` runner builds Rust, the standard
library, and vendored C with ThreadSanitizer. The runner also checks the C
archive for instrumentation so a green result cannot accidentally cover only
the Rust half of the FFI boundary. It covers independent memory sessions,
native source-tree includes, virtual `SourceBundle` trees, and all optional
renderer formats. The published crate includes the ignored test source, but
not that maintenance runner; use a matching ManT repository tag to reproduce
the mixed-language sanitizer build.

`renderer.rs` is gated by the default-off `render` feature. It owns exact
ASCII and HTML goldens, deterministic UTF-8 and locale-state checks, output
limit boundaries, process-stdio isolation, concurrent formatter isolation,
and repository-only reuse of the separately licensed real roff corpus. The
real-fixture test skips only in the published crate's staged package, where
sibling repository fixtures are intentionally unavailable. The ASan runner
executes the exact caller-buffer and output-limit regression under mixed
Rust/C instrumentation.
