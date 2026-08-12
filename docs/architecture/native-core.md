# ManT architecture

ManT is one native documentation engine with three presentation boundaries:
an interactive TUI, deterministic command-line projections, and a read-only
MCP server. Native man/mdoc pages and Markdown enter the same renderer-neutral
document model before any interface sees them.

The architecture follows four constraints:

- parse each source once and share the resulting semantics;
- keep filesystem and process authority outside renderers;
- make every external machine contract explicit and versioned;
- keep ordinary reading local, bounded, and independent from host manual tools.

## Layer model

```text
mant              CLI, mode selection, source updates, request JSON, MCP
├─ mant-ui         Ratatui state machine and terminal presentation
└─ mant-core       resolution, parsing, lowering, projections, output
   ├─ mant-sources local Markdown registry and optional update machinery
   ├─ mant-ast     versioned renderer-neutral contracts
   └─ libmandoc-rs
      └─ vendored libmandoc + private C shim
```

The crates have deliberately asymmetric responsibilities:

| Crate | Owns | Does not own |
| --- | --- | --- |
| `mant-ast` | Document, query, catalog, outline, excerpt, search, and schema types | Parsing, files, rendering, or processes |
| `libmandoc-rs` | An owned libmandoc parse tree, diagnostics, parser lifecycle, and C build boundary | ManT types, source discovery, or output |
| `mant-sources` | Registered Markdown discovery and optional transactional Git/archive installation | Native manuals, rendering, or MCP |
| `mant-core` | Source resolution, Markdown parsing, libmandoc lowering, tldr composition, projections, and renderers | CLI policy, terminal lifecycle, or MCP transport |
| `mant-ui` | Interactive navigation, discovery, links, history, search, layout, and terminal lifecycle | Filesystem lookup or source mutation |
| `mant` | User-facing modes, terminal detection, source updates, request JSON, schemas, and MCP stdio | A second parser or frontend-specific document model |

Interactive queries pass an in-memory `QueryBundle` directly from `mant-core`
to `mant-ui`. They do not serialize through JSON or spawn a child process.
Explicit output, redirection, one-shot request JSON, and MCP use the same core
operations through their respective process boundaries.

## Shared document model

`mant.document/v7` is the source-neutral document contract. It contains root
content, recursive sections, blocks, inline nodes, layout hints, source
locations, diagnostics, and semantic definition identities. `mant.query/v7`
combines an optional document with an optional tldr quick reference while
preserving their different origins and licences.

The model carries intent that a renderer cannot safely recover from text:

- section and anchor IDs identify page-local destinations;
- definition identities group aliases and classify options, commands,
  variables, and environment variables;
- `document-reference` retains a hierarchical path and current Markdown
  source identity;
- `manual-reference` retains a manual name and optional section;
- external and email links remain distinct from local navigation.

The TUI activates these typed nodes and asks the `mant` host to resolve only
cross-document addresses. It never reconstructs a destination from a rendered
label or opens an arbitrary local path. Non-interactive renderers preserve the
visible link text even when their output medium cannot activate it.

Every external machine payload carries an exact schema discriminator. Rust
Serde types are the serialization authority and Schemars derives Draft
2020-12 schemas from them. The complete compatibility rules and field-level
contracts live in the [protocol reference](../protocol.md).

## Source resolution

A `DocumentResolver` creates one lazy snapshot of registered Markdown and the
native manual index. Reusing it keeps discovery and lookup consistent across a
TUI session; constructing a new resolver observes current local state.

Logical catalog addresses have three roots:

```text
documents/<path>
sources/<source>/<path>
manual/<section>/<name>
```

An unqualified selector checks personal Markdown, configured sources in
priority order, and then native manuals. Exact paths precede unique component
suffixes. Collisions remain explicit instead of choosing an arbitrary file.
Physical input is a separate request variant and never becomes a positional
logical selector.

### Native manual pipeline

One immutable `ManualIndex` owns both catalog discovery and exact lookup. It
derives roots from `MANT_MANPATH`, `MANPATH`, and platform conventions, then
indexes raw, gzip, and zstd files in traditional `man<section>/` directories
and flat roots. Unix adds user, PATH-derived, and system locations; Windows has
an intentionally small user fallback and otherwise uses configured roots.

Rust owns source I/O and decompression before plain roff bytes cross the parser
boundary. Reads are capped for stored and decoded data. Redirect-only `.so`
aliases are resolved against the indexed collection with canonical-path,
cycle, depth, and total-byte checks. Directory symlinks are not traversed. An
explicitly indexed leaf symlink may identify an external file, but every `.so`
target must remain inside the logical manual root. Direct `--input` pages have
no collection root and therefore reject redirect-only aliases.

