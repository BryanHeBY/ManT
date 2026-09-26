# PERF00 annotated process baseline

`scripts/annotated_perf_baseline.py` records a fresh-process comparison of
pristine CVS mandoc, the old ManT route, and `--annotated-preview`. It does
not build binaries or decode fixtures. Freeze those files under repository
`target` first, and do not rebuild into their paths while measuring.

For the conventional four-page frozen directory, run from the repository root:

```sh
python3 scripts/annotated_perf_baseline.py \
  --frozen-dir target/annotated-perf-p00.hUTLAs \
  --out target/annotated-perf-p00.hUTLAs/run-da6ddd7d
```

The directory must contain exactly one `mant-*` binary, `mandoc-cvs`, and
decoded `gcc.1`, `git.1`, `clang.1`, and `rclone.1`. The `mant-<hex>` suffix
must match the current checkout. The script derives an in-memory manifest
with all six SHA-256 values and the established query terms (`-x`, `--help`,
`-help`, `--help`). An already existing `--out` directory is rejected, not
overwritten. A dirty worktree requires `--allow-dirty` and is fingerprinted
in the report; run a clean committed baseline when possible.

For another fixture set or a candidate binary built from WIP, supply an
explicit JSON manifest and a separate new output directory:

```json
{
  "schemaVersion": 1,
  "label": "candidate-label",
  "sourceHead": "40-character Git HEAD",
  "binaries": {
    "mant": {"path": "target/frozen/mant-candidate", "sha256": "64 lowercase hex characters"},
    "native": {"path": "target/frozen/mandoc-cvs", "sha256": "64 lowercase hex characters"}
  },
  "fixtures": {
    "git": {"path": "target/frozen/git.1", "sha256": "64 lowercase hex characters", "query": "--help"}
  },
  "oracleIdentity": "cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1",
  "build": {
    "mant": "cargo build --locked --release -p mant --features annotated-preview",
    "native": "pristine pinned CVS, make CFLAGS=-O2 -pipe mandoc"
  }
}
```

Paths are repository-root-relative; with `--frozen-dir`, relative artifact
paths are relative to that directory instead. An optional `oracle` object
with `path` and `sha256` can pin the fixed reference binary too. The script
checks every artifact hash before and after sampling, checks the manifest
file hash when supplied, and refuses a changing worktree. For WIP, tracked
diff and untracked file hashes are captured; this is an identity record, not
a substitute for a committed source tree. Record the exact build flags and
oracle attestation in the tracked result summary; the automatic manifest
cannot infer them from an executable.

The report at `--out/results.json` includes complete commands, source and
binary identities, successful exit codes, stdout/stderr hashes, compact JSON
query/outline summaries, 12 interleaved output rounds, eight interleaved
explain/search rounds, per-trial wall/CPU milliseconds and peak RSS KiB, plus
median, quartiles, IQR, minimum, and maximum. It uses 78 columns, `C.UTF-8`,
warm page cache, one new process per trial, and `/usr/bin/time`. Its CPU time
has only 10 ms resolution, so small-page CPU medians are coarse. Wall time
includes wrapper, startup, load, rendering, output, and exit; it is not
collector self-time. An outline's compact semantic summary contains only
top-level fields such as `semanticsComplete`; the full JSON SHA-256, not an
outline node count, establishes its response identity.
Native mandoc has no equivalent IR query, so only the two ManT routes are
compared for explain/search. Output hashes between native and ManT include
different framing and are not expected to match byte-for-byte.

This runner provides the uninstrumented end-to-end measurement. Coarse
stage timing and opt-in counters must be reported separately, with their
probe boundaries and overhead documented; do not infer native collector
cost by subtracting total CLI times. Keep raw reports under `target` and
commit only compact summaries, identities, methods, and limitations here.

## Frozen baseline at `da6ddd7d` (2026-09-26)

