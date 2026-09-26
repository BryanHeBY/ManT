# Annotated optimization checkpoint (2026-09-26)

This is a measured checkpoint after PERF02c, not a claim that the full
profiling gate or the old-path performance target has been met. The PERF00
[method and original artifact identities](README.md) remain authoritative.
All changes preserve the native formatter as the body authority; the default
roff route, public schema, version, dependencies, and vendor patches are
unchanged.

## Accepted work and exact savings

| Commit | Work removed on the successful path | Scope limit |
| --- | --- | --- |
| `899c5da9` | Frozen 12-round output and 8-round query measurement, commands, hashes and RSS | Instrumentation, not a product speedup |
| `795ee5f7` | Two redundant C deep result checks after the final native seal | Rust FFI still checks every copied pointer, range and string |
| `acf6d999` | A second immediate `fixed.validate()` with no intervening mutation | Isolation and modified/untrusted bodies still revalidate |
| `30dd0605` | Repeated complete HEAD text/range walks for up to 64 names in one Fixed form | Does not cache the full declaration proof across consumers |
| `cc95d5bc` | Per-owner entry proof repeated by `DocumentValidation` indexing before its existing full Fixed validation | The full validation still runs once in this operation and once during codec preparation; public `DocumentIndex::build` and failed validation still check each owner |

The PERF02c release binary was built from the `30dd0605` checkout with the
PERF02c product diff. It was promoted as `cc95d5bc`; subsequent edits before
the commit affected only test lint syntax, not production code. Its SHA-256 is
`f1a17b0e10b680b8883122b5d473b2ee101848363415b82fb161a3fec5585c6e`.
Rebuilding at committed `cc95d5bc` with the same release command yielded
that exact binary SHA-256 again; the remaining uncommitted files at the time
of this recheck were only this report and the paired runner.
The uninstrumented report is
`target/annotated-perf-p02c.eQyz3e/run-perf02c/results.json` (report SHA-256
`8ef4c49409583bb4cfa1c8af2ba4cdd4870b50b3d3bdd1d09ea002aad2d1427f`).
The same frozen decoded four pages and pristine CVS binary from PERF00 were
used in every checkpoint. All candidates used
`cargo build --locked --release -p mant --features annotated-preview`, 78
columns, `C.UTF-8`, and the same 12/8 interleaved runner. Each row below is
one separate measurement session, so cross-row deltas include machine drift.

| Candidate | GCC annotated / old ms | rclone annotated / old ms | Git annotated ms | Clang annotated ms |
| --- | ---: | ---: | ---: | ---: |
| PERF00 | 414.13 / 305.88 | 776.73 / 543.06 | 17.73 | 10.92 |
| PERF01 | 401.56 / 299.68 | 749.69 / 536.13 | 17.28 | 10.58 |
| PERF02a | 392.06 / 304.19 | 742.71 / 545.59 | 17.28 | 10.31 |
| PERF02b | 397.44 / 310.06 | 748.90 / 549.88 | 17.34 | 10.25 |
| PERF02c | 378.25 / 292.14 | 725.01 / 528.98 | 16.86 | 9.83 |

For the two endpoints, the four-page annotated metrics are shown together.
Wall and CPU are milliseconds, RSS is KiB, and IQR is the wall-time
interquartile range. `/usr/bin/time` CPU has only 10 ms resolution, so the
small-page CPU medians are not meaningful fine-grained speed evidence.

| Page | PERF00 wall / IQR / CPU / RSS | PERF02c wall / IQR / CPU / RSS |
| --- | ---: | ---: |
| GCC | 414.13 / 5.83 / 400 / 58,268 | 378.25 / 4.93 / 370 / 58,612 |
| Git | 17.73 / 0.56 / 10 / 11,402 | 16.86 / 0.33 / 10 / 11,822 |
| Clang | 10.92 / 0.45 / 5 / 10,234 | 9.83 / 0.33 / 0 / 10,572 |
| rclone | 776.73 / 12.24 / 770 / 144,086 | 725.01 / 6.21 / 715 / 144,402 |

