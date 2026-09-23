# libmandoc structured-rendering internals

Status: historical structured-path module map. The refactor completed on
`dev`; baseline was frozen at `bcd6661d` on 2026-09-21 and the native split
completed at `eb502316`. The active P1 target is the
[native annotated-output P0 contract](native-annotated-output-contract.md).
This map is still accurate for the old files until their staged migration,
but does not prescribe the new display authority or final module layout.

This document is the module and ownership map for the behavior-preserving
`libmandoc-rs` structured-rendering split.  It supplements the frozen
[native structured-rendering contract](native-structured-rendering-contract.md);
it does not change that contract or authorize new structured capabilities.

## Scope and invariants

The refactor keeps the public API, private C ABI, schema, feature names,
dependencies, native execution order, validation order, failure precedence,
budgets, and metrics unchanged.  It does not reorganize pinned CVS sources or
add another roff executor.  Existing tests move only with the module they
exercise and retain their assertions.

The only result access path remains:

```text
native result handle
  -> native result check
  -> handle-bound borrowed result view
  -> transfer-budget and relation validation
  -> private Rust-owned transfer model
  -> public typed StructuredDocument
  -> native handle release
```

No raw slice or string may outlive the native result handle.  Splitting a C
type must not add a heap allocation merely to make the type opaque: controlled
allocation-failure order and cumulative allocation metrics are observable
parts of the existing private contract.

## Rust modules

The final modules follow the boundaries below.  Visibility is limited to the
nearest common parent; the split did not replace private items with crate-wide
visibility.

```text
src/structured/
  mod.rs          public path-preserving re-exports and renderer entry
  source.rs       source identity, coordinates, spans, and provenance
  content.rs      owners, roots, atoms, refs, links, blocks, and roles
  document.rs     metadata and StructuredDocument
  options.rs      profiles, limits, stages, and errors
  tests.rs        public-model invariants

src/ffi/structured/
  mod.rs          narrow orchestration surface
  raw.rs          private repr(C) views, discriminants, and extern functions
  input.rs        bundle descriptors and input-lifetime binding
  session.rs      re-entry guard, native call sequence, and handle RAII
  validation.rs   ABI admission, ranges, keys, and ordered relations
  validation/
    preflight.rs  owned-transfer object, edge, and byte budgets
  transfer.rs     handle-bound borrowed views and owned transfer model
  conversion.rs   owned-transfer to public typed-model conversion
  tests/          ABI, source, prose, resource-failure, and concurrency groups
```

The public `libmandoc_rs::structured::*` paths remain stable through private
submodules plus re-exports.  Raw ABI types stay below `ffi::structured`.
Borrowed views are constructed only from `&ResultHandle` and are consumed
before that handle can be dropped.  The current raw-owned intermediate model
remains distinct from the public typed model in this refactor.

The former private codec `NativeProseProjection` and
`mandoc/structured_document` tree were removed during annotated P1. They were
test-only consumers of the old structured result, not a production fallback.
The active native-to-IR bridge is `mant-codec/src/annotated_fixed.rs` with
borrowed final-run selections and one owned Fixed surface. The public
`libmandoc_rs::structured::*` facade remains intact until the later loader
cutover/removal unit; this module map describes its historical ownership.

## Native modules and ownership

`mant_mandoc_structured.h` remains the single private cross-language ABI
header.  A project-private internal header may contain shared declarations.
Every cross-file C symbol uses the `mant_structured_` project prefix; no C
source file includes another C source file.

```text
shim/mant_mandoc_structured.c           collector dispatch and active buffer
shim/mant_mandoc_structured_session.c   parse/render orchestration and cleanup
shim/mant_mandoc_structured_source.c    bundle/include and source provenance
shim/mant_mandoc_structured_budget.c    charging and controlled allocation
shim/mant_mandoc_structured_builder.c   final result construction
shim/mant_mandoc_structured_link.c      link identity, targets, and label refs
shim/mant_mandoc_structured_marker.c    read-only man list-marker evidence
shim/mant_mandoc_structured_structure.c list/item ownership and node phases
shim/mant_mandoc_structured_result.c    result check, view, transfer, and free
shim/mant_mandoc_structured_abi.c       size, alignment, and offset probes
```

The concrete split was extracted in dependency order: ABI/result leaves,
budget, source, session, then builder/buffer.  Collector dispatch and its
reclaimable token/slot/column/projection storage deliberately remain in one C
translation unit: partial consumption and retirement are one state machine,
and separating them would widen the mutation boundary.  Their representation
is declared in the project-private session/buffer headers, but no other C
module directly accesses or modifies those fields.  A module was added to
`build.rs` in the same commit that connected it; every intermediate commit
continued to compile the supported feature combinations.

| State or resource | Sole writer/owner | Transfer or release boundary |
| --- | --- | --- |
| structured call, parser, renderer, callbacks | session | one reverse-order cleanup path |
| structured TLS, probe and failure injection | session | reset before every return |
| existing generic shim source TLS | legacy shim | installed/restored through existing entry |
| input descriptors | Rust input | borrowed only for the native call |
| source keys, maps, include diagnostics | source | immutable source facts move to result |
| counters, limits, failure record | budget | cumulative counters never become live counts |
| final arrays and owned strings | builder/result | result handle or failed-session cleanup |
| link data and label refs | link builder | result cleanup |
| pending token, live slots, free list | buffer | retire only after pending and slot refs clear |
| partial consumption and projections | buffer | projection storage retires with its token |
| node/content boundary context | collector | active render only; no whole-page event history |

