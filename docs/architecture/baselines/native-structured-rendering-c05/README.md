# C05 native table and fixed-display checkpoint

Status on the current C05 `dev` candidate: the bounded private native → owned → shared-IR
path covers logical `tbl` cells and native fixed geometry, no-fill/literal
displays, and terminal-executed `eqn` words. This is not the default CLI path.
The S1 representative-page and performance gates remain open as described
below; this record does not approve a production switch.

## Delivered units

| Commit | Boundary |
| --- | --- |
| `e139917f` | Native `tbl` row/cell content, empty and rule cells, spans and ownership. |
| `d99ea356` | Table lowering into one shared content store and final IR consumers. |
| `495a4b68` | Pinned terminal observer for direct table drawing geometry. |
| `9d3d0eba` | Checked fixed-view transfer and native table geometry in IR/UI. |
| `af7bd4d1` | No-fill/literal fixed rows, zero-width positions, and executed equation words. |
| `2dfd332f` | Coalesce adjacent physical mappings without refunding per-scalar work. |

The fixed view contains placements into the same logical roots as cells or
display blocks. Generated borders/rules remain decorations. The fixed view
is not a second body or a second link occurrence. Plain tables without native
alignment/rules use the generic table display; geometry-sensitive tables carry
the native fixed view. The pinned `eqn_term.c::eqn_box` emits terminal words
through `term_word`; its inline and standalone examples are therefore retained
as executed logical content, without a second equation layout algorithm.

## Verification performed

- Exact minimal man/mdoc roff sources were run through
  `target/mandoc-migration/reference/mandoc` before the new behavior
  assertions. Relevant execution paths were inspected in pinned `tbl_term.c`,
  `term.c`, `man_term.c`, `mdoc_term.c`, `eqn_term.c`, `mdoc_validate.c`, and
  `tag.c`.
- `libmandoc-rs --features structured --lib`: 198 passed, 2 ignored;
  `mant-codec --features native-structured --lib`: 389 passed, 1 ignored after the
  added scale and long-line checks;
  `mant-render --lib`: 66 passed. Parser-only, render-only,
  structured-only, and render+structured `libmandoc-rs` tests passed; the
  latest render+structured run had 199 passed and 2 ignored.
- `mant-ir`, `mant-ui`, `mant-query`, and `mant-protocol` suites and doctests
  passed. Strict Clippy passed for the modified Rust packages. Native table
  cases include empty/span/rule cells, shared links and fixed placements;
  a real Ratatui `Buffer` checks narrow viewport horizontal viewing.
- `cargo fmt --all --check` and `git diff --check` passed. Vendor patch replay
  and `libmandoc-rs` package file inventory passed at this checkpoint.
- The complete workspace test run passed when its local loopback test servers
  were allowed to bind. The first sandboxed run failed only four
  `mant-sources` tests at `bind()` with `PermissionDenied`; the same full
  command then passed with loopback binding available.

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

At 20,000 rows this is 40,000 logical cells and 40,001 fixed lines. Time and
RSS grow approximately linearly across these sizes; the additional full-IR
peak is about 70 MiB at the largest size. The final IR and native-owned
representation coexist during transfer, so the peak is not the retained IR
size. These figures include the Rust test process, not CLI startup. They are
reproducible with the ignored `native_fixed_table_scale` test and
`MANT_C05_ROWS`/`MANT_C05_PHASE`. The earlier per-scalar placement version
measured 333–347/467–488 ms and 173/234 MiB at 20,000 rows. Adjacent affine
placements were then coalesced: the same content, cells and physical rows
remain, while builder-operation and relation-edge work are still charged for
each terminal scalar. A separate 10,000-character no-fill line had previously
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

No independent C05 cross-review is recorded here. The current result is a C05
implementation checkpoint, not an S1 approval.
