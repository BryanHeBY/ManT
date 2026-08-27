# mantdoc conformance assets

This directory owns repository-level evidence for replacing `libmandoc-rs`.
It is intentionally outside `crates/mantdoc/`: external roff sources, legacy
oracle snapshots, and large renderer goldens must never become accidental
contents of the published `mantdoc` package.

The versioned M0 manifests under [`manifests/v1/`](manifests/v1/) are the
machine-readable contract for the first migration baseline:

- `oracle.toml` identifies the patched libmandoc implementation exactly.
- `legacy-api.toml` enumerates every exported legacy item and active repository
  consumer, with an implementation owner and migration destination.
- `capabilities.toml` maps each legacy behavior and consumer to its mantdoc
  destination and required exit test.
- `corpora.toml` separates stable upstream, pinned HEAD, fixed fixtures, audit
  ledgers, fuzz seeds, and third-party references by provenance and policy.
- `differential.toml` defines canonical AST, diagnostic, and `mant-ir`
  comparison rules and the accepted-difference key.
- `baseline.toml` records commands, limits, toolchain, the first generated
  parser-to-owned-AST timing sample, and the native canonical snapshot that
  becomes the regression oracle after the temporary C adapter is removed.
- `m3-execution.toml` freezes the selected stable roff execution cases, each
  source hash and the reviewed native raw-roff bounded-report expectation. The
  `mantdoc-corpus-inventory --m3-execution` command validates it without
  extracting or redistributing upstream payload.

The adjacent M4 complete-lane parser/recovery smoke command is
`mantdoc-corpus-inventory --m4-man-smoke`. It checks every one of the 99
checksum-pinned stable man(7) inputs, including 50 reviewed source-malformed,
scope-recovery, and style diagnostic sequences enumerated in
`mantdoc-conformance`. Full native canonical regression uses the separate
snapshot and renderer regression uses M9.

The adjacent M5 complete-lane parser/recovery smoke command is
`mantdoc-corpus-inventory --m5-mdoc-smoke`. It checks every one of the 276
checksum-pinned stable mdoc(7) inputs, including 170 reviewed scanner, scope,
metadata, and validation-recovery diagnostic sequences. Canonical AST, IR, and
renderer promotion is recorded in the frozen migration evidence; maintained
native regression uses the snapshot and M9 renderer lane.

For a fast local aggregate across M3–M6, run
`python3 scripts/run-mantdoc-differential-shards.py /tmp/mandoc-1.14.6.tar.gz --shards 4`.
M5 uses the checksum-ordered `case_index % shard_count` partition; add
`--lanes m3,m4,m5,m6,m9` for the parallel renderer lane.

The adjacent M6 complete-lane parser/preprocessor smoke command is
`mantdoc-corpus-inventory --m6-preprocess-smoke`. It checks every one of the
58 checksum-pinned stable tbl(7) and eqn(7) inputs, including eight reviewed
scanner/tab and malformed-eqn recovery sequences. Canonical AST, IR, and
renderer promotion is recorded in the frozen migration evidence; maintained
native regression uses the snapshot and M9 renderer lane.

The unpublished `mantdoc-conformance` package owns the Rust differential
harness. M1 establishes its case identities; subsequent milestones download or
verify only the declared corpus and emit snapshots below
`tests/conformance/baselines/`. It must reject unknown schema versions and
must not silently update a hash, golden, or accepted difference.

The native canonical snapshot is a release/M11 gate, deliberately separate
from the fast daily differential loop. It parses all 572 checksum-verified
stable inputs through `mantdoc` alone and compares their canonical hashes with
the checked-in snapshot:

```sh
cargo run --locked --package mantdoc-conformance --bin mantdoc-canonical-snapshot -- \
  /tmp/mandoc-1.14.6.tar.gz --verify \
  tests/conformance/baselines/v1/mandoc-1.14.6-native-canonical.sha256
```

The snapshot header retains the original oracle identifier only as promotion
provenance; verification neither links nor invokes any C code.

## Corpus rules

- `mandoc-stable-1.14.6` is the required upstream conformance lane. Its source
  archive is checksum-pinned; M1 can derive a read-only, deterministic inventory
  from it only after verifying the archive SHA-256. Its `regress/` payload has
  not been copied into this directory. Later promotion of any input requires its
  per-file hash, license attribution, and review.
- `mandoc-head-cf84231e` is a non-gating errata/forward lane. It is not assumed
  to be reproducible from a moving CVS checkout; M0 must record an immutable
  archive/tree hash or per-file CVS revisions and hashes before its payload is
  accepted.
- Fixed fixtures remain in `tests/fixtures/roff/`. Their source-specific
  READMEs and license texts are authoritative; the manifest merely makes their
  use and package exclusion machine-checkable.
- Historical audit CSV rows identify external source hashes. They are evidence,
  not redistributable inputs and not runnable tests unless the exact source is
  obtained again from its recorded artifact.
- The `ljh-sh/roff` reference is observation-only by default. Copying a test or
  source file requires an explicit manifest entry, source hash, license text,
  attribution, and review.

## Package boundary

The future `mantdoc-conformance` package has `publish = false` and is excluded
from workspace default members. `cargo package --list -p mantdoc` becomes a
required M1+ check: it must contain only focused, redistributable package tests
and never `tests/conformance/corpus/`, `baselines/`, host audit bundles, or the
legacy C oracle.

## Change procedure

Any update to these manifests must include the reason, exact command, source
or artifact hash, and a focused verification result. Do not regenerate a
baseline merely because an implementation now differs from it. Record a
source-hash-specific accepted difference first, then review it at the relevant
AST, IR, renderer, or release gate.
