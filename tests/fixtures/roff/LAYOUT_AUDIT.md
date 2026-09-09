# Roff renderer-layout audit ledger

`LAYOUT_AUDIT.csv` is an independent, opt-in ledger for local comparisons
between a `man(1)`-compatible reference renderer (normally GNU groff on Linux)
and ManT's semantic `--format man` text. It does not replace the content
fidelity or AST-to-IR structure ledgers, and it never invalidates or requires a
re-run of their completed rows.

The audit uses the shared `audit-roff-fidelity.py` rendering path, so aliases,
manual-root resolution, terminal cleanup, and the controlled `MANWIDTH=200`
reference environment have one implementation. It then derives a deliberately
narrow layout signal:

- authored relative indentation collapse at a unique whole-line anchor inside
  a no-fill or literal-display region; a renderer's implicit display gutter is
  explicitly out of scope;
- blank-line spacing changes between two adjacent source lines when the source
  itself contains a blank line or `.sp` request inside a recognised no-fill or
  literal-display region;
- two short no-fill reference lines that ManT renders as one line, unless the
  same text is also attributable to a flowed or single-line source occurrence;
  this avoids treating formatter wrapping or repeated text as a lost boundary.

It records blank-run counts, indentation levels, and aligned anchor counts in
its JSON report for human review. It does **not** compare ordinary paragraph
wrapping, absolute terminal columns, headers, page furniture, or general table
geometry: those vary with the formatter, device, macro package, and ManT's
intentional copy-friendly text presentation.

| Field | Meaning |
| --- | --- |
| `corpus`, `path`, `section`, `source_sha256` | Exact decompressed manual identity, independent of the other ledgers |
| `layout_schema` | Renderer-layout probe version that produced the row |
| `scan_status` | Latest automatic result: `clean`, `review`, or `hard-failure` |
| `review_status` | Human disposition: `not-required`, `pending`, `false-positive`, `confirmed-open`, or `confirmed-fixed` |
| `note` | Human conclusion, reference environment, and any focused regression |

Run it only for a newly added corpus or an explicitly chosen sweep; ordinary CI
does not call it. A normal run is incremental against this ledger alone:

```sh
cargo build -p mant
python3 scripts/audit-roff-layout.py --manpath /tmp/new-release/share/man \
  --corpus new-release-amd64 --max-pages-per-section 20 \
  --json /tmp/mant-layout.json --findings-only
```

To add layout evidence for exactly the source bytes already comparable in the
content-fidelity ledger, select that corpus explicitly. The content ledger is
only read as an immutable identity index; its historical `skipped` and
`hard-failure` rows are not comparable renderer baselines and are excluded. It
is neither re-rendered nor rewritten:

```sh
python3 scripts/audit-roff-layout.py --manpath /tmp/old-release/share/man \
  --corpus old-release-amd64 --replay-fidelity-records \
  --json /tmp/mant-layout-old-release.json --findings-only
```

Use `--recheck-recorded` only when changing this layout probe or deliberately
revisiting a renderer result. Keep reviewed third-party output in a local
scratch directory; commit the compact ledger conclusion and a focused licensed
fixture only after confirming a real ManT defect.

After a narrowly scoped lowering fix that can only remove a known candidate,
`--recheck-review-recorded` replays just unchanged rows whose latest layout
status is `review`. This keeps a confirmation pass proportional to the actual
finding set rather than rescanning an entire local corpus.

## 2026-08-24 BSD closure result

All 2,357 NetBSD 11.0 pages and all 6,363 DragonFly BSD 6.4.2 pages were clean
under the groff-backed source-gated layout oracle. The temporary OpenBSD
current sample was also 200/200 clean, and all 36 self-contained ports manuals
were clean. These results concern authored no-fill boundaries, relative
indentation, and source-requested spacing only; they do not redefine terminal
wrapping or formatter margins as ManT contracts.

## Source-geometry acceptance

