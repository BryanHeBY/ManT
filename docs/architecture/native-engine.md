# ManT native engine and crate boundaries

ManT is one native documentation engine with three presentation boundaries:
an interactive TUI, deterministic command-line projections, and a read-only
MCP server. Native man/mdoc pages and Markdown enter the same renderer-neutral
document model before any interface sees them.

The architecture follows four constraints:

- parse each source once and share the resulting semantics;
- keep filesystem and process authority outside renderers;
- make every structured host and process contract explicit and versioned, and
  keep agent presentation compact and bounded;
- keep ordinary reading local, bounded, and independent from host manual tools.

## Layer model

```text
source adapters
└─ mant-engine          resolution, lowering, composition, query, rendering
   ├─ mant-sources      local Markdown registry and optional updates
   ├─ libmandoc-rs     owned native parser boundary
   └─ mant-ir           semantic center: ResolvedContent and document IR
      ├───────────────> mant-ui and human renderers (direct semantic use)
      └─ projection ──> mant-protocol ──┬─> host callbacks and JSON
                                       └─> compact MCP presentation

mant                    composes modes, policies, updates, terminal, and stdio
```

That diagram describes data ownership. The compile-time workspace dependency
direction is related but not identical:

```text
mant
├── mant-ui ───────────────┬──> mant-ir
│                          └──> mant-protocol ──> mant-ir
├── mant-engine ───────────┬──> mant-ir
│                          ├──> mant-protocol
│                          ├──> mant-sources
│                          └──> libmandoc-rs
├── mant-sources (update feature)
├── mant-protocol
└── mant-ir
```

Arrows mean “depends on”; they do not imply serialization. `mant-ui`
deliberately depends on both the direct IR and the stable catalog DTOs it
exchanges with its host. `mant-engine` owns human
renderers as query operations, while `mant-ui` owns interactive terminal
presentation. The `mant` crate is the composition root and the only crate that
turns those components into the user-facing process.

The crates have deliberately asymmetric responsibilities:

| Crate | Owns | Does not own |
| --- | --- | --- |
| `mant-ir` | Logical document addresses; source-neutral document and quick-reference IR; typed node IDs and ranges; visitors and derived indexes | Versioned process envelopes, parsing, files, or rendering |
| `mant-protocol` | Shared query contracts, logical projections, versioned JSON DTOs, and deterministic compact presentation | Parsing, files, query execution, terminal policy, transports, or processes |
| `libmandoc-rs` | An owned libmandoc parse tree, diagnostics, parser lifecycle, and C build boundary | ManT types, source discovery, or output |
| `mant-sources` | Registered Markdown discovery and optional transactional Git/archive installation | Native manuals, rendering, or MCP |
| `mant-engine` | Source resolution, Markdown parsing, libmandoc lowering, tldr composition, projections, and renderers | CLI policy, terminal lifecycle, or MCP transport |
| `mant-ui` | Interactive navigation, discovery, links, history, search, layout, and terminal lifecycle | Filesystem lookup or source mutation |
| `mant` | User-facing modes, terminal detection, source updates, request JSON, schemas, and MCP stdio | A second parser or frontend-specific document model |

`mant-ir` is deliberately the semantic center, while `mant-engine` is the
execution layer that creates and operates on it. Interactive queries pass an
in-memory `ResolvedContent` directly to `mant-ui`; human renderers also consume
the in-memory model. They do not serialize through JSON or spawn a child
process. Structured host and process interactions instead use
`mant-protocol`: the TUI catalog callbacks, one-shot request JSON, CLI query
JSON, JSON Schema, and MCP all share its logical identities and projections.
CLI and request boundaries serialize versioned envelopes; MCP renders focused
projections as bounded text or CommonMark rather than exposing the complete
AST.
Source update and prune commands use their own schema-marked maintenance
reports owned by `mant-sources`; they do not become document protocol variants.

Multi-document operations use `mant-protocol::DocumentScope` as a host-neutral input. `mant-engine::DocumentResolver` resolves its ordered roots, follows only typed IR `Document` and `Manual` edges, and returns one bounded breadth-first graph plus the loaded documents in matching order. Search, explanation, CLI JSON, and interactive search consume that same scope; they do not infer families from filename prefixes or merge independent document trees into one AST. The TUI receives the already loaded scope in memory, while its ordinary catalog finder remains a separate global host callback.

