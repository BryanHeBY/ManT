# Native document core

Status: implemented.

## Context

Earlier prototypes reconstructed a document from renderer HTML and maintained
parallel parser and presentation models. Those representations were removed
after the native document contract became authoritative. ManT now has one
renderer-neutral model, and Rust is the sole owner of document interpretation.

## Decision

ManT uses one Rust implementation with six layers:

```text
mant-ast          versioned document and query contracts
libmandoc-rs      owned libmandoc parse tree, private C shim, and build logic
mant-sources      local Markdown registry; optional Git and archive updates
mant-core         document loading, parsing, query, and output renderers
mant-ui           Ratatui presentation over an in-memory query bundle
mant              mode selection, CLI, MCP, and versioned stdio boundary
```

The `mant` executable selects interactive or output presentation after parsing
arguments. A complete query on terminal stdin and stdout is handed directly to
`mant-ui`; projections, explicit formats, redirection, request JSON, and MCP
remain deterministic non-interactive surfaces.

On Linux, macOS, and Windows, `libmandoc-rs` is the boundary around the bundled
C parser. Its deliberately small private C shim hides libmandoc structure layouts and parser handles; the
crate copies each completed parse into an owned, renderer-neutral tree with
structured diagnostics. The crate exposes no ManT-specific types and never
formats JSON. `mant-core` alone lowers that parse tree into ManT's public
document contract. Windows builds a checked memory-only parser configuration;
Rust supplies bytes without exposing POSIX file transport to the C layer.

## Stable and unstable models

`mant.document/v6` is the stable structured-document contract consumed by the
UI and output renderers. `mant.query/v6` combines an optional document with an
optional tldr document while preserving their different sources and licences.
The document source is man, mdoc, or Markdown. Blocks before the first heading
live in the document's root `blocks`; heading content remains in the recursive
section tree.

All external machine payloads carry an exact schema identifier. Rust structs
are the source of truth. New optional object fields may be added within a
schema version; incompatible meaning changes and new required node variants
require a new schema version. The in-process Rust UI consumes the same typed
model directly and does not serialize it first.

The initial document contract keeps navigation semantic instead of encoding
every destination as an untyped URI:

- `external-link` stores an external `uri` and its rendered label;
- `email-link` stores an address without a synthetic `mailto:` prefix;
- `manual-reference` identifies another manual by name and optional section;
- `section-reference` targets a document-local section ID;
- `anchor` marks a zero-width document-local destination such as mdoc `Tg`.

Definition-list entries may also carry a semantic `identity` with a stable
document-local ID, a role, a name-matching case policy, and normalized names. The current document
contract assigns this identity to recognized command-line options, commands,
variables, and environment variables. It preserves the complete
rendered term and description while making aliases such as `-g` and
`--listed-incremental` discoverable as one addressable entry.

Section IDs and explicit anchor IDs occupy the same namespace within one
document. Renderers may style or activate these nodes differently, but must
preserve their visible children in non-interactive output.

The TUI activates resolved `section-reference` nodes directly: clicking one
places the target heading at the top of the content viewport, selects it in
the sidebar, and expands hidden ancestors. This is deliberately a stateless
page-local jump; navigation history is not part of the current interaction
contract.

## Native process boundary

The project deliberately exposes process protocols instead of Node-API. This
avoids ABI-specific addons and makes the same binary directly useful to any
language. One-shot requests serve external integrations; the same executable
also provides a long-lived, read-only MCP stdio server. The native reader does
not pay this boundary cost because it calls `mant-core` in process.
The field-level contract, version matrix, examples, and client checklist live
in the [JSON protocol and Schema reference](../protocol.md).
The public surface is use-case oriented rather than a mirror of parser
internals:

