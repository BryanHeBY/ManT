# C05 native table and fixed-display checkpoint

Status on the current C05 `dev` candidate: the bounded private native → owned → shared-IR
path covers logical `tbl` cells and native fixed geometry, no-fill/literal
displays, terminal-executed `eqn` words, and typed reveal of empty/rule cells
and fixed lines in a real terminal Buffer. This is not the default CLI path.
C05's functional and N/F/S review findings are closed; the separate S1
representative-page and production-performance gates remain open below. This
record does not approve a production switch.

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
- Current feature-matrix library tests: `libmandoc-rs` parser-only 96 passed,
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
  archive replayed all 35 patches with `--verify`; the package inventory
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
