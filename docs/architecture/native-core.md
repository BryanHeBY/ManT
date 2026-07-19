# Native document core

Status: accepted for the `refactor/rust-native-addon` migration.

## Context

Mant currently obtains HTML from bundled mandoc, an installed mandoc, or
man-db/groff and then reconstructs a document model in TypeScript.  A second,
source-level `mant.roff-ast/v1` format exposes part of libmandoc's internal
tree for diagnostics.  These two trees have different purposes and neither is
an appropriate long-term boundary between the document engine and the TUI.

Keeping parsing and serialization rules in both Rust and TypeScript would
also allow whitespace, list, link, and fallback behavior to diverge.  The
native migration therefore establishes one renderer-neutral document model
and makes Rust the sole owner of document interpretation.

## Decision

Mant will use a Rust native core with four layers:

```text
mant-ast          versioned document and query contracts
mant-mandoc-sys   pinned libmandoc build and private C shim
mant-core         source loading, parsing, query, and output renderers
mant-napi         thin JSON-oriented Node-API adapter
```

The TypeScript application becomes a thin host and presentation layer.  It
owns CLI argument handling, user-facing error presentation, the OpenTUI React
interface, navigation, search interaction, and terminal styling.  It does not
interpret roff or renderer HTML and does not serialize JSON or Markdown
documents.

The C shim is private and deliberately small.  It hides libmandoc structure
layouts, manages parser handles, and exposes the information Rust needs to
lower a document.  It never defines Mant's public AST and never formats JSON.

## Stable and unstable models

`mant.document/v1` is the stable manual document contract consumed by the UI
and output renderers.  `mant.query/v1` combines an optional manual document
with an optional tldr document while preserving their different sources and
licences.

The existing `mant.roff-ast/v1` tree remains a diagnostic format during the
migration.  It mirrors pinned libmandoc implementation details, omits some
normalized arguments and table/equation payloads, and is not the UI contract.
It may later be replaced by a Rust-generated debug representation.

All cross-language payloads carry an exact schema identifier.  Rust structs
are the source of truth, and TypeScript validates the JSON boundary before
passing a value to React.  New optional object fields may be added within a
schema version; incompatible meaning changes and new required node variants
require a new schema version.

## Native API boundary

The final Node-API surface is use-case oriented rather than a mirror of parser
internals:

```text
queryJson(requestJson)       -> query JSON
queryMarkdown(requestJson)   -> CommonMark
updateTldr(requestJson)      -> update result JSON
nativeVersion()              -> native API version
```

The UI requests compact query JSON.  `--json` requests pretty query JSON and
writes the native string unchanged.  `--markdown` writes native CommonMark
unchanged.  Fatal native failures cross the boundary as concise errors;
recoverable parser findings are structured diagnostics in the query result.

## Parsing and fallback policy

The primary path reads the located manual source and lowers libmandoc's
validated man(7) or mdoc(7) tree directly into `mant.document/v1`.  Rust owns
compression handling and preserves the original source path and include base
directory.  `.so` aliases and includes must work without exposing temporary
paths in the result.

The current TypeScript HTML parsers remain available until direct lowering is
covered by deterministic fixtures and native/legacy differential tests.  A
groff HTML compatibility parser moves into Rust before the legacy parsers are
removed, retaining the current fallback for constructs libmandoc reports as
unsupported.  Best-effort native output is retained together with its
diagnostics when no higher-fidelity fallback is available.

Because libmandoc 1.14.6 uses process-global character, diagnostic, tag, and
recursion state, all embedded parser sessions are serialized.  Initialization
happens once, and the private shim provides per-request diagnostic reset and
capture so one parse cannot contaminate the next.

## Ownership after migration

Rust owns:

- manual source loading, decompression, aliases, and include context;
- libmandoc man/mdoc lowering and groff HTML compatibility parsing;
- section, block, inline, layout-hint, link, table, and equation semantics;
- tldr cache discovery, parsing, update behavior, and query composition;
- versioned JSON and CommonMark serialization.

TypeScript owns:

- CLI token parsing, TTY selection, and user-facing error presentation;
- the native client and runtime schema/version guard;
- OpenTUI React rendering, colors, syntax highlighting, and input state;
- interactive search, navigation, scrolling, menus, and sidebar sizing.

## Test boundary

Rust tests are authoritative for all parsing and serialization semantics.
They use checked-in roff, renderer HTML, tldr, and expected JSON fixtures and
do not require an installed manual page for normal CI.  They also cover
repeated parser sessions, diagnostic isolation, compression, includes, and
Markdown escaping.

TypeScript retains boundary, CLI, and UI tests.  Shared contract fixtures are
decoded by TypeScript and generated or compared by Rust.  During migration,
differential tests compare native results with the existing parsers for large
git, gcc, clang, tar, and ls pages.  Implementation-specific HTML parser tests
are removed only after equivalent source-level Rust tests exist.

## Migration rules

Every migration commit must keep the repository buildable and tested.  Third-
party mandoc sources are committed separately from Mant code.  The native
path is introduced alongside the legacy path, made authoritative in a later
commit, and only then followed by deletion of the TypeScript parsers, sidecar,
and duplicate dependencies.  Behavior switches and cleanup are never combined
in the same commit.
