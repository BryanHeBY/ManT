# mantdoc

`mantdoc` is `ManT`'s native Rust parser for roff, man, mdoc, tbl, and eqn
sources. It replaces `libmandoc-rs` without a C build script, FFI, native
library link, or unsafe code. See the repository
[migration guide](../../docs/mantdoc-migration.md) for its deliberate public
API differences from `libmandoc-rs`.

## Stability

`0.1.0` is the first public release. Its API is deliberately byte-first and
storage-independent. Compatible additions use `0.1.x`; a breaking public API
change will use the next pre-1.0 minor release.

- `mantdoc` is a parser and bounded reference renderer, not a general-purpose
  groff device implementation. Unsupported formatter/device behavior remains
  visible through typed diagnostics or normalized text where possible.
- Resource limits apply to source input, expansion, nesting, resolver access,
  diagnostics, renderer output, and source-map storage. A malformed or
  over-limit input never requires a host-global parser state.

The stable surface includes:

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

## Parsing model

Each parse owns its roff environment: strings, registers, copy-mode and nested
macros, delayed expansion, numeric/nroff conditionals, loops, translations,
and explicit `.so` resolution. Resolved sources execute in source order and
share bounded session state. The parser uses iterative collection and execution
stacks for untrusted nesting; `.break`, macro-local `.shift`, and `.return`
remain session-local.

The man and mdoc structural parsers, tbl/eqn preprocessors, and optional
renderers produce an immutable owned tree plus recoverable diagnostics. The
exact compatibility boundary is exercised against the checksum-pinned mandoc
1.14.6 regression corpus in repository-only tests; it does not redistribute
upstream source or renderer output.

The M1 arena decision is measured with:

```text
cargo test -p mantdoc arena_layout_microbenchmark --release -- --ignored --nocapture
```

The release-candidate parser throughput evidence uses the same generated
manuals as the M0 legacy benchmark:

```text
cargo bench --locked --package mantdoc --bench parse
```
