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

## mantdoc special-character catalog

The native `mantdoc` parser contains a Rust representation of the named roff
character catalog from mandoc 1.14.6 `chars.c`. It is Copyright (c) 2009,
2010, 2011 Kristaps Dzonsons and Copyright (c) 2011, 2014, 2015, 2017, 2018,
2020 Ingo Schwarze, and is distributed under the ISC license. The complete
required copyright and permission notice is in
[`LICENSES/mandoc-chars-1.14.6.txt`](LICENSES/mandoc-chars-1.14.6.txt).

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
# Third-party notices for `mantdoc`

## mandoc special-character catalog

`src/chars.rs` contains a Rust representation of the named-character catalog
from `chars.c` in mandoc 1.14.6. The catalog is Copyright (c) 2009, 2010,
2011 Kristaps Dzonsons and Copyright (c) 2011, 2014, 2015, 2017, 2018, 2020
Ingo Schwarze. It is distributed under the ISC license. The complete required
copyright and permission notice is in
[`LICENSES/mandoc-chars-1.14.6.txt`](LICENSES/mandoc-chars-1.14.6.txt).
