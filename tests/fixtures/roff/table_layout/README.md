# Table layout regression inputs

The eighteen `*.1` files are original Apache-2.0 ManT inputs. The two long
boxed cases reproduce the structural mechanisms of the reported awk tables
without substituting for the immutable real awk page in
[`../real/archlinux/`](../real/archlinux/README.md).

`cases.json` binds the exact original source hashes to independent native
facts and content witnesses. Every source was run through the locked pristine
CVS `20261013T025419Z` reference in ASCII, UTF-8, HTML, tree and lint modes
before the expectations were written. Sixteen sources are lint-clean;
`recovery-provenance.1` deliberately includes unsupported table-block requests,
and `rule-payload.1` deliberately supplies content under a rule slot. The latter
payload must remain suppressed, not be resurrected as recovered body text.
The recorded recipe fixes the locale, filenames and the 120-column native
ASCII/UTF-8 profile. Original-case HTML hashes cover the extracted table
projection, not complete page HTML; real-page hashes cover complete stdout.
These hashes bind evidence to inputs, not portable reader geometry to a native
terminal screenshot.

| Inputs | Mechanism |
| --- | --- |
| boxed-expressions, boxed-escape-paragraphs | Actual-sized four-column rule groups and three-column multiline `w(34)` prose, two header rows, box and vertical edges |
| frame-alias, doublebox, allbox-spans | Option aliases, stroke strengths, horizontal/vertical spans and default grids |
| vertical-boundaries, trailing-columns | T& changes, row-local edges and unoccupied logical tails |
| leading-rules, mixed-layout-rules, rule-payload | Whole/layout/cell rules, mixed strokes and source precedence |
| width-units, unbreakable-width, soft-wrap-auto | Native unit conversion, soft preferences, long words and responsive prose |
| font-scope, recovery-provenance | Cell font lifetime and safe source recovery remain independent from decoration |
| independent-tables, empty-rows, shared-layout | Table identity, physical rows and shared native layout storage |

`real_cases.json` binds seven complete immutable upstream pages to their source
hashes, native table counts and twelve selected table probes. The pages contain
104 tables altogether: awk (5), termios.h (2), sort (1), scanf (1), CMake (1),
rclone (87), and groff_man_style (7). Their distribution/release provenance,
compression and licenses remain with the existing source-specific catalogue;
the matrix does not copy those files into a second corpus. Nonzero native lint
statuses are recorded, not rewritten as clean inputs.

## Executable checks

- `libmandoc-rs/tests/table_layout.rs`: native facts, shared layout identity,
  true spans above 65,535, and FFI ABI coverage in the crate's library tests.
- codec `mandoc/tests/tables/layout.rs`: native-to-IR edges, widths, spans and
  bounded derived edge expansion.
- IR/protocol table tests: actual JSON strings, closed wire validation, sparse
  limits and current schema consistency; published v0.12 remains unchanged.
- UI `tests/table_layout.rs`: source → actual JSON → plain/ANSI/TUI Buffer,
  responsive widths, complete span-region alignment, physical row receipts,
  real pages, and actual Markdown readback. Private UI and codec tests cover
  pointer/copy/search/anchor and artifact owner ranges at their owning layers.
- `python3 -m unittest scripts.roff.tests.test_table_fixture_contract`: retained
  source identities and matrix census, with no host mandoc or zstd dependency.

Tests distinguish source text, logical topology, authored edges, physical rows
and viewport placement. No whitespace folding proves row counts or border
endpoints. Portable fenced Markdown may simplify table geometry and active
links; it must not delete original content or addressable anchors. Minimum-width
and work-budget fallback preserves all source cells once. It is not evidence
that every native tbl option, decimal alignment or device geometry is supported.