`libmandoc-rs` wraps the bundled C parser behind a small private shim and
copies every completed parse into an owned Rust tree with structured
diagnostics. `mant-core` alone lowers that tree into `mant-ast`. Linux, macOS,
and Windows use the same parser version; Windows supplies bytes through a
checked memory-only configuration instead of exposing POSIX file transport to
C. ManT invokes libmandoc with native includes denied after Rust has resolved
the source chain.

Libmandoc 1.14.6 has process-global character, diagnostic, tag, and recursion
state, so parser sessions are serialized. Initialization happens once and each
request receives isolated diagnostic reset and capture.

### Markdown and installed sources

Local Markdown uses `pulldown-cmark` with source positions. ManT lowers a
deliberate structural subset—headings, prose, emphasis, code, links, code
blocks, lists, tables, hard breaks, and thematic breaks—into the shared AST.
Unsupported blocks retain their exact visible source with diagnostics instead
of disappearing. Recognized definition lists receive the same semantic
identities as native-manual definitions.

The registry preserves extension-free relative paths below personal
`documents/` and each installed source. Discovery is bounded, ignores
symlinks, and rejects case-only logical collisions for cross-platform safety.
Relative Markdown references are resolved lexically within their current
source and cannot escape into another source.

The default `mant-sources` feature set is read-only. Only the native `mant` CLI
enables update support: shallow Git acquisition and bounded HTTP archives feed
the same staging and atomic activation transaction. MCP sees installed local
state but has no update, network, or prune operation. Configuration, provider
metadata, precedence, and cleanup behavior live in the
[document-source guide](../sources.md).

### Quick references

Tldr content remains a distinct `QueryBundle` channel rather than a special
document section. Cached tldr-pages data and a Markdown-owned tldr preface use
the same parser and presentation model, with origin metadata controlling
attribution. Projection path `0` and selector `tldr` expose this channel
without renumbering ordinary document sections.

## Presentation boundaries

### Interactive TUI

`mant-ui` receives a complete typed bundle plus host callbacks for catalog
discovery, document loading, and safe external URI handling. The state machine
owns a hierarchical sidebar, a complete-catalog finder, page search, typed link
hit regions, and bounded back/forward history. Successful local jumps stay in
memory; cross-document requests return exact `DocumentAddress` values to the
host. Only HTTP(S) and `mailto` URIs may be delegated to the platform handler,
without invoking a shell.

### CLI and request JSON

Direct CLI projections and `--request-json` execute the same query engine.
Complete queries, outlines, excerpts, semantic explanations, searches, and
catalog results have independent versioned contracts. Human text and
CommonMark are renderers over those values rather than alternate parsers.

Search renders one canonical CommonMark projection, builds visible-text byte
mappings, and reports both a reusable semantic node and exact generated
Markdown coordinates. This keeps literal and regular-expression results
consistent across text and JSON presentation.

### MCP stdio

`mant --mcp` keeps standard output exclusively for JSON-RPC and exposes only
read-only local discovery and query tools. Tool schemas derive from the same
Rust contracts. MCP accepts logical document identities rather than arbitrary
host paths, never invokes Git or HTTP, and reads current local state on each
call. Concise structured results omit lowering diagnostics; ordinary CLI JSON
remains the diagnostic inspection surface.

## Layout ownership

Vertical spacing and filled inline flow are normalized before presentation.
Sections and blocks retain source-requested distance, explicit roff spacing
remains structured, list compactness stays on its owning list or item, and hard
breaks remain distinct from source wrapping. TUI, text, and CommonMark
renderers therefore adapt one layout model instead of reconstructing roff or
Markdown rules independently.

## Verification boundary

Rust tests are authoritative for parsing, lowering, contracts, source
resolution, rendering, terminal interaction, and process behavior. Checked-in
real roff fixtures cover multiple distributions and cross-platform toolchains;
tests do not depend on manuals installed on the CI host.

The shipped `docs/manuals/mant.md` is executable documentation: tests parse it
through the supported Markdown pipeline, require its quick reference and
semantic entries, and reject lossy fallbacks. Contract fixtures are decoded
and regenerated by Rust, while real-document TUI tests verify that substantial
source text, anchors, and typed links survive terminal lowering.

Repository layout, local commands, fixture policy, and CI responsibilities are
documented in the [development guide](../development.md).
