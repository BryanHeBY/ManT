# mantdoc

`mantdoc` is `ManT`'s native Rust replacement for `libmandoc-rs`.  It is an
unpublished `0.1.0-alpha` workspace crate while the parser is being built in
milestones.  It contains no C build script, FFI, native library link, or unsafe
code. See the repository [migration guide](../../docs/mantdoc-migration.md) for
the deliberate public API differences from `libmandoc-rs`.

The frozen pre-0.1 surface is deliberately byte-first and storage-independent:

- `Parser` accepts a `Source` and a caller-selected `ParserConfig`/`Limits`.
  `Limits::max_source_lines` independently bounds source-map line-index storage.
- `Document` owns an immutable arena, exposes opaque `NodeId` and `NodeRef`,
  and offers iterative traversal. It owns a source map; node and diagnostic
  spans use opaque `SourceId` values resolved through the document's logical
  source names and derived line/byte-column positions. Arena, string-table,
  and source-map storage indices never become logical serialized data.
- `SourceResolver` is explicit; `SourceBundle` is an in-memory, bounded,
  logical-path implementation that never falls back to the host filesystem.
- The raw `Parser::parse` core never performs transport or filesystem I/O.
  `parse_bytes`, `parse_file`, `parse_file_in_root`, and `parse_bundle` are
  opt-in adapters:
  `gzip`/`zstd` frame decoding is feature-gated and bounded by the same root
  source limit. File input retains caller-provided logical identity; a
  contained-root parse derives a canonical root-relative identity and
  authorizes only `.so` files below that root.
- Diagnostics carry stable codes independently from their messages.
- `special_character` exposes the complete pinned mandoc 1.14.6 named-character
  catalog without a build-time C dependency, distinguishing visible Unicode
  scalars from known zero-width formatter controls.
- With the optional `serde` feature, `LogicalParseReport` is the versioned
  durable exchange form: it contains the logical AST, typed diagnostics,
  logical source names/line-columns, and counters without exposing `NodeId` or
  `SourceId`. Deserializing this value does not reconstruct a parser session or
  an arena-backed `Document`.

M2 implements the bounded byte scanner, argument lexer, dynamic control and
escape characters, and visible-text normalization on top of these contracts.
M3 adds a per-parse roff environment: bounded strings, integer registers,
copy-mode and nested macros, delayed expansion, numeric/nroff inline
conditionals, and explicit `.so` resolution. Resolved includes execute at their
source position, share the session environment, retain source-map identities,
and are bounded for cycles, nesting, bytes, sources, lines, and diagnostics.
The supported `.while` subset rechecks numeric/register predicates and enforces
both local and aggregate iteration budgets. It includes nested `\\{ ... \\}`
scopes whose text, environment requests, and ordinary controls re-execute on
each iteration, using explicit collection/execution stacks bounded by
`max_tree_depth`; `.break` exits the nearest loop. Simple macros invoked in a
scope execute with their invocation arguments and can close that active scope.
General conditional scopes, `continue`, and macros opening scopes that close in
later physical input remain later work. Macro-local `.shift` and `.return` are
supported. Remaining macro control-flow and the man/mdoc structural parsers
remain later work.
The public contract is frozen as a pre-0.1 migration baseline. The crate
remains unpublished while the final release and distribution checks complete.

The M1 arena decision is measured with:

```text
cargo test -p mantdoc arena_layout_microbenchmark --release -- --ignored --nocapture
```

The release-candidate parser throughput evidence uses the same generated
manuals as the M0 legacy benchmark:

```text
cargo bench --locked --package mantdoc --bench parse
```
