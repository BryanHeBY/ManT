# Paired performance measurements

This tool runs retained artifacts; it never compiles, checks out a revision,
changes CPU policy or restores an oracle. Run measurements after builds and
correctness audits finish, without concurrent Cargo or other heavy work.
Results belong under `target/` or an external evidence directory.

## Frozen inputs and modes

The four default inputs are Fedora 44 GCC, Git and Clang, plus the official
Windows rclone release manual. Their stored and uncompressed SHA-256 values
are checked against the permanent fixture README identities. CLI measurements
use original zstd containers. Operation measurements use a deterministic gzip
container made from exactly the same decoded bytes, including rclone CRLF.
Both container identities and the transformation are recorded.

The CLI panel measures fresh processes for text, Markdown, outline Summary,
explain `-h`, and visible literal search `option` with default pagination.
It uses direct display and no colors, writes stdout to `/dev/null`, and includes
startup, real input I/O/decode/load/query/encode, output and destruction.
Independent correctness invocations preserve original stdout, stderr, status,
byte counts and hashes. Hash equality is not semantic acceptance; differences
require the caller's source-bound content/consumer review.

`measure_native_load` accepts gzip plus `load`, `index`, `outline`, `explain`,
`text`, `markdown`, `search`, or `phase`, and an optional query literal. It
decodes before timers and performs one initial load. Seven operation values
are retained; **the median of the last five is one process sample**. Result
construction and allocation (including `Box`) are timed, destruction and
measurement JSON encoding are outside. The initial load is discarded before
load measurements; other modes reuse it. Index rebuilds every operation;
outline is Summary. Initial loading does not assert separately warmed queries.
Use this same example source and auxiliaries with both compared revisions;
old measurement-array formats are rejected rather than silently mixed.

`phase` independently measures uncompressed native parse/owned transfer,
source-less lowering plus recognition, native text encoding and explicit
destruction of those results. A separate source-aware bytes loader operation
includes parsing/lowering allocation but excludes I/O and result destruction.
These wall observations cannot be subtracted from another harness's medians
or called a CPU profiler. Phase source-less and source-aware output are not
two competing product backends.

## Process resources and statistics

Linux GNU `/usr/bin/time` supplies whole-command user/system CPU (milliseconds)
and peak RSS (KiB). Its command rusage includes reaped descendants; this is
not a detached-process tree monitor. Parent spawn-to-wait wall includes this
wrapper. These fields are separate from the inner operation timer. Operation
process resources include initial load, decompression and untimed destruction;
they are **not exclusive query memory or CPU**. Unsupported collectors are
explicitly unmeasured. RSS maxima are reported alongside medians.

The finite, positive timeout is bound in manifest, case cards and raw records.
The child wait is blocking; a separate deadline watchdog kills the isolated
process group on timeout. Watchdog creation/cancel/join are within the parent
wall scope. POSIX `wait(timeout)` exponential polling is deliberately avoided
because it quantizes short processes. No centisecond resource-tool elapsed
value replaces the parent monotonic wall timer.

Each input/mode runs three untimed A and B process warmups, two batches of ten
paired A/B processes with AB/BA order alternating, and ten A/A controls split
five before/five after. Each metric reports both medians/MAD/maxima, signed
`B-A`, relative differences (undefined at zero A), batch directions and A/A
`P95(abs(A2-A1))` noise. Seed `20261002` and 10,000 bootstrap resamples are fixed:
resample process pairs within their original batch size, combine with original
batch weights and compute the median difference's 95% interval. Stable change
requires both batch directions to agree, the interval to exclude zero, and the
absolute median difference to exceed that metric's A/A noise. A stable increase
or decrease is an observation, not automatic acceptance. Failures, timeouts,
signals, unavailable metrics and malformed operation responses retain raw
records; no failed experiment becomes a successful median by deleting samples.

`--pairs`, `--warmups` and `--aa-pairs` permit small tool checks. Their plan is
labelled nonformal. Subsets of pages/modes are partial coverage. Do not count
seven within-process timers as seven independent process pairs.

## Usage and build cards

Build the executable and operation example independently for each retained
revision, in isolated target directories. Run their existing behavior gates
first. Supply a build card for each side, e.g.:

```json
{
  "revision": "full commit or WIP identity",
  "compiler": "full rustc -Vv output",
  "nativeCompiler": "C compiler identity and flags",
  "profile": "release and profile configuration",
  "features": ["default"],
  "cargoLockSha256": "exact revision's lock hash",
  "operationHarnessSha256": "shared operation source closure hash when operation panel is enabled",
  "artifacts": {
    "cli": {"sha256": "retained mant hash"},
    "operation": {"sha256": "retained measure_native_load hash"}
  },
  "allowedOutputDifferences": "specific approved changes, or none",
  "costAcceptance": "predeclared budget/acceptance rule; not inferred from results"
}
```

This is a caller build attestation, not proof this tool independently compiled
the binary. Missing or empty producer strings, malformed feature lists or lock
hashes, and missing artifact bindings are marked incomplete;
the current worktree's commit cannot authenticate an older executable.
The operation source closure is calculated by
`scripts.dev.performance.cards.operation_harness_identity()` from the example
and its two auxiliaries; both build cards must bind that exact source. Identity
admission failure records an incomplete manifest and stops before measurement.
Existing output directories are rejected without overwriting prior evidence.

```sh
python3 -m scripts.dev.performance.runner \
  --baseline target/retained/base/mant \
  --candidate target/retained/candidate/mant \
  --baseline-operations target/retained/base/measure_native_load \
  --candidate-operations target/retained/candidate/measure_native_load \
  --baseline-build-card target/retained/base/build.json \
  --candidate-build-card target/retained/candidate/build.json \
  --output target/performance/comparison

python3 -m unittest scripts.dev.performance.tests
```

`--panel cli` skips operation artifacts; `--panel operation` measures only the
example. Every case has a scope card, independent CLI correctness evidence,
append-only raw sample ledger, per-invocation stderr/operation stdout/resource
records, and summary. The manifest binds inputs, artifacts, harness files,
caller build cards, host/environment, cache policy and sampling plan. The
harness rechecks both executables and stored/transformed input identities after
collection, treating mutation or disappearance as incomplete evidence. A complete measurement is
still `measured-unreviewed`: fidelity, scope coverage and cost acceptance are
caller responsibilities.

Existing `cfg(test)` field scan/projection, owner visit, receipt/device view
and retirement counters remain the independent complexity proof. This tool
does not pretend they are present in release binaries, add production logs or
infer allocations from RSS. Additional parse/cache/clone/index counters require
a separately reviewed instrumentation boundary. No new native dependency or
public IR/schema is introduced by these developer examples.