The PERF00→PERF02c annotated GCC quartile interval changed from
410.83–416.66 to 375.48–380.41 ms; rclone changed from 771.95–784.19 to
722.00–728.21 ms. These are end-to-end process times, not collector self-time.
Old-route medians also moved, so the full cross-session difference must not
be attributed to the patches. A separate same-session, alternating 12-round
PERF02b/PERF02c output comparison measured GCC 396.61 versus 386.23 ms;
the median of the 12 *paired* differences (candidate minus base) was
-8.71 ms, with all 12 candidate rounds faster. On rclone the median paired
difference was +2.03 ms and only five rounds were faster; that is not a
stable speedup. Git and Clang paired deltas were -0.10 and -0.09 ms, both
small relative to process timing noise. This second measurement used
`scripts/annotated_perf_pair.py`, which checks complete output/diagnostic
identity before timing, alternates binary and page order, verifies frozen
SHA-256s and Git state, and saves each wall/CPU/RSS trial and paired-delta
distribution (individual deltas can be recalculated from the trials). Its raw report
is `target/annotated-perf-p02c-paired/results.json` (SHA-256
`57d5c1eb3766fddbae3526193fe664b946f4be7cb5f740eae847bb4c9df80733`).
The earlier unpersisted paired probe gave a similar GCC direction but is not
used as the auditable performance claim.

The annotated text output, outline, explain and search complete response
SHA-256 values match PERF00 on all four pages at PERF02c. Fixed query gold
self-check and all 51 current cases passed. The measured GCC annotated RSS
median changed from 58,268 to 58,612 KiB; rclone from 144,086 to 144,402 KiB.
These small cross-session differences do not establish a real memory change.
In the PERF02c session the remaining annotated/old output gap was 29.5% for
GCC and 37.1% for rclone. Old and annotated queries do not perform identical
semantic work; their runtime is a reference, not a speedup denominator.

## Investigated but not retained

Two independent PERF03a trials preserved all four-page annotated output,
outline and query identities but did not demonstrate a stable whole-page
benefit. A Rust-only ASCII branch duplicated the existing `unicode-width`
ASCII path without reducing C→Rust calls (raw report
`target/annotated-perf-p03.tLnwkh/run-perf03/results.json`). A C wrapper
actually avoided those calls for U+0020–U+007E; it passed feature and native
boundary tests, but a same-session 12-round paired comparison measured GCC
391.65→391.83 ms and rclone 728.41→727.74 ms, both within variation. Its
raw report is `target/annotated-perf-p03c.2Itj3g/run-perf03c/results.json`.
Both trials were removed with `apply_patch`; neither is in the product commits.
No vendor patch or ABI change was retained.

PERF03b scalar-sink, PERF04 layout-space/allocation batch accounting, and
PERF05 collector-event coalescing were not started: no opt-in per-callback
timing or count yet demonstrates that their expected savings outweigh their
event-order and budget risks. A read-only audit found a possible repeated
sibling scan for `.Lk` descriptions in
`shim/mant_mandoc_annotated_collector_marks.c::mant_annotated_marks_visible_link`.
This is a complexity hypothesis, not a measured real-page hotspot or an
accepted behavior fix. A scale probe and pinned-CVS comparison are required
before changing that boundary.

## Verification and open profiling work

PERF02c passed the parser-only, render-only, annotated-only and
render+annotated `libmandoc-rs` test combinations; `mant-ir` (215 unit
tests), `mant-codec` (475), `mant-query` (74 passed, one ignored), the 51-case
Fixed gold, and strict `mant-ir` Clippy. N/S/F read-only cross-reviews found
no blocking lifecycle, ABI or semantic issue in the accepted changes.
Separately, the all-feature workspace tests, all-target/all-feature strict
Clippy, strict workspace documentation, the native symbol check, offline
packaged-crate check, and the eight-configuration CLI capability matrix
passed. The full `scripts/check.sh --build-profile release` did **not** pass:
it stopped before Cargo at six missing Arch Linux fixture registrations in
the roff audit coverage ledger. Those fixtures and ledgers predate PERF00 and
were not changed in this optimization. Passing the independently run checks
does not imply the remainder of `check.sh` was executed.
The standalone codec consumer check also did not pass: online resolution was
blocked by network access, while offline `--locked` resolution required an
update to its independent Cargo.lock. A read-only check found the same stale
lock condition in the other four independent consumers. The missing
`unicode-segmentation` lock entries predate PERF00; no consumer manifest or
lockfile was changed here. These checks remain unverified until their locks
are synchronized in a separate maintenance unit.

The coarse PERF00 stage probe observed GCC `render_bundle` 302–323 ms and
Fixed lowering 63–69 ms, and rclone 620–638/91–94 ms. `render_bundle`
includes preparation, parsing, formatting, collecting, final checking and
owned transfer, so those numbers cannot be assigned to the collector.
The guide's finer opt-in parse/formatter/display/FFI/Fixed/query stage CPU
and event/width/sink/growth/HEAD counters, plus scale curves for all proposed
hot paths, remain uncollected. This checkpoint therefore does not certify
PERF03b/04/05 or close the profiling portion of PERF06. A later run should
instrument one selected hotspot, then repeat uninstrumented release A/B
before accepting a high-risk native change.
