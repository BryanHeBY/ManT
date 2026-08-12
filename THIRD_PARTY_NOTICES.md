# Third-party notices

ManT is licensed under Apache-2.0. The repository also contains the following
third-party material, each kept within an explicit distribution boundary.

## Rust dependencies

Native executables include Rust packages from the locked Cargo dependency
graph. Their selected license texts and package/version mapping are generated
by cargo-about in [`THIRD_PARTY_LICENSES.html`](THIRD_PARTY_LICENSES.html).
Every native release archive carries this file as
`LICENSES/RUST_DEPENDENCIES.html`; CI rejects it when it no longer matches the
locked multi-platform graph.

## Bundled parser

`crates/libmandoc-rs/vendor/mandoc-1.14.6/` is a pinned mandoc 1.14.6 source
snapshot. Its upstream inventory, exact exception mapping, and complete
reusable terms are documented in
[`crates/libmandoc-rs/THIRD_PARTY_NOTICES.md`](crates/libmandoc-rs/THIRD_PARTY_NOTICES.md)
and [`crates/libmandoc-rs/LICENSES/`](crates/libmandoc-rs/LICENSES/).
Original source headers remain intact. Native release archives carry the
notices required by the parser sources compiled into their executable.

## Test fixtures

Fixed real-world roff fixtures under `tests/fixtures/roff/real/` are used only
for parser regression tests and are not included in crates.io packages or
native release archives. Their per-file provenance, transformations, hashes,
and license mapping are recorded in
[`tests/fixtures/roff/real/README.md`](tests/fixtures/roff/real/README.md), with
complete applicable terms under `tests/fixtures/roff/real/LICENSES/`.

## Screenshot fonts

The deterministic screenshot tooling carries JetBrains Mono 2.304 font files
under `docs/assets/fonts/`. They are licensed under the SIL Open Font License
1.1; the complete text and pinned file hashes are stored alongside the fonts.
The font files are repository documentation assets and are not included in
crates.io packages or native release archives.
