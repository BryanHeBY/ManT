# mant-engine

`mant-engine` is `ManT`'s document execution layer. It resolves local documents
through `mant-loader`, which delegates decoding and lowering to `mant-codec` into
the semantic center in `mant-ir`. It delegates bounded content queries to
`mant-query`, composes versioned protocol responses, and produces
deterministic output without owning a terminal or command-line process.

## What this crate provides

- Registered Markdown lookup through the read-only `mant-loader` / `mant-sources` boundary.
- Integration with `mant-codec`'s source-positioned Markdown parser, explicit
  loss diagnostics and optional embedded tldr content.
- Source-neutral inline headings: Markdown ATX/Setext and native section
  headings retain styles and typed links. An extracted first H1 is real
  document-heading content, not duplicated metadata or a synthetic paragraph.
- Integration with the loader's bounded native input policy: explicit leaf-file symlink support,
  root-constrained `.so` alias resolution, and delegated `man(7)`/`mdoc(7)`
  lowering through `mant-codec` on every supported platform.
- Loader-owned regular-file-only manual-path configuration reads. Unix nonblocking
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
- One strict selector resolver for outline roots and excerpts: explicit local
  `ContentSelector::Path` or `ContentSelector::Id`, without names, aliases,
  shorthand, fragment activation or document-link following.
- Independent bounded explanation evidence: exact names/forms, literal content
  and validated alias relationships. Multiple owners and no-evidence are normal;
  ordinary support retains its section and block path without becoming an entry.
  Consecutive native declaration groups additionally supply complete reading
  context for empty heads without moving another item's body or inferring
  aliases. Context is carried in the returned DTO, not recovered by a renderer.
- Excerpt selection and literal or regular-expression search with generated
  Markdown coordinates.
- Markdown, text, man-style text, and JSON renderers over one normalized IR.
  Heading Markdown preserves manual targets as explicit `man:topic(section)`
  (or unqualified `man:topic`) links; document, external and email targets
  remain ordinary Markdown links. A local link in any real heading automatically
  selects addressable output, including its destinations; incompatible semantic
  comments are omitted even when requested. Otherwise `preserve_anchors` remains
  opt-in. Root destinations precede the actual document heading, not its body
  or quick reference. An excerpt preserves heading links but may exclude their
  target content; it does not follow them. This does not change
  the historical portable body policy of showing native manual-reference labels.
  Raw anchor destinations support HTML navigation; they do not make addressable
  Markdown a lossless semantic-IR reimport format. Use IR JSON for that contract.
  Multiline level-one/two headings use Setext syntax. At deeper levels portable
  ATX output folds explicit breaks to spaces while keeping the heading level,
  visible words and links; IR JSON retains the exact break structure.
- Source-aware `render_query_text_with` / `render_excerpt_text_with` callbacks
  over the same plain-text block layout, with composable source markup and
  validated owner-local name roles rather than rendered-line name matching.
- Loader-owned installed-client and private tldr cache discovery. Explicit subprocess-backed
  updates are available only with the opt-in `tldr-update` feature.

For already-loaded tldr text, `mant-codec::parse_tldr_page` and
`mant-codec::parse_tldr_command` are pure parsing entry points, also re-exported
here without `tldr-update`. `TldrPageLocation` supplies
identity metadata only: it does not trigger a cache read, host/platform lookup,
or URL fetch. `TldrParseError` reports syntax failure; the separate
`TldrCacheError` adds read/location context, and `TldrUpdateError` belongs to the
explicit maintenance operation.

Process argument parsing, MCP transport, and interactive presentation remain
outside this crate.

The default `roff` feature preserves native-manual support and explicitly
forwards to `mant-loader/roff` and `mant-codec/roff`. Disable default features
for Markdown/tldr-only production use without native parsing or decompression.
The default feature set remains read-only with respect to tldr data. The native
`mant` composition root enables `tldr-update`; library consumers, renderers,
and MCP-oriented embeddings do not receive subprocess update authority unless
they request it explicitly.

## Execution pipeline

