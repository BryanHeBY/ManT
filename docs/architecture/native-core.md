# Native document core

Status: implemented.

## Context

ManT previously obtained HTML from bundled mandoc, an installed mandoc, or
man-db/groff and then reconstructed a document model in TypeScript. A second
`mant.roff-ast/v1` sidecar exposed part of libmandoc's internal tree. These
parallel representations were removed after the native document contract
became authoritative.

Keeping parsing and serialization rules in both Rust and TypeScript would
also allow whitespace, list, link, and fallback behavior to diverge.  The
native migration therefore establishes one renderer-neutral document model
and makes Rust the sole owner of document interpretation.

## Decision

ManT will use a Rust native core with four layers:

```text
mant-ast          versioned document and query contracts
libmandoc-rs      owned libmandoc parse tree, private C shim, and build logic
mant-core         source loading, parsing, query, and output renderers
mant-cli          standalone agent CLI and versioned stdio process boundary
```

The TypeScript application becomes a thin host and presentation layer.  It
owns the interactive `mant` command, user-facing TUI errors, the OpenTUI React
interface, navigation, search interaction, and terminal styling.  The native
`mant-cli` command is independently usable by agents and scripts.  TypeScript
does not interpret roff or renderer HTML and does not serialize JSON or
Markdown documents.

`libmandoc-rs` is the boundary around the bundled C parser. Its deliberately
small private C shim hides libmandoc structure layouts and parser handles; the
crate copies each completed parse into an owned, renderer-neutral tree with
structured diagnostics. The crate exposes no ManT-specific types and never
formats JSON. `mant-core` alone lowers that parse tree into ManT's public
document contract.

## Stable and unstable models

`mant.document/v2` is the stable manual document contract consumed by the UI
and output renderers.  `mant.query/v2` combines an optional manual document
with an optional tldr document while preserving their different sources and
licences.

All cross-language payloads carry an exact schema identifier.  Rust structs
are the source of truth, and TypeScript validates the JSON boundary before
passing a value to React.  New optional object fields may be added within a
schema version; incompatible meaning changes and new required node variants
require a new schema version.

The initial document contract keeps navigation semantic instead of encoding
every destination as an untyped URI:

- `external-link` stores an external `uri` and its rendered label;
- `email-link` stores an address without a synthetic `mailto:` prefix;
- `manual-reference` identifies another manual by name and optional section;
- `section-reference` targets a document-local section ID;
- `anchor` marks a zero-width document-local destination such as mdoc `Tg`.

Definition-list entries may also carry a semantic `identity` with a stable
document-local ID, a role, and normalized names. Version 2 currently assigns
this identity to recognized command-line options. It preserves the complete
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

The project deliberately uses a process boundary instead of Node-API. This
avoids ABI-specific addons, isolates native failures, and makes the same binary
directly useful outside Bun. One-shot requests serve the TUI and shell usage;
the same executable also provides a long-lived, read-only MCP stdio server.
The public surface is use-case oriented rather than a mirror of parser
internals:

```text
mant-cli <topic> [--format <format>]   -> query Markdown, text, or JSON
mant-cli <topic> --outline [sections|options] -> selectable section and option tree
mant-cli <topic> --node <path-or-id>   -> selected section subtrees
mant-cli <topic> --explain <alias-or-id> -> one option, command, or environment entry
mant-cli <topic> --search <pattern>    -> matches with node and Markdown locations
mant-cli <topic> --force-libmandoc     -> strict direct-parser diagnosis
mant-cli <topic> --force-groff         -> opt into the groff HTML compatibility path
mant-cli --update-tldr                 -> update result JSON
mant-cli --protocol-version            -> protocol description JSON
mant-cli --schema <contract>           -> generated JSON Schema
mant-cli --mcp                         -> read-only MCP tools over stdio
```

For the TUI, `mant-cli --request-json --format json --compact` reads one closed,
versioned `QueryRequest` object from standard input and emits exactly one
`mant.query/v2` object on standard output.  Standard error contains concise
diagnostics only.  Status 0 means success, 2 means invalid invocation or
request, and 1 means an operational failure.  The TypeScript client drains
stdout and stderr concurrently, validates the protocol and schema, and starts
one process per document query; interactive search and navigation never spawn
additional native processes.

For agent clients that speak the Model Context Protocol, `mant-cli --mcp`
keeps standard output exclusively for JSON-RPC and exposes four generated,
read-only tools: manual outline, selected content, semantic explanation, and
search. Their input and output schemas derive directly from Rust types, while
diagnostics remain on standard error. MCP is an alternate process protocol; it
does not add another executable or a second document interpretation path.

`mant.request/v2` requires a `schema` marker, `topic`, and a closed `view`
variant; it accepts an optional manual `section` and rejects unknown fields at
both the envelope and view levels. `full` returns `mant.query/v2`, `outline`
selects either section-only or option-aware structure, `excerpt` selects one
or more node paths, IDs, or aliases, and `search` returns `mant.search/v1`.
The direct-only `--explain` convenience flag normalizes to exactly one
`excerpt` selector, then rejects anything other than a semantic manual entry.
It deliberately adds no request or response variant, so agents retain one
stable excerpt contract for both explicit `--node` requests and option-focused
explanations.
`mant-cli --schema request` exposes that exact input contract; `query`,
`outline`, `excerpt`, and `search` expose the output contracts, while `all`
returns a named catalog. The schemas are derived with
Schemars from `mant-ast`'s Serde types, explicitly pinned to JSON Schema Draft
2020-12, and generated separately for deserialize and serialize behavior.

