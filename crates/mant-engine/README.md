# mant-engine

`mant-engine` is `ManT`'s document execution layer. It resolves local documents
through `mant-sources`, lowers every source into the semantic center in
`mant-ir`, builds in-memory and versioned protocol projections, and produces
deterministic output without owning a terminal or command-line process.

## What this crate provides

- Registered Markdown lookup through the read-only `mant-sources` boundary.
- A conservative, source-positioned Markdown parser with explicit loss
  diagnostics and optional embedded tldr content.
- Bounded native manual loading, explicit leaf-file symlink support,
  root-constrained `.so` alias resolution, and `man(7)`/`mdoc(7)` lowering on
  every supported platform.
- Semantic outlines with compact scope summaries, role filters, nested entry
  paths, authored forms, value domains, and optional section/entry roots.
- Typed local and cross-document navigation: only logical document and manual
  links become bounded scope edges; external, email, and page-local targets do
  not expand a query.
- One selector resolver for outline roots, excerpts, and explanations: exact
  path, exact ID, exact alias, then normalized shorthand.
- Excerpt selection and literal or regular-expression search with generated
  Markdown coordinates.
- Markdown, text, man-style text, and JSON renderers over one normalized IR.
- Installed-client and private tldr cache discovery. Explicit subprocess-backed
  updates are available only with the opt-in `tldr-update` feature.

Process argument parsing, MCP transport, and interactive presentation remain
outside this crate.

The default feature set is read-only with respect to tldr data. The native
`mant` composition root enables `tldr-update`; library consumers, renderers,
and MCP-oriented embeddings do not receive subprocess update authority unless
they request it explicitly.

## Execution pipeline

```text
logical selector / physical input
              │
              v
DocumentResolver ──> Markdown parser or libmandoc lowering
              │
              v
      mant_ir::ResolvedContent
         ├─> DefinitionIdentity ─> SemanticIndex ─> outline / excerpt
         ├─> typed document graph ─> bounded scope search / explain
         ├─> Markdown / text / man-style renderers
         └─> versioned mant-protocol responses
```

| Need | Preferred API |
| --- | --- |
| Reuse one stable discovery snapshot | `DocumentResolver` |
| Resolve a complete typed request | `resolve_query_with_policy` |
| Resolve and project its requested view | `execute_query` |
| Resolve a bounded multi-document scope | `DocumentResolver::resolve_scope` |
| Resolve and project a scope request | `DocumentResolver::execute_scope_query` |
| Parse in-memory Markdown without discovery | `parse_markdown` or `query_markdown_text` |
| Parse in-memory roff without discovery | `parse_manual_bytes` or `query_roff_bytes` |
| Build a focused result from existing content | `build_outline_projection`, `select_excerpt`, `search_query` |
| Produce human or JSON output | The `render_*` functions |

## Basic use

The in-memory Markdown path is deterministic and works on every supported
platform:

```rust
use mant_protocol::EntryProjection;
use mant_engine::{
    build_outline_projection, query_markdown_text, render_outline_text,
};

let query = query_markdown_text(
    "# Demo\n\n## Options\n\n- `--verbose`: Show more detail.\n",
    Some("demo.md".to_owned()),
)?;
let outline = build_outline_projection(&query, EntryProjection::All, None)?;

println!("{}", render_outline_text(&outline));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `resolve_query` or `resolve_query_with_policy` when a caller needs the full
document bundle. Use `execute_query` to validate, resolve, and materialize the
request's `view` through one engine boundary. Use `parse_markdown` when the
caller needs the parsed document and tldr preface without query composition.
`DocumentResolver` can be reused when several operations must share one lazy
filesystem snapshot; constructing a new resolver refreshes discovery.

`resolve_scope` and `execute_scope_query` keep linked-document traversal,
aggregate content budgets, and breadth-first projections at that same engine
boundary. Process and MCP adapters should pass a `ScopeQueryRequest` rather
than reimplementing scope traversal.

Named resolution treats the full document and command quick reference as two
orthogonal facets. A manual section selects an exact native full document; it
does not by itself disable a compatible section `1` or `8` tldr attachment.
`QueryPolicy::ManualOnly` excludes that facet, while `TldrOnly` requests it
without requiring a full document. Dotted names are never split heuristically.

The engine returns `mant_ir::ResolvedContent` to direct semantic consumers and
creates `mant-protocol` projections for every structured host or process
boundary. A projection can stay in memory for a TUI callback or be serialized
for CLI JSON and compact MCP presentation. Serializing the IR directly is not a supported substitute
for those versioned DTOs.

## Platform behavior

| Platform | Markdown engine | Native man/mdoc engine |
| --- | --- | --- |
| Linux with glibc | Yes | Bundled `libmandoc-rs` |
| macOS | Yes | Bundled `libmandoc-rs` |
| Windows | Yes | Bundled `libmandoc-rs` |

Every supported target compiles `libmandoc-rs`. Windows uses its memory-only C
transport while Rust owns file I/O, decompression, paths, and `.so` redirects.
Native root discovery is also Rust-owned: Linux reads man-db mappings or
mandoc `man.conf`, macOS reads its PATH, active developer selection, and
`MANPATH`/`MANCONFIG` configuration, and Windows optionally reads `ManT`'s own
`man.conf`. The Windows subset supports direct and mandatory roots, bounded
one-level fragments, PATH-conditioned mappings, quoted paths, and single-pass
`%NAME%` expansion before automatically adding `%APPDATA%\ManT\man` and the
compatible `%USERPROFILE%\.local\share\man` fallback. Invalid directives are
omitted from queries and returned by `inspect_manual_roots` for local doctor
reporting. Windows environment names are matched case-insensitively, and the
fragment bound stops later pattern traversal, all without spawning a host
manual utility.

Native lowering conserves validated zero-width navigation targets as section,
semantic-entry, or inline identities. This includes targets libmandoc moves
onto structural paragraph, display, list, item, and function wrappers; it does
not synthesize visible placeholder text.

## Layering

`mant-engine` returns an owned `mant_ir::ResolvedContent` for direct semantic
use and owned `mant-protocol` values at versioned integration boundaries. It does not expose
libmandoc C structures. Applications that only need raw roff syntax should use
[`libmandoc-rs`](https://crates.io/crates/libmandoc-rs) directly. Applications
that need the complete command or reader should install
[`mant`](https://crates.io/crates/mant).

Architecture and source-resolution details are documented in the
[ManT native-engine reference](https://github.com/BryanHeBY/ManT/blob/main/docs/architecture/native-engine.md).
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0. Native builds also contain the separately attributed vendored mandoc
sources supplied by `libmandoc-rs`.
