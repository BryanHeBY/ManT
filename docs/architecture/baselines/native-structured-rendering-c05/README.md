# C05 native table and fixed-display checkpoint

Status on the current C05 `dev` candidate: the bounded private native → owned → shared-IR
path covers logical `tbl` cells and native fixed geometry, no-fill/literal
displays, terminal-executed `eqn` words, and typed reveal of empty/rule cells
and fixed lines in a real terminal Buffer. This is not the default CLI path.
The original checkpoint's N/F/S findings were closed, but a later C05 review
found five additional fixed/table defects. The repair below addresses
those findings; the separate S1 representative-page and production-performance
gates remain open. This record does not approve a production switch.

## Delivered units

| Commit | Boundary |
| --- | --- |
| `e139917f` | Native `tbl` row/cell content, empty and rule cells, spans and ownership. |
| `d99ea356` | Table lowering into one shared content store and final IR consumers. |
| `495a4b68` | Pinned terminal observer for direct table drawing geometry. |
| `9d3d0eba` | Checked fixed-view transfer and native table geometry in IR/UI. |
| `af7bd4d1` | No-fill/literal fixed rows, zero-width positions, and executed equation words. |
| `2dfd332f` | Coalesce adjacent physical mappings without refunding per-scalar work. |
| `721d396c` | Resolve fixed-display link origins in the final IR. |
| `4f697258` | Enforce table budgets and reclaim fixed-use sidecars on failure. |
| `7e92ba23` | Keep zero-column combining content in fixed placements. |
| `c4ad1419` | Copy unclipped fixed rows across horizontal selection. |
| `0b484d5c` | Observe every fixed table cell's native position; validate cell/point identity in C and Rust. |
| `1cd09ab3` | Retain real rule cells and private point identities through IR, projection, and Buffer reveal. |
| `fa3e9869` | Repair fixed scope handoff, overlay geometry, explicit tbl width/spacing, and bounded scalar validation. |

## Post-checkpoint C05 review repairs

- The table observer now switches after the existing `term_newln()` and before
  `term_tbl()`, so a preceding no-fill/literal display closes before cell
  ownership is bound. Paragraph handoffs close the old physical view after
  the native pre-handler flush. C, FFI, and final IR checks reject a fixed
  display placing content from another block root; this prevents the observed
  double rendering of `after`.
- Backspace marks the following visible placement as an overlay, including
  cross-token, wide/narrow, and boxed-cell uses. A zero-column combining
  placement retains the pending overstrike. Checked transfer and IR reject an
  unmarked later overlap; the safe text projection clears stale wide-glyph
  continuation cells.
- Explicit tbl width and spacing, including `l0`, select native fixed geometry
  according to `tbl_layout.c`/`out.c`. Three validation layers use bounded
  scalar checkpoints for long mixed-width atoms, removing quadratic prefix
  scans and the pseudo 32 MiB relation-failure threshold without refunding
  actual rendering work.

The independent review also exposed a separate unsupported composition:
`.Bd -literal` containing a nested `.Bl` can share one native physical line
between list head and body roots. The current structured model cannot claim a
faithful fixed view for that line, and the structured entry rejects the
combination during result validation. An attempted ordinary-list fallback was
discarded after a fixed-CVS multi-line/double-space comparison showed it could
merge physical lines. This remains a follow-up capability, not a C05 repair
claim.

The repair was verified with the pinned CVS minimal inputs before
new behavior assertions; parser-only 96, render-only 97, structured-only 214
(2 ignored), and render+structured 215 (2 ignored) `libmandoc-rs` library
tests passed. `mant-ir` passed 139 tests and `mant-codec` 396 (1 ignored).
The full workspace all-features test and doctest run passed when local loopback
binding was available; its sandboxed first run failed only the four
`mant-sources` test-server `bind()` cases. Strict workspace Clippy, format
check, offline 36-patch vendor replay, and package inventory passed.