`LAYOUT_ACCEPTANCE.json` binds the parent-relative geometry migration to producer
commit `4d7fc0ed`, executable/source hashes, 46 original reduced inputs, reviewed
mandoc CVS HEAD and groff differences, and release performance samples. The
inputs are retained in the record so reproducing the source matrix does not
depend on a temporary review directory. Its renderer commands consume each
case's `source` through stdin; native page furniture and soft wrapping are not
byte-equality assertions.

The acceptance matrix is also guarded by deterministic production tests:

| Coverage | Regression boundary |
| --- | --- |
| A01: btrfs six-column layout | `mant-ui/src/document/tests/source_geometry.rs`, real-manual probe in `mant-ui/examples/layout_profile.rs` |
| A02–A03: RS state, units and bounds | `mant-engine/src/mandoc/tests/layout_geometry.rs`, `mant-engine/src/mandoc/layout/distance.rs` |
| A04: both ownership normalization paths | `mant-engine/src/definitions/normalize.rs`, source-bound explanation gold |
| A05–A08: labels, bodies and native styles | `mant-engine/src/mandoc/tests/layout_geometry.rs`, `mant-ui/src/document/tests/layout.rs` |
| A09–A10: display widths and original term lines | `mant-protocol/src/presentation/geometry.rs`, `mant-engine/tests/inline_terms.rs`, UI anchor/layout tests |
| A11–A12: independent gaps and item boundaries | `mant-engine/tests/man_paragraph_boundaries.rs`, `mant-engine/tests/relative_scope_paragraph_spacing.rs`, `mant-ui/src/document/tests/item_spacing.rs`, shared gap validator tests |
| A13: HP/in and literal transitions | `mant-engine/tests/hanging_literal_geometry.rs`, source geometry tests; unsupported device requests remain explicit in mant-roff(7) |
| A14: hard/soft rows and selection | `mant-ui/src/document/tests/layout.rs`, `mant-ui/src/document/tests/zero_width.rs`, wrapping/search tests |
| A15: semantic/target conservation | 199 query gold cases over 125 paths / 122 source identities; 51 target, semantic and projection fixtures |
| A16–A17: translation and narrow bounds | `mant-ui/src/document/tests/source_geometry.rs`, shared geometry/table tests and `mant-ui/src/document/wrap.rs` |
| A18: contracts and evidence | IR/manual/README/changelog updates, v0.11 schema snapshot, source hashes and current performance in the acceptance record |

Paths in the table are relative to `crates/`. The full local gate and the
all-feature workspace run passed for that producer. The 51-page layout,
fidelity and structure replays produced byte-identical CSVs to the existing
reviewed ledgers. This includes the known groff recursion failure and reviewed
layout/content signals; it does not reclassify them as clean. No new 45,036-page
sweep or native Windows/macOS run is claimed by this record.

### Execution-row and facade follow-up

`LAYOUT_FOLLOWUP_ACCEPTANCE.json` records the R01–R06 follow-up at producer
`0779e2be`. It retains 15 additional original source inputs, reference versions,
binary/source hashes, regression boundaries and the exact verification scope.
The earlier acceptance record is unchanged rather than relabeled as new proof.

The full local gate, all-feature workspace tests and all 199 source-bound query
cases passed. The 51-page fixture replays retained the previous layout/fidelity
review candidates and reference recursion failure; their CSVs, and the
structure CSV, are byte-identical to the reviewed ledgers. All 46 prior reduced
inputs plus the 15 new inputs were executed with the final producer, the pinned
mandoc snapshot and groff 1.24.1. Reference comparisons use executed row events,
blank-row counts and relative origins, not formatter page furniture equality.

Regression tests additionally cover empty font operands versus zero-width
glyphs, explicit `fi`/`nf` inside displays and synopses, empty display/list-item
predecessors, bounded gaps separated by an empty literal row, and CLI/ANSI/TUI
row conservation. The source-based tests are independent of the local review
directory. This follow-up does not claim a new full-corpus or native
Windows/macOS run.