```text
mant <name> [--format <format>]   -> query Markdown, text, or JSON
mant <path.md> [--format <format>] -> query one local Markdown file
mant -                             -> read Markdown directly from stdin
mant <name> --outline [sections|entries] -> selectable section and semantic-entry tree
mant <name> --node <path-or-id>   -> selected section subtrees
mant <name> --explain <alias-or-id> -> one option, command, variable, or environment entry
mant <name> --search <pattern>    -> matches with node and Markdown locations
mant <name> --manual              -> bypass registered Markdown by the same name
mant <name> --source <source>     -> select one configured Markdown source
mant --update-docs                -> Git/archive source update report JSON
mant --prune-docs [--dry-run]     -> explicit orphaned-source cleanup report JSON
mant --update-tldr                 -> update result JSON
mant --protocol-version            -> protocol description JSON
mant --schema <contract>           -> generated JSON Schema
mant --mcp                         -> read-only MCP tools over stdio
```

For process integrations, `mant --request-json --format json --compact` reads
one closed, versioned `QueryRequest` object from standard input and emits
exactly one versioned projection on standard output: `mant.query/v6`,
`mant.outline/v6`, `mant.excerpt/v6`, or `mant.search/v6`, according to the
requested view. Standard error contains concise diagnostics only. Status 0
means success, 2 means invalid invocation or request, and 1 means an
operational failure. Interactive search and navigation operate on the already
loaded in-memory document and never spawn additional processes.

For agent clients that speak the Model Context Protocol, `mant --mcp`
keeps standard output exclusively for JSON-RPC and exposes five generated,
read-only tools: local-document discovery, outline, selected content,
semantic explanation, and search. Document tools accept a name plus an
optional configured source or manual section. That narrower boundary prevents
agents from opening arbitrary host paths: Markdown must first be placed in the
flat root directory or installed by the native CLI. MCP only reads current
local state through `mant-sources` without its update feature and never invokes
Git or HTTP. Ordinary names continue to fall back to the
native manual index. Input and output schemas derive directly from Rust types.
MCP drops lowering diagnostics and keeps standard error silent; the ordinary
CLI and request JSON surfaces remain the diagnostic inspection paths. An MCP
outline retains only the compact `entriesComplete: false` signal when rejected
declarations make semantic discovery incomplete. MCP is an alternate process
protocol; it does not add another executable or a second document
interpretation path.

`mant.request/v6` requires a `schema` marker, one closed `input`, and one
closed `view`. The input is either a document name with an optional source or manual section or a
local Markdown path; raw document content is deliberately not part of the
process contract. Direct `mant -` reads bounded UTF-8 input before constructing
an in-memory query and does not add a third public input variant. Unknown
fields are rejected at every level. `full` returns `mant.query/v6`, `outline`
selects either section-only or entry-aware structure, `excerpt` selects one or
more node paths, IDs, or aliases, `explain` resolves exactly one semantic
entry, and `search` returns `mant.search/v6`. Both `--explain` and the request
view use the same entry-only projection and return `mant.excerpt/v6`, avoiding
a separate explanation response shape.
`mant --schema request` exposes that exact input contract; `query`,
`outline`, `excerpt`, and `search` expose the output contracts, while `all`
returns a named catalog. The schemas are derived with
Schemars from `mant-ast`'s Serde types, explicitly pinned to JSON Schema Draft
2020-12, and generated separately for deserialize and serialize behavior.

Interactive and machine-oriented use are modes of one installed `mant`
executable. Local development runs it directly through Cargo; release builds
produce `target/release/mant`. There is no companion command lookup, private
executable extraction, or runtime outside the native process.

A complete query attached to a terminal opens the TUI; a non-interactive
complete query defaults to Markdown. Outline and search projections default to
text. `--format json` is pretty by default and `--compact` is available to
process clients. Fatal native failures cross the boundary as concise errors;
recoverable parser findings are structured diagnostics in the complete query
result.

