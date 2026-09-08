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
- Shared regular-file-only manual-path configuration reads. Unix nonblocking
  opens and handle checks reject FIFOs without waiting for a writer, while
  retaining symlinks to regular configuration files and bounded UTF-8 reads.
- Semantic outlines with compact scope summaries, role filters, nested entry
  paths, authored forms, value domains, and optional section/entry roots.
- Markdown entry authoring with grouped visible names, independent invocation
  forms, linked code terms, and explicitly open or exhaustive local choices.
  Item-owned `mant:entry` comments can declare exact IDs, disjoint visible-name
  alias groups, and validated same-document alias relationships without
  replacing content or inferring equivalence from punctuation.
  See the [authoring field map](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-markdown.md#authoring-to-entry-field-map)
  for declared fields versus index-derived facts.
  Grouping names supplies shared selectable content, not proof of behavioral
  equivalence. Semantic facts bind to the original list content without
  replacing its punctuation, paragraphs, numbering or tightness.
- Typed local and cross-document navigation: only logical document and manual
  links become bounded scope edges; external, email, and page-local targets do
  not expand a query.
- One strict selector resolver for outline roots and excerpts: exact
  path, exact ID, exact alias, then normalized shorthand.
- Independent bounded explanation evidence: exact names/forms, literal content
  and validated alias relationships. Multiple owners and no-evidence are normal;
  ordinary support retains its section and block path without becoming an entry.
- Excerpt selection and literal or regular-expression search with generated
  Markdown coordinates.
- Markdown, text, man-style text, and JSON renderers over one normalized IR.
- Source-aware `render_query_text_with` / `render_excerpt_text_with` callbacks
  over the same plain-text block layout, with composable source markup and
  validated owner-local name roles rather than rendered-line name matching.
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
         ├─> ListItem / DefinitionItem.entry ─> SemanticIndex ─> outline / excerpt
         ├─> typed document graph ─> bounded scope search / explain
         ├─> Markdown / text / man-style renderers
         └─> versioned mant-protocol responses
```

Lowering preserves ordinary lists as ordinary lists and genuine definitions as
terms plus descriptions. Both can carry `mant_ir::EntryFacts`; annotation never
replaces their blocks. Markdown binds declarations and coverage to original
item positions before consuming comments, then binds forms to final content.
Native lowering records explicit term forms while styled macro evidence is
still available. Shared validation independently filters invalid forms, names
and relationships without erasing owners or their children. Derived selectors
reuse a single immutable location snapshot, and search composes byte ownership
with rendered text even when table cells flatten for portable Markdown.

### Native lowering ownership

Native lowering is private implementation, not a second public document API.
The following boundaries keep source interpretation shared across prose,
literal displays, lists and table recovery:

| Responsibility | Owner and lifetime |
| --- | --- |
| Stage composition | `mandoc/mod.rs` resolves the parsed document and runs lowering, navigation and validation. |
| Source lookup | `source_context.rs`, `ast.rs` and `equations.rs` provide source/AST services and bounded operation-local memoization; diagnostics use their own collector. They do not store a hidden formatter register. |
| Formatter state | `FormatterState` carries current font, previous font and spacing explicitly between consumers. A normal font-scope exit restores current font but retains previous-font effects. |
| Container routing | `containers.rs` streams borrowed children and scope boundaries; structural payloads remain tables/lists. Logical punctuation adjacency is a separate, non-executing classification in `adjacency.rs`. |
| Inline and physical lines | `InlineBuilder` executes word/control events; `source_cursor.rs` places source-visible events on physical lines. No-fill changes layout, not macro interpretation. |
| Structural layout | Block drivers own pending paragraphs and list state; section, synopsis, man no-fill and dialect-specific list consumers remain separate. Shared definition helpers do not own formatter state. |
| Speculative recovery | Table-cell candidates retain output, final formatter state and diagnostics until ownership acceptance. Rejection rolls back all three, unlike a normal font-scope exit. |

Complete corpus regressions live in the repository integration tests, outside
the published `src/**` source set. Packaged unit tests remain self-contained.
Target and topology audits complement these exact text/font/line assertions;
a clean target ledger alone does not establish rendering fidelity.

### Public entry points

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
| Collect bounded independent semantic evidence | `explain_query`, `validate_explanation_query` |
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

`explain_query` returns `QueryExplanation`, not a unique excerpt. Its options
bound returned owners (default 50, maximum 256), a zero-based offset, and copied
forms/facts/previews/body payload (default 1 MiB, maximum 4 MiB). A borrowed
collection plan orders direct entries, explicit relations, entry mentions and
context mentions before global pagination. Within each class, scope preserves
BFS document and original IR order, sharing one budget across selected records.
A bounded priority pool prevents early mentions from excluding later direct
entries, reporting any discarded candidates. Scope has one flat evidence page
and source reports, never duplicate nested bodies. `select_explanation` uses the defaults;
`select_excerpt` is the strict navigation API. Check `outcome`, scope coverage
and `truncation` separately; `semanticsComplete` describes semantic validation,
not exhaustive recall. Literal previews retain up to two original matched
blocks, each a maximum 1024 Unicode scalars with exact match ranges and actual
source positions. `resolve_explanation_block` resolves preview paths in final
IR. Compact rendering shows those windows for mentions instead of unrelated
full owner content. Clipping, omitted previews, and atomic body omission are
distinct. No evidence query performs I/O or executes examples.

```rust
let query = mant_engine::query_markdown_text("# Demo\n\n- `--help`: Usage.\n", None)?;
let evidence = mant_engine::select_explanation(&query, "--help")?;
assert_eq!(evidence.outcome, mant_protocol::ExplanationOutcome::Evidence);
assert!(evidence.evidence.iter().any(|owner| owner.bases.iter().any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Name { .. }))));
# Ok::<(), Box<dyn std::error::Error>>(())
```

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
