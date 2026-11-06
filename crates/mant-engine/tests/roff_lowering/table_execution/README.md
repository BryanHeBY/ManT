# Native table execution regression matrix

These fixtures bind complete source inputs to the pristine pinned CVS
`20261013T025419Z` oracle. Expectations were collected before assertions from
ASCII, UTF-8, HTML, tree and lint profiles. Hashes identify that evidence;
portable viewport geometry is not required to reproduce a native screenshot.

| Fixture | Executed domain |
| --- | --- |
| `cases.json` | 1,188 configurations / 1,045 distinct sources; includes the original 198/64/96 probes and 609 configurations, T1 (192) and T2 (240), and shared execution controls |
| `adjacent.json` | 65 source configurations covering original number spelling, spans, repeated/missing layouts, rules, motions, line lengths and source-relative margins |
| `motion_controls.json` | Eleven motion/empty-word controls: source-order NBRSP, authored blanks, BACKAFTER versus BACKBEFORE, and vertical-space debt |
| `consumers.json` | 62 selected complete sources for JSON validation, actual CLI/PTY, Markdown two-round readback, query ranges, styles, entry owners and real TUI cells/navigation |
| `styles.json` | Independent font/link facts for selected consumer inputs; a strong text cell is distinct from code-styled text |

T1 crosses two dialects, 24 table programs and four following owners. T2
crosses eight programs, ten contexts and three followers. Tree-derived
expectations verify actual table/HEAD ownership, not just input spelling. All
973 native-table sources assert declared columns, native row kinds and spans;
the other 72 sources are explicitly table-free SY controls. Complete native
follower words and no-fill witness rows are checked in their original owner,
including the legacy GAMMA/DELTA suffixes.
Seventy-two original SY controls contain no native table and are explicitly
classified as such. Recovery and warning inputs retain their qualifications;
a nonzero lint result is not reclassified as a clean source.

`roff_lowering::table_execution` checks complete post-table words, their
original section/inline owner, real declared topology and JSON strings. It
preserves opposite controls where a bare zero-advance marker must remain
active. The algorithm follows `tbl_term.c::term_tbl`: fill all non-span
columns, including missing declared slots, before flushing any column. Whole
rules and empty cells do not imply that a native word or flush occurred.
Numeric padding uses original source spelling and `out.c::tblcalc` bounds.

Codec-private `tables::recovery::budget_tests` reuses 16 exact full sources
with the recovery-attempt budget exhausted. These are actual exhausted-path
runs, not inferences from default-budget success. A separate private
column-work test proves bounded execution fallback retains safe text and
reports ContentCoverage. Structural work and measurement callback limits are
also tested across the libmandoc FFI.

The consumer subset is shared by `native_table_execution` engine/UI tests and
CLI process tests. Real PTYs are distinguished from render-library width
arguments. Query checks resolve response-local ranges, Unicode scalar
coordinates and original entry bindings. Markdown portable fences can flatten
active cell links; this does not permit following ordinary links or source
owners to migrate. No matrix here claims every native tbl option is supported.

Full real-page table and following-owner fixtures remain in
`tests/fixtures/roff/table_layout/`, referring to the existing immutable source
catalogue rather than duplicating upstream pages. Audit totals, one-shot
profiles and human adjudication live outside tracked source under `target/`.