```text
logical selector / physical input
              │
              v
DocumentResolver ──> mant-loader ──> mant-codec (Markdown / tldr / roff)
              │
              v
      mant_ir::ResolvedContent
         ├─> mant-query ─> outline / excerpt / search / explain / references
         ├─> typed document graph ─> mant-query borrowed scope
         ├─> Markdown / text / man-style renderers
         └─> versioned mant-protocol responses
```

Lowering preserves ordinary lists as ordinary lists and genuine definitions as
terms plus descriptions. Both can carry `mant_ir::EntryFacts`; annotation never
replaces their blocks. Markdown binds declarations and coverage to original
item positions before consuming comments, then binds forms to final content.
Native lowering records explicit term forms while styled macro evidence is
still available. Shared validation independently filters invalid forms, names
and relationships without erasing owners or their children. Content selectors
reuse a single immutable location snapshot; semantic names belong to independent
explanation evidence. Compact outline summaries count borrowed facts and validated
form bindings without constructing a full semantic index or copying form text.
Reference inventories independently scan the exact original owner, regardless of
entry display filters. Search composes byte ownership
with rendered text even when table cells flatten for portable Markdown.

### Native source interpretation

`mant-codec` owns native lowering, Markdown parsing, semantic annotation and
portable document encoding. Its [native ownership map](https://github.com/BryanHeBY/ManT/blob/dev/crates/mant-codec/README.md#native-lowering-ownership)
documents formatter state, source geometry, target retention and transactional
table recovery. `mant-loader` prepares inputs; the engine composes queries over
that one implementation through `mant-query`; it does not reinterpret source
macros, rebuild entry facts, or duplicate selection and evidence algorithms.

### Public entry points

| Need | Preferred API |
| --- | --- |
| Reuse a read-only discovery/loading snapshot | `mant_loader::DocumentLoader` |
| Reuse an application loading/query snapshot | `DocumentResolver` |
| Load a borrowed source specification without a query view | `mant_loader::DocumentLoader::load`, `mant_loader::LoadSpec`, `mant_loader::LoadPolicy` |
| Resolve a complete typed request | `resolve_query_with_policy` |
| Resolve and project its requested view | `execute_query` |
| Resolve a bounded multi-document scope without querying | `mant_loader::DocumentLoader::resolve_scope` |
| Resolve and project a scope request | `execute_scope_query` or `DocumentResolver::execute_scope_query` |
| Query caller-owned document snapshots without loading | `mant_query::QueryScopeView::new`, `mant_query::search_scope`, `mant_query::explain_scope` |
| Parse in-memory Markdown without query composition | `mant_codec::parse_markdown` (also re-exported here) |
| Compose a query from in-memory Markdown | `query_markdown_text` |
| Parse prepared plain roff without loading or decompression | `mant_codec::parse_roff_bytes` (`roff` feature) |
| Apply standalone-input policy to prepared plain roff bytes | `mant_loader::parse_manual_bytes` or engine `query_roff_bytes` (`roff`) |
| Audit production file lowering against its exact native witness | `mant_loader::parse_manual_source_with_report` (`roff`) |
| Build a focused result from existing content | `mant_query::build_outline_projection`, `mant_query::select_excerpt`, `mant_query::search_query` |
| Collect bounded independent semantic evidence | `mant_query::explain_query`, `mant_query::validate_explanation_query` |
| Produce human or JSON output | The `render_*` functions |

## Basic use

The in-memory Markdown path is deterministic and works on every supported
platform:

```rust
use mant_protocol::EntryProjection;
use mant_engine::{query_markdown_text, render_outline_text};
use mant_query::build_outline_projection;

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
caller needs the parsed document and tldr preface without query composition;
that function is implemented and exported by `mant-codec`.
`DocumentResolver` can be reused when several operations must share one lazy
filesystem snapshot; constructing a new resolver refreshes discovery.

Loading and view validation have distinct error owners. `mant_loader::DocumentLoader`
accepts a borrowed `LoadSpec` and `LoadPolicy`, never a serialized request or query
view; failures are `LoadError`. The application `DocumentResolver` validates a
complete request and joins loading with query execution. `QueryError::Load` and
`QueryError::QueryValidation` preserve the originating category and error chain
without giving acquisition code access to search or projection behavior.

The same split applies to scopes: `ScopeLoadError` describes traversal inputs
or acquisition failures, while `ScopeQueryError::Execution` retains failures
from querying the already-loaded collection. One-shot request functions reject
invalid inputs and views before constructing a system resolver. Explicit
resolver methods reuse their caller-owned snapshot; the validated execution
path does not repeat the application validation merely because a snapshot was
created by a convenience function.

`mant_loader::DocumentLoader::resolve_scope` owns linked-document traversal and
aggregate content budgets. Engine `execute_scope_query` joins that acquisition
with breadth-first query results at one application boundary. Process and MCP adapters should pass a `ScopeQueryRequest` rather
than reimplementing scope traversal.

For already-loaded or caller-produced content, construct `QueryScopeView` from
the borrowed logical graph and the matching `ResolvedContent` slice, then use
`search_scope` or `explain_scope`. The view checks lengths, exact addresses,
unique source slots, BFS depth/order and graph provenance before execution;
it cannot silently truncate mismatched collections. It borrows the original
content and permits repeated queries without loading, parsing or serialization.
The caller remains responsible for supplying one coherent provenance snapshot,
not merely equal addresses from unrelated revisions. Loading coverage and
frontier records remain accessible through the same borrowed graph; an absent
target is not proof of nonexistence. Query errors are `ScopeExecutionError`,
separate from source acquisition errors.

`LoadedDocumentScope` exposes immutable `scope()` and `documents()` accessors;
`into_parts()` transfers both owned components without cloning content. The
engine's complete workflow constructs the validated view from this loading
result and retains global classification, paging and shared copy budgets.

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

`render_explanation_text_with` and `render_scope_explanation_text_with` expose
the same report as plain text, with terminal-neutral `TextPresentation`
spans. Their callbacks preserve visible text and boundary whitespace. Exact
matches and ordinary name styling resolve only against returned forms/content;
deserialized responses need no original document or query-side table. The
plain report does not prefix original lines. Complete displayed declaration
owners suppress duplicate Forms per record. Recovered group context is displayed
once with its provider; only truly isolated empty definitions get an empty-body
notice, distinct from copy/support omission. Shared body references map positions
back to each actual member. This does not change the DTO. The
corresponding Markdown renderers quote source lines, escape metadata, and retain
verbatim fenced code. Outline text likewise has one plain/decorated tree through
`render_outline_text_with`, with complete IDs on hanging metadata lines.

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
`mant_loader::LoadPolicy::ManualOnly` excludes that facet, while `TldrOnly` requests it
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

With the default `roff` feature, every supported target compiles `libmandoc-rs`. Windows uses its memory-only C
transport while Rust owns file I/O, decompression, paths, and `.so` redirects.
Within `mant-loader`, `manual_input` owns that product input policy and its
`ManualError` failures. `mant-codec` accepts prepared plain bytes and a
source label, with includes denied; it never opens that label or a redirect.
Standalone alias syntax is recognized once by a pure codec helper, while only
the indexed loader may resolve it. Stored and decoded bytes each share a
16 MiB budget across the complete chain, which permits at most 16 redirects.
The report-bearing file API lowers the same owned native parse witness rather
than reopening or reparsing the input. Indexed alias metadata is attached by
the loader after parsing, not used to authorize codec IO.
Native root discovery is also loader-owned Rust code: Linux reads man-db mappings or
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
not synthesize visible placeholder text. This policy is implemented once in
`mant-codec`, not in an engine-specific parser.

## Layering

`mant-engine` returns an owned `mant_ir::ResolvedContent` for direct semantic
use and owned `mant-protocol` values at versioned integration boundaries. It does not expose
libmandoc C structures. It owns application request composition and report
rendering; it is not merely a forwarding facade. Pure query execution belongs
to `mant-query`. Parser/encoder, loader and query re-exports are transitional
conveniences, not duplicate implementations.
Consumers needing only source-to-IR conversion or portable document Markdown
should depend on `mant-codec` directly; consumers needing source discovery,
loading, read-only caches or owned scopes should use `mant-loader`. Consumers
with existing IR that need selection, search, explanation or reference
projections should use `mant-query`, which does not load files or render reports.
The engine's opt-in tldr maintenance implementation remains separate from
loader authority. Applications that only
need raw roff syntax should use
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