The source HEAD was `da6ddd7d7c4d943d744f4ca66be536b82c74cff6` and the
tracked diff was empty. Only this new script and README were untracked while
measuring; the runner recorded their content hashes and verified the worktree
was unchanged during sampling. This measured-result section was appended
afterward, so its current README hash intentionally differs from the raw
report's measurement-time untracked-file hash. The release ManT binary was built with
`cargo build --locked --release -p mant --features annotated-preview` using
Rust 1.98.0 and GCC 16.2.1, with no task-specific `RUSTFLAGS`, `CFLAGS`, or
`CC` override. The pristine pinned CVS binary was the frozen `make -j4
CFLAGS='-O2 -pipe' mandoc` artifact from the earlier four-page measurement.
The fixed reference oracle is
`cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1` (attestation in
`crates/libmandoc-rs/upstream/oracle/attestations/`); its separate
`target/mandoc-migration/reference/mandoc` SHA-256 is
`d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d`.
The pristine performance binary is not the reference binary; the former's
output was checked against the latter in the earlier frozen baseline.

| Frozen artifact | SHA-256 |
| --- | --- |
| ManT release | `63818f64fac827c1700dcc224bed46b8e82c650d5efba89a464ecf47ac09d217` |
| Pristine CVS performance binary | `4892dbc7f379a956d65413b8597402f9454d38c83374188a5f0e76be93f7e88a` |
| Decoded GCC | `a86bd1d671aa2afff558eb3b7c8b6e2a8bbaf55cb9fb2a6549b7538dd56e0f0f` |
| Decoded Git | `2736b9d20cd9a36c7971ccdbd7ba5e71cea543960340bb9a27984369df559862` |
| Decoded Clang | `ff10a1611fc293291388feabf2d988c2c8950c0e784a34d4fcea5b4eb966fce0` |
| Decoded rclone | `f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e` |

The canonical raw report is
`target/annotated-perf-p00.hUTLAs/run-da6ddd7d-cpu/results.json`; its
generated manifest SHA-256 is
`eeaf097812e673f099deffd485ff2f7ac78c5fff8bf0b2c4135f59099e1292c6`.
The first report without CPU columns remains in the neighboring
`run-da6ddd7d` directory and is not combined with this run. On WSL2/i5-13500,
the canonical medians were:

| Page | CVS output ms | Old output ms / MiB | Annotated output ms / MiB | Old → annotated explain ms | Old → annotated search ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| GCC | 74.20 | 305.88 / 87.7 | 414.13 / 56.9 | 343.18 → 442.80 | 404.90 → 420.70 |
| Git | 4.44 | 14.11 / 14.4 | 17.73 / 11.1 | 15.14 → 18.48 | 17.39 → 18.45 |
| Clang | 3.52 | 8.98 / 12.5 | 10.92 / 10.0 | 9.25 → 10.94 | 10.74 → 11.22 |
| rclone | 152.09 | 543.06 / 238.2 | 776.73 / 140.7 | 594.63 → 830.46 | 678.97 → 810.77 |

The output SHA-256 identities of all three routes on all four pages equal
those in the earlier frozen P0/G1 records. This establishes output stability
across the two measurements, not equivalent semantic work between old and
annotated queries. CPU and IQR values, command arguments, stderr identities,
outline/query response identities and compact semantic summaries are in the
raw report. The script does not interpret an ordinary process-level delta as
a collector-only cost.

The existing ignored release-stage probe was also run twice in inverted page
order with `cargo test --locked --release -p mant --features annotated-preview
annotated_preview_four_page_stage_costs -- --ignored --nocapture`. It observed
GCC `render_bundle` 302–323 ms and Fixed lowering 63–69 ms; rclone 620–638 ms
and 91–94 ms, respectively. `render_bundle` includes input preparation,
native parse/format/collector/final validation and FFI owned transfer; it
must not be labeled collector self-time. This probe measures wall time only,
not stage CPU, and does not count per-event work. Finer opt-in stage and
counter attribution remains a separate profiling check before considering
high-risk scalar-sink, layout-span, or collector-event changes.