This separation also explains why `mant-protocol` depends on `mant-ir`: its
DTOs project selected semantic types and provide explicit conversions,
but the IR never depends on a protocol version. Dependency direction therefore
keeps the semantic model usable without process framing.

## Shared document model

`mant-ir::Document` is the source-neutral in-memory representation. It contains
root content, recursive sections, blocks, inline nodes, layout hints, source
locations, diagnostics, and semantic definition identities. Node identities
use `NodeId`; exact Markdown coordinates use half-open UTF-8 `TextRange`
values; `DocumentIndex` provides a derived lookup sidecar without embedding
mutable caches in the tree. Syn-style `Visit` and `VisitMut` traits keep
cross-cutting passes exhaustive as the IR evolves.

At a structured integration boundary, `mant-protocol::DocumentResponse` adds
the exact `mant.document/v0.8` discriminator and producer metadata. `mant.query/v0.8`
combines an optional document response with an optional tldr quick reference
while preserving their different origins and licences.

The model carries intent that a renderer cannot safely recover from text:

- section and anchor IDs identify page-local destinations;
- definition identities group aliases and classify options, commands,
  variables, and environment variables;
- `Inline::Link` has an explicit target kind for a hierarchical Markdown
  document, installed manual, page-local section, external URI, or email;
- `OutlinePath` validates internal one-based outline addresses; its protocol
  projection is the serialized `NodePath`, distinct from filesystem paths and
  labels;
- external and email links remain distinct from local navigation without
  multiplying overlapping inline node variants.

The TUI activates these typed nodes and asks the `mant` host to resolve only
cross-document addresses. It never reconstructs a destination from a rendered
label or opens an arbitrary local path. Non-interactive renderers preserve the
visible link text even when their output medium cannot activate it.

Every versioned document-query payload carries an exact schema discriminator.
Rust Serde types are the serialization authority and Schemars derives Draft
2020-12 schemas from them. IR types reused by protocol projections are treated
as a wire-bearing semantic subset: CI snapshots their complete structural
schemas and rejects representation drift under an unchanged discriminator.
The complete compatibility rules and field-level contracts live in the
[protocol reference](../manuals/mant-protocol.md).

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

An unqualified selector checks personal Markdown first. Configured sources then
sort around native manuals at priority zero: positive sources precede manuals,
manuals precede zero and negative sources, and sources are ordered by descending
priority and ascending name. Exact paths precede unique component suffixes.
Collisions remain explicit instead of choosing an arbitrary file.
Physical input is a separate request variant and never becomes a positional
logical selector.

### Native manual pipeline

One immutable `ManualIndex` owns both catalog discovery and exact lookup. It
derives roots from `MANT_MANPATH`, `MANPATH`, and platform conventions, then
indexes raw, gzip, and zstd files in traditional `man<section>/` directories
and flat roots. Its dedicated path layer reads Linux man-db maps and mandatory
roots (or mandoc `man.conf`), macOS PATH, active-developer, `MANPATH`, and
`MANCONFIG` sources, and an optional ManT-owned Windows `man.conf`; only an
unavailable host configuration falls back to user, PATH-derived, and
conventional system locations. No lookup spawns the host `man`, `manpath`, or
`xcode-select` program.

Rust owns source I/O and decompression before plain roff bytes cross the parser
boundary. Reads are capped for stored and decoded data. Redirect-only `.so`
aliases are resolved against the indexed collection with canonical-path,
cycle, depth, and total-byte checks. Directory symlinks are not traversed. An
explicitly indexed leaf symlink may identify an external file, but every `.so`
target must remain inside the logical manual root. Direct `--input` pages have
no collection root and therefore reject redirect-only aliases.

`libmandoc-rs` wraps the bundled C parser behind a small private shim and
copies every completed parse into an owned Rust tree with structured
diagnostics. `mant-engine` alone lowers that tree into `mant-ir`. Linux, macOS,
and Windows use the same parser version; Windows supplies bytes through a
checked memory-only configuration instead of exposing POSIX file transport to
C. ManT invokes libmandoc with native includes denied after Rust has resolved
the source chain.

