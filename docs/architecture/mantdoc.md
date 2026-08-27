# `mantdoc` native parser

`mantdoc` is ManT's single Rust implementation of roff, man, mdoc, tbl, and
eqn parsing. It has no C/FFI runtime path and does not depend on the historical
`libmandoc-rs` oracle.

## Design

- The parser accepts `Source` values with explicit logical names. File access,
  compression, include resolution, and source bundles are opt-in adapters with
  bounded authority.
- A parser session produces an immutable, owned arena `Document`, typed
  diagnostics, source locations, and bounded parse statistics. Public traversal
  uses node references and preorder/child iterators rather than recursive
  mutable trees.
- Roff execution, man/mdoc structural parsing, tbl/eqn preprocessing, and
  rendering use explicit session state and resource `Limits`; no host-global
  locale or callback state participates in parsing.
- Optional `serde`, `gzip`, `zstd`, and `render` capabilities are additive.
  Renderer output is bounded and uses the same explicit source policy.

## Regression boundary

The private `crates/mantdoc/tests/conformance/` support owns long-running
upstream checks. It validates a checksum-pinned `mandoc-1.14.6` archive without
copying its payload, then checks every input against an immutable 572-case
native canonical regression snapshot, upstream lint output, and upstream
renderer outputs. `scripts/run-mantdoc-differential-shards.py`
builds the private examples once and runs independent lanes in parallel.

The checked-in expectations are native test assets, not a compatibility
exception ledger: parser behavior must equal the native golden or the gate
fails. The historical extraction and C-oracle work remain recoverable through
Git history and the standalone `libmandoc-rs` repository.

The stricter completion contract is documented in
[`mantdoc-compatibility-goal.md`](mantdoc-compatibility-goal.md). It prioritizes
known execution defects and existing upstream golden files before independent
AST/diagnostic/IR oracle work and the exhaustive real-manual audit.