Buffer state is one lifecycle unit.  Collector and session code request
complete operations such as record, consume, discard, clear-pending, and
release; they do not update token, slot, free-list, or projection fields
independently.  Active storage remains separate from returned result arrays.
The final collector release also freezes probe counters before reclaiming the
active sidecar; cumulative builder/content/source counters remain distinct
from active and peak collector storage.

The session module is the only owner of structured-specific TLS.  The generic
bundle/source TLS already present in `mant_mandoc_shim.c` is not migrated by
this refactor.  Independent threads remain concurrent; recursive entry on one
thread remains rejected without adding a process-wide lock.

## Frozen baseline

The pinned oracle SHA-256 remains
`d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d`.
The pristine source archive SHA-256 remains
`6ccee6e73346b3e2c9701ba70dc5ac0525f2dd160cfe98ffc00ad56371d28c7d`.
Behavioral outputs, diagnostic/source examples, and oracle identities remain
in the C01 baseline and contract; this internal refactor does not create new
expected output.

The `bcd6661d` release probes remain the deterministic resource checkpoint:

| Fixture | Events | Tokens | Slot capacity | Sidecar bytes | Cumulative builder bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| GCC | 6,416,335 | 1,232,413 | 16,384 | 917,824 | 53,958,307 |
| Git | 287,477 | 54,832 | 1,024 | 106,816 | 1,940,715 |

Deterministic counters and results must remain equal for an unchanged build
configuration and input.  Wall time and RSS are measured comparatively and
reported as ranges, not asserted as exact values.  The previously recorded
warmed observations were 2.29 seconds/68,712 KiB for GCC and 0.17
seconds/51,504 KiB for Git.

The following baseline commands passed at `bcd6661d` on 2026-09-21:

```sh
cargo test --locked -p libmandoc-rs --no-default-features
cargo test --locked -p libmandoc-rs --no-default-features --features render
cargo test --locked -p libmandoc-rs --no-default-features --features structured
cargo test --locked -p libmandoc-rs --no-default-features --features render,structured
cargo test --locked -p libmandoc-rs --all-features
```

The final gate additionally includes strict native warnings and Rust lint,
ASan, TSan, symbol isolation, packaged-source tests, offline vendor replay,
relevant consumers, and the workspace suite.  Windows and macOS execution is
reported only when actually run by CI or on those hosts.

## Completed verification

The final Linux x86_64 local gate ran on 2026-09-21.  The following were
executed, not merely inspected:

- parser-only, render-only, structured-only, render+structured, and
  all-feature `libmandoc-rs` tests, including strict native warnings;
- the complete `scripts/check.sh --build-profile release` gate, covering the
  workspace, independent consumers, packaged source sets, symbol isolation,
  docs with warnings denied, strict Clippy, fuzz compilation, audits, query
  gold, and the release executable smoke test;
- offline `sync_vendor.py --verify` replay of all 29 locked patches with zero
  vendor differences;
- the mixed Rust/C AddressSanitizer suite and the ThreadSanitizer suite at 64
  rounds per worker;
- release GCC and Git sidecar probes after warming the build cache.

All deterministic probe counters matched the frozen table.  Warm observations
were 2.17 seconds/68,848 KiB for GCC and 0.10 seconds/53,812 KiB for Git.  These
single-host timings are evidence against an obvious local regression, not a
portable performance guarantee.  Windows and macOS execution remains for CI.

The full gate exposed two pre-existing SourceKey migration omissions outside
the mechanical split.  They were repaired separately: profiler fixtures now
pass their exact decoded byte length to lowering, and v1 single-source query
gold coordinates bind explicitly to root `SourceKey` 1 while rejecting other
keys.

The 2026-09-23 C04 follow-up separately replayed the current 31-patch series
from the locked archive with zero vendor differences. This is not a rerun of
the historical release-profile gate, ASan, or TSan checks above.

## Deliberately retained large modules

- `src/ffi/structured/validation.rs` keeps ordered range, key, and
  relationship checks together because their precedence is part of the FFI
  contract.
  Transfer object, edge, and byte accounting is isolated in
  `validation/preflight.rs` because it runs as one separate admission step.
- `src/ffi/structured/raw.rs` and `shim/mant_mandoc_structured.h` remain the
  contiguous Rust/C ABI declarations needed for layout review.
- `shim/mant_mandoc_structured.c` keeps collector dispatch beside the active
  buffer state machine, as described above; it no longer owns session, source,
  result, ABI, budget, or final-builder responsibilities.
- `shim/mant_mandoc_shim.c` remains unchanged.  It jointly supports legacy AST
  export, source authorization, and reference rendering; splitting it is an
  independent risk-bearing unit and does not block structured collector work.

## Commit and review boundaries

Each commit keeps a connected implementation and records the moved owner and
tests run.  Mechanical movement does not change expectations.  A newly found
behavior discrepancy stops the mechanical unit: the corresponding pinned CVS
path and exact reference input are inspected before any separate behavior
change is considered.

At the start and close of every structured-rendering milestone, review file
size together with responsibility count and expected next-stage churn.  Split
before the next capability when a module has accumulated independent owners or
safety boundaries; do not split a cohesive state machine merely to meet a line
target.  Keep mechanical moves separate from behavior changes and rerun the
affected feature matrix after every connected move.

Read-only cross-review occurred after the Rust FFI split, after source/budget/
session extraction, and after buffer/collector encapsulation.  The reviewers
found no blocking regression; their non-blocking notes about shared private
buffer representation and retained cross-module probe aggregation are recorded
above rather than hidden by another algorithm change.