The fixed view contains placements into the same logical roots as cells or
display blocks. Generated borders/rules remain decorations. The fixed view
is not a second body or a second link occurrence. Plain tables without native
alignment/rules use the generic table display; geometry-sensitive tables carry
the native fixed view. The pinned `eqn_term.c::eqn_box` emits terminal words
through `term_word`; its inline and standalone examples are therefore retained
as executed logical content, without a second equation layout algorithm.
The v0.12 JSON/schema shape is unchanged: layout-rule strengths retain their
old serialized form; `TableCell.point` is a private in-memory shortcut, not a
new wire field. JSON round trips remain valid but do not retain that new
cell-to-point reveal shortcut.

## Verification performed

- Exact minimal man/mdoc roff sources were run through
  `target/mandoc-migration/reference/mandoc` before the new behavior
  assertions. Relevant execution paths were inspected in pinned `tbl_term.c`,
  `term.c`, `man_term.c`, `mdoc_term.c`, `eqn_term.c`, `mdoc_validate.c`, and
  `tag.c`.
- Original checkpoint feature-matrix library tests: `libmandoc-rs` parser-only 96 passed,
  render-only 97, structured-only 204 passed/2 ignored, render+structured 205
  passed/2 ignored. `mant-codec` has 391 passed/1 ignored, `mant-ir` 138,
  `mant-render` 66, and `mant-ui` 252. The v0.12 protocol schema snapshot has
  4 passed. Native cases cover empty/span/rule/text-block cells and malformed
  point identities; actual Ratatui Buffers cover narrow/horizontal viewing,
  whole-row rules, origin-preserving stacking, and nested aligned cells.
- Strict Clippy passed for the modified Rust packages with all targets and
  features. The complete workspace test and doctest run passed with local
  loopback binding allowed. A sandboxed run failed only four `mant-sources`
  tests at local test-server `bind()` with `PermissionDenied`; the same full
  command then passed with loopback binding available.
- `cargo fmt --all --check` and `git diff --check` passed. The locked CVS
  archive replayed all 35 patches at this checkpoint. The repair adds patch
  0036, and its offline `--verify` replay passed; the package inventory
  includes the terminal observer and structured shim sources.

## S1 representative-page survey and performance boundary

The private `project_native_manual` path accepted the complete
`entry-name-boundaries-man.1` fixture (two sections, 23 content atoms), and
the C03 tests exercise real outline/explain consumers. The Fedora 44 GCC,
Git, and Clang pages and Windows rclone page were each decoded and run through
that private path. Each returned explicit `Unsupported` at render, rather
than a partial document. For example, the GCC source contains thousands of
`.IX`/`.Sp` requests and `.SS` subsections outside the current structured
macro set. These four pages cannot yet provide a valid new-path end-to-end
timing or memory comparison; long-tail expansion belongs to later coverage
units, not C05.

To exercise page-scale *supported* geometry, the exact generated allbox `tbl`
sources with 1,000, 5,000, and 20,000 rows were run through fixed CVS first.
The 1,000-row case is a retained regression: each of its 2,000 cells has one
logical occurrence while the fixed view contains the native rules. The
ignored release-mode scale probe uses identical rows at all three sizes:

| Rows | Native owned transfer | Full native → final IR | Native/full peak RSS |
| --- | --- | --- | --- |
| 1,000 | 11 ms | 15 ms | 11/16 MiB |
| 5,000 | 58 ms | 88 ms | 35/54 MiB |
| 20,000 | 255–263 ms | 373–385 ms | 133/203 MiB |

Those are the earlier affine-placement measurements, before cell-point
placements and checked reveal. The final `0b484d5c` + `1cd09ab3` candidate was
rebuilt in release mode and measured by running its warmed test executable
directly (not the Cargo driver) on the same generated rows:

| Rows | Native owned transfer | Full native → final IR | Native/full peak RSS |
| --- | --- | --- | --- |
| 1,000 | 10.7–11.0 ms | 16.3 ms | 10.4/15.2 MiB |
| 5,000 | 59.6 ms | 89.6 ms | 35.6/53.4 MiB |
| 20,000 | 273.3 ms | 381.5–389.5 ms | 134.4/198.4 MiB |

