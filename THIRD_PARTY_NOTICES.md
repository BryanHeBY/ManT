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

## Bundled pager

`crates/mant-ui/src/pager/vendor/` contains minus 5.7.2, privately adapted for
static/search paging and independent styled-row redraw. It is licensed under
MIT OR Apache-2.0; both complete texts and the exact adaptation/verification
instructions accompany the sources. It is included in the mant-ui crate, not
listed as a separate registry dependency. Native archives carry its licenses as
`LICENSES/MINUS-MIT` and `LICENSES/MINUS-APACHE`.

## Bundled parser

The native source-reading and decompression boundary belongs to `mant-loader`;
semantic lowering belongs to `mant-codec`. Both are ManT Apache-2.0 crates,
not copies of the upstream parser. Their opt-in `roff` features select the
separately attributed `libmandoc-rs` dependency; the product engine enables that
support by default. Neither package redistributes a second vendored tree.
`mant-query` is also ManT Apache-2.0 code: it queries existing IR and uses the
codec's canonical Markdown artifacts with native features disabled. It does
not contain or enable another native parser copy.

`crates/libmandoc-rs/vendor/mandoc-1.14.6/` is a pinned mandoc 1.14.6 source
snapshot with an ordered local patch series. Its upstream inventory, local
modification summary, exact exception mapping, and complete reusable terms are
documented in
[`crates/libmandoc-rs/THIRD_PARTY_NOTICES.md`](crates/libmandoc-rs/THIRD_PARTY_NOTICES.md)
and [`crates/libmandoc-rs/LICENSES/`](crates/libmandoc-rs/LICENSES/); the exact
reproducible patch inputs remain in the repository beside the vendored tree.
Original source headers remain intact. Native release archives carry the
notices required by the parser sources compiled into their executable.

## tldr-pages content

When `mant --update-tldr` downloads pages from the
[`tldr-pages/tldr`](https://github.com/tldr-pages/tldr) project, those page
contents remain third-party material licensed under Creative Commons
Attribution 4.0 International (CC BY 4.0). ManT preserves the upstream source
path and renders `tldr-pages · CC BY 4.0 · <platform> · <language>` with every
upstream quick reference. The complete license is in
[`LICENSES/CC-BY-4.0.txt`](LICENSES/CC-BY-4.0.txt).

The ManT executable and its embedded quick references are not relicensed by
this notice. The CC BY 4.0 boundary applies only to content whose recorded
origin is `tldr-pages`; embedded project-authored content remains covered by
the containing project's license.

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