`mant` and `mant-cli` are separate installed executables.  The TUI resolves
`MANT_CLI_PATH` first and otherwise looks up `mant-cli` on `PATH`; it never
embeds or extracts the Rust binary.  Local `bun run dev` performs an
incremental Cargo release build and supplies the staged binary through
`MANT_CLI_PATH`.  Release builds place both executables beside each other in
`dist/`, ready for an installer to put them on the same `PATH`.

Direct `mant-cli` queries default to Markdown for useful terminal and agent
output. `--format json` is pretty by default and `--compact` is available to
process clients. Fatal native failures cross the boundary as concise errors;
recoverable parser findings are structured diagnostics in the query result.

Outline and excerpt views are projections of the same complete native
document, so they never reimplement parsing rules. Outlines expose both a
one-based tree path such as `4.2` and the document-local section ID. Passing
`--outline` includes semantic option entries with paths such as `4.2/o3` by
default. `--outline sections` is the explicit compact view for callers that
only need section topology.
Excerpt selection accepts a section path, option path, document ID, or option
alias; it includes complete selected content, deduplicates overlaps, and
preserves source order. Their JSON contracts are `mant.outline/v2` and
`mant.excerpt/v2`; plain text and CommonMark are also available. The TUI uses
the same `QueryRequest` contract with `view.kind = "full"`; agents can select
outline and excerpt projections directly through `--request-json`.

Search is a native projection of the same full query. Rust renders one
canonical CommonMark document, builds visible-text byte mappings from its
CommonMark event stream, and applies the same literal or regular-expression
matcher regardless of output format. Anchors already emitted for sections and
semantic definitions act as a source map, so every occurrence reports both a
stable Markdown range and the nearest path accepted by excerpt selection.
The TUI keeps its in-memory interaction loop and never spawns a process while
typing; this result model is the shared semantic basis for future UI indexing,
not a second parser or a dependency on the system `grep` executable.

## Parsing and renderer policy

The primary path reads the located manual source and lowers libmandoc's
validated man(7) or mdoc(7) tree directly into `mant.document/v2`.  Rust owns
compression handling and preserves the original source path and include base
directory.  `.so` aliases and includes must work without exposing temporary
paths in the result.

Libmandoc is the only default parser. An unsupported diagnostic does not
discard an otherwise complete document, and ManT does not automatically invoke
groff or choose between renderer outputs. `--force-libmandoc` is a strict
diagnostic policy outside `mant.request/v2`: it requires a direct native
manual, prevents a tldr-only response, and prints recoverable parser findings
on standard error.

`--force-groff` is an explicit compatibility path for investigating renderer
differences. It calls `man -Thtml` and lowers the resulting HTML without first
parsing the source through libmandoc. It likewise rejects a tldr-only response
and prints renderer diagnostics. It is intentionally opt-in because its
availability depends on the host `man` implementation and its coverage is
smaller than direct source lowering.

Direct lowering is covered by deterministic native fixtures and was compared
against the former TypeScript implementation on large installed ls, git, gcc,
clang, and tar pages before cut-over. The groff HTML compatibility parser also
lives in Rust for the explicit `--force-groff` diagnostic mode. Best-effort
native output is retained together with its diagnostics rather than being
silently replaced by a second renderer.

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
- lowering the owned `libmandoc-rs` man/mdoc tree and groff HTML compatibility
  parsing;
- section, block, inline, layout-hint, link, table, and equation semantics;
- tldr cache discovery, parsing, update behavior, and query composition;
- versioned JSON and CommonMark serialization.

TypeScript owns:

- the interactive `mant` command, TTY selection, and TUI error presentation;
- the `mant-cli` process client and runtime schema/version guard;
- OpenTUI React rendering, colors, syntax highlighting, and input state;
- interactive search, navigation, scrolling, menus, and sidebar sizing.

## Test boundary

Rust tests are authoritative for all parsing and serialization semantics.
They use checked-in roff, renderer HTML, tldr, and expected JSON fixtures and
do not require an installed manual page for normal CI.  They also cover
repeated parser sessions, diagnostic isolation, compression, includes, and
Markdown escaping.

Rust additionally owns `mant-cli` argument, stdio protocol, exit-code, and
agent-facing output tests.  TypeScript retains process-client, interactive
command, and UI tests.  Shared contract fixtures are decoded by TypeScript and
generated or compared by Rust. The one-time native/legacy differential gate
was removed together with the old parser after the cut-over commit;
equivalent source-level and renderer-compatibility coverage remains in Rust.

## Migration rules

The migration kept the repository buildable at every commit. Third-party
mandoc sources were committed separately, the native path was introduced and
tested alongside the legacy path, the TUI was switched in its own commit, and
only then were the TypeScript parsers, sidecar, and duplicate dependencies
deleted.