After the scalar-index repair, one release native-only 20,000-row probe via
Cargo measured 277.8 ms, 40,002 atoms, 40,001 lines, and 40,000 cells. This
is a single post-change timing, not a warmed direct-executable comparison;
peak RSS and the full native → IR phase were not remeasured here.

The 20,000-row case now retains 40,000 additional zero-width cell-point
placements, bounded by the existing placement, byte, operation, and relation
limits. Time and RSS remain approximately linear; the native-only time is a
little above the prior candidate, while full-IR time overlaps its range.
These are single-host observations, not a production-path comparison.

At 20,000 rows this is 40,000 logical cells and 40,001 fixed lines. Time and
RSS grow approximately linearly across these sizes; the additional full-IR
peak is about 64 MiB at the largest size. The final IR and native-owned
representation coexist during transfer, so the peak is not the retained IR
size. These figures include the Rust test process, not CLI startup. They are
reproducible with the ignored `native_fixed_table_scale` test and
`MANT_C05_ROWS`/`MANT_C05_PHASE`. The earlier per-scalar placement version
measured 333–347/467–488 ms and 173/234 MiB at 20,000 rows. Adjacent affine
placements were then coalesced: the same content, cells and physical rows
remain. Builder operations are still charged for each terminal scalar;
relation edges are charged only for retained placements. A separate
10,000-character no-fill line had previously
failed checked-result validation because per-character placement prefixes
exhausted its bounded scan; fixed CVS renders it as one 10,005-column row,
and the corrected native and final-IR tests now accept it. The 20,000-row
source was 437,825 decoded bytes (SHA-256
`0f736d1df2800f84507e3eab32e5df4d2f2c0258a2d24f64e63843baeacf980b`).

For the *same* 20,000-row source, the preserved old full loader measured
about 44–53 ms and 45 MiB peak RSS; the current old loader measured about
64–71 ms and 71 MiB. These numbers are not fidelity-equivalent to C05: the
old loader lacks the 40,001 native fixed rows, their placements and rule
geometry. They establish the cost of the added C05 representation and also
show that some pre-existing legacy-path cost rose before any C05 collector is
used. We should not discard fixed geometry merely to match the old timing.

An alternating release comparison of the *unchanged production path* used
the same Arch Linux GCC fixture and the existing `measure_native_load`
example, seven same-process observations per run, three runs per revision.
The preserved start-of-work binary at `aa211f73` was compared with the C05
candidate at `a0787e5d` (before the private-path affine fix):

| Operation | Start-of-work samples | C05 `a0787e5d` samples |
| --- | --- | --- |
| Load | about 209–237 ms | about 233–258 ms |
| Index | about 3.7–6.3 ms | about 5.1–6.9 ms |
| Outline | about 0.8–3.3 ms | about 2.3–5.3 ms |
| Explain `-Wall` | about 32–37 ms | about 76–80 ms |
| Peak process RSS | about 76–79 MiB | about 84–96 MiB |

This comparison is **not** a C05 fixed-view benchmark: `mant-engine` does not
enable the private structured feature, and the production path does not
execute these new collectors. The changed work is across the earlier IR,
source-qualified provenance, and query response-content projection
migrations. In particular, explain now reserves and attaches a checked
response-local content closure, including snapshot work; it was absent in
the start-of-work binary. The regression is real for this workload, but the
cost breakdown and an equivalent new-path large-page comparison still need
a separate performance investigation before the S1 exit can be signed off.

Independent N/F/S C05 review of `aa211f73..97cb0840` found native fixed-view,
resource-budget, and consumer gaps. The repairs enforce dedicated table limits,
release active fixed-use sidecars on failure, preserve combining content,
copy complete unclipped fixed rows, and reveal checked cell/line positions.
A second N/F/S read-only review found and then rechecked cell-identity swaps,
source-coordinate stacking, nested alignment, and the old v0.12 wire shape;
the affected regressions and full workspace suite passed after repair. The
remaining S1 gates above are not C05 feature claims and this checkpoint is not
an S1 approval.