Outline and excerpt views are projections of the same complete native
document, so they never reimplement parsing rules. Outlines expose both a
one-based tree path such as `4.2` and the document-local section ID. Passing
`--outline` includes semantic option, command, variable, and environment entries with
paths such as `4.2/o3` by default. `--outline sections` is the explicit compact
view for callers that only need section topology.
Markdown content before the first heading is exposed as path `root` with ID
`document-overview`; it does not consume or renumber ordinary heading paths.
Excerpt selection accepts a section path, entry path, document ID, or entry
alias; it includes complete selected content, deduplicates overlaps, and
preserves source order. Their JSON contracts are `mant.outline/v6` and
`mant.excerpt/v6`; plain text and CommonMark are also available. The TUI
constructs the same full query in memory; agents can select outline and excerpt
projections directly through `--request-json`.

Search is a native projection of the same full query. Rust renders one
canonical CommonMark document, builds visible-text byte mappings from its
CommonMark event stream, and applies the same literal or regular-expression
matcher regardless of output format. Anchors already emitted for sections and
semantic definitions act as a source map. An internal root anchor covers
Markdown content before the first heading, so every occurrence reports both a
stable Markdown range and the nearest path accepted by excerpt selection.
The TUI keeps its in-memory interaction loop and never spawns a process while
typing; this result model is the shared semantic basis for future UI indexing,
not a second parser or a dependency on the system `grep` executable.

## Parsing and source policy

Local Markdown uses `pulldown-cmark` as a source-positioned parser and lowers a
deliberate subset into the same document contract: headings, paragraphs,
strong/emphasis/code, links, code blocks, lists, GFM tables, hard breaks, and
thematic breaks. Unsupported block constructs remain visible as `unsupported`
blocks with exact source text and diagnostics; unsupported inline constructs
remain literal text. ManT never silently drops syntax it cannot structure.
The first H1 supplies document-title metadata without making every following
H2 an extra-indented child. Blank source lines become ordinary layout hints,
and the framing newline before a closing code fence is removed before it can
paint a false empty code row.

An option list is semantic only when every item begins with one or more
code-formatted option names followed by `:` or a dash separator. Those lists
become ordinary definition-list entries with stable option identities, so the
same outline, explain, search, and TUI navigation code works for manuals and
project documentation.

ManT's Markdown extension is structurally separate from ordinary headings. An
optional tldr preface begins with `<!-- mant:tldr:start -->`, ends with
`<!-- mant:tldr:end -->`, and must be the first non-empty construct. CommonMark
renderers hide these comment markers while rendering the enclosed tldr-pages
Markdown normally. ManT masks the complete preface without changing byte or
line coordinates, parses it into `QueryBundle.tldr`, and independently lowers
the remaining Markdown into `QueryBundle.document`.
Consequently outline path `0`, selector `tldr`, search, textual projections,
and the highlighted TUI panel use exactly the same implementation for cached
and document-owned quick references. An origin marker suppresses the
tldr-pages attribution for document-owned content. A same-named ordinary
heading has no special semantics.

The first H1 in the document portion supplies metadata and its following prose
becomes root overview content. This explicit two-channel model keeps tldr
layout conventions out of the general Markdown AST and renderers.

The primary path discovers manual hierarchies in Rust, reads the located
source, and lowers libmandoc's validated man(7) or mdoc(7) tree directly into
`mant.document/v6`. Rust owns compression handling and preserves the original
source path and indexed manual root. Both stored and decoded bytes are capped
across the complete input chain before plain roff bytes cross the parser
boundary. Rust recognizes redirect-only `.so` aliases, resolves raw, gzip, or
zstd targets under the canonical manual root, rejects redirect targets that
escape through symlinks, cycles, excessive depth, and mixed-content includes,
then invokes libmandoc with includes denied. An explicitly indexed leaf file
symlink may resolve outside the root; directory symlinks are not traversed, and
the first and every later `.so` target remain constrained to the root.
Libmandoc therefore opens no manual source or include path for ManT.

One immutable `ManualIndex` owns both catalog discovery and exact lookup. It
derives roots from `MANT_MANPATH`, `MANPATH`, and platform conventions, then
indexes flat pages and traditional `man<section>` hierarchies in the raw, gzip,
and zstd formats the parser can consume. Unix adds user/XDG, PATH, and system
locations; Windows defaults only to `%USERPROFILE%\.local\share\man`.
Ordinary CLI, TUI, and MCP requests therefore do not spawn a host `man`
process or depend on its database being initialized.