The pinned libmandoc 1.14.6 snapshot originally kept character, diagnostic,
tag, roff-request, and recursion state in process globals. The local vendor
patch makes those parser-session slots thread-local, and the shim keeps source
roots and diagnostic capture thread-local as well. Independent parser calls
therefore run concurrently without a process-wide lock; one-time native
initialization remains synchronized. Date conversion avoids process-global
timezone mutation and uses reentrant platform APIs where local time is
required. Recursive re-entry on one thread is not supported, and the owned
node and equation copies stop after 256 levels so hostile nesting cannot carry
an unbounded C tree into recursive Rust consumers. A mixed Rust/C
ThreadSanitizer runner guards this boundary locally because instrumenting only
Rust would miss races inside the vendored parser.

### Markdown and installed sources

Local Markdown uses `pulldown-cmark` with source positions. ManT lowers a
deliberate structural subset—headings, prose, emphasis, code, links, code
blocks, lists, tables, hard breaks, and thematic breaks—into the shared IR.
Unsupported blocks retain their exact visible source with diagnostics instead
of disappearing. Recognized definition lists receive the same semantic
identities as native-manual definitions.

The registry preserves extension-free relative paths below personal
`documents/` and each installed source. Discovery is bounded and rejects
case-only logical collisions for cross-platform safety. Personal documents may
use explicit leaf-file links to regular files, including external targets;
directory and broken links are ignored. Managed caches never follow links, and
source acquisition rejects selected Git or archive links before activation.
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
without renumbering ordinary document sections. Explicit tldr resolution uses
the registered-document precedence groups, filters each group by actual
embedded content, and places the cache at the shared built-in priority-zero
boundary. This keeps source ranking in `mant-sources` and content policy in the
engine instead of duplicating either rule in the CLI.

Named lookup plans the full-document source and quick-reference policy as
orthogonal decisions. An explicit manual section fixes the full document to one
native category but still permits a section `1` or `8` quick reference under
the combined policy. Manual-only excludes it, and tldr-only omits the full
document. The CLI recognizes only explicit section forms; dotted logical names
are never reinterpreted after a failed lookup.

The engine's default tldr feature set is read-only. Subprocess-backed updates
through an installed tldr client or Git are compiled only by the opt-in
`tldr-update` feature, which the native `mant` composition root enables. This
matches `mant-sources`: reusable engine consumers receive local discovery and
parsing without implicit update authority, while MCP exposes no update entry
point even in the full executable.

## Presentation boundaries

### Interactive TUI

`mant-ui` receives a complete typed bundle plus host callbacks for catalog
discovery, document loading, and safe external URI handling. The state machine
owns a hierarchical Outline tree, a complete-catalog finder, page search, typed link
hit regions, and bounded back/forward history. Successful local jumps stay in
memory; cross-document requests return exact `DocumentAddress` values to the
host. Only HTTP(S) and `mailto` URIs may be delegated to the platform handler,
without invoking a shell.

### CLI and request JSON

Direct CLI projections and `--request-json` execute the same query engine.
Complete queries, outlines, excerpts, semantic explanations, searches, and
catalog results have independent versioned contracts. Human text and
CommonMark are renderers over those values rather than alternate parsers.

Search renders one canonical CommonMark projection together with structured
semantic node ranges. A visible-text map then preserves exact generated
Markdown coordinates without rediscovering owners from rendered anchors. This
keeps literal and regular-expression results consistent across text and JSON
presentation.

### MCP stdio

`mant --mcp` keeps standard output exclusively for JSON-RPC and exposes only
five read-only local discovery and query tools. Closed input schemas derive
from Rust parameter types. MCP accepts logical document identities rather than
arbitrary host paths, never invokes Git or HTTP, and reads current local state
on each call. Successful results contain one bounded text block with
Unicode-scalar page metadata and no server-side paging state; they omit ASTs,
schema metadata, physical paths, and ordinary lowering diagnostics. CLI JSON
remains the structured diagnostic inspection surface.

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