Libmandoc is the only manual parser. An unsupported diagnostic does not discard
an otherwise complete document, and recoverable findings remain structured in
the document contract. ManT never invokes a host renderer or chooses between
renderer outputs.

`--manual` is an input-resolution policy outside `mant.request/v6`. It bypasses
registered Markdown with the same filename stem, requires a readable native
manual, and omits any attached tldr quick reference. An explicit `--section`
has the same exclusivity because sections belong only to native manuals.
`--tldr` remains a CLI convenience for the existing reserved `tldr` node
projection, so this policy change does not alter the protocol schema.

Direct lowering is covered by deterministic native fixtures from multiple
distributions, including large git, gcc, clang, tar, and shell pages.
Best-effort native output is retained together with its diagnostics rather than
being silently replaced by another renderer.

Registered documents and caches have distinct lifecycles. Each platform has
one user data root containing `sources.toml` and a `documents/` tree. Personal
documents are flat at that tree's root; installed source directories and their
revision metadata live below `documents/sources/`. The native CLI alone updates
Git repositories or direct archives with transactional directory replacement.
The private tldr checkout remains below the platform cache root.
Installed-client tldr roots
precede the private checkout, which remains the final read fallback even when
a client executable is present.

Vertical layout is part of this normalization boundary rather than a TUI
heuristic. Sections retain the distance requested before `SH`, `SS`, `Sh`, and
`Ss`; ordinary blocks retain macro-driven leading distance in their layout;
explicit `sp` and blank roff input lines remain vertical-space nodes. mdoc list
compactness stays on the list block, while man `.PD` changes are also retained
per definition item so one option list can switch between normal and compact
layout. Renderers may adapt these row counts to their medium, but must not
invent or discard terminal spacing at the process boundary.

Filled inline flow is normalized at the same boundary. A roff `.br` becomes an
inline hard break, later filled source lines contribute word boundaries, and
man alternating-font macros concatenate their arguments according to man(7)
rather than punctuation heuristics. Non-printing width and break-hint escapes
never become visible characters. Consequently, the TUI, text, and CommonMark
renderers consume the same line and spacing semantics instead of reconstructing
them independently.

Because libmandoc 1.14.6 uses process-global character, diagnostic, tag, and
recursion state, all embedded parser sessions are serialized.  Initialization
happens once, and the private shim provides per-request diagnostic reset and
capture so one parse cannot contaminate the next.

## Ownership after migration

Rust owns:

- manual source loading, decompression, aliases, and include context;
- local Markdown loading, structured-subset parsing, and loss diagnostics;
- lowering the owned `libmandoc-rs` man/mdoc tree;
- section, block, inline, layout-hint, link, table, and equation semantics;
- tldr cache discovery, parsing, update behavior, and query composition;
- versioned JSON and CommonMark serialization;
- terminal mode selection, Ratatui rendering, search, navigation, scrolling,
  menus, and sidebar sizing.

## Test boundary

Rust tests are authoritative for all parsing and serialization semantics.
They use checked-in roff, tldr, and expected JSON fixtures and
do not require an installed manual page for normal CI.  They also cover
repeated parser sessions, diagnostic isolation, compression, includes, and
Markdown escaping.

The shipped `docs/manuals/mant.md` file is also a fixture: tests require it to
parse without lossy fallbacks and to expose its embedded quick reference and
semantic entries.

Rust additionally owns `mant` argument, stdio protocol, exit-code, interactive
reader, and agent-facing output tests. Shared contract fixtures are decoded,
generated, and compared by Rust. Source-level coverage exercises the current
implementation without a duplicate frontend.

## Repository boundary

The repository root is the Cargo workspace. Every shipped behavior belongs to
one of its six crates, while `tests/fixtures/` and `tests/contracts/` provide
the only cross-crate external data. Build, test, coverage, and release
automation invoke Cargo directly; no second application runtime participates
in compilation or execution.
