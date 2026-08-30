# mant-ir

## Name

mant-ir — source-neutral document model shared by ManT parsers and in-process consumers

## Description

`mant-ir` is the normalized in-memory representation produced after Markdown, man, mdoc, and tldr input has been parsed. It lets the query engine, terminal UI, renderers, and indexes operate without depending on source-specific syntax trees.

The Rust crate is a library contract for trusted semantic components. It is not the versioned structured integration contract, and its Serde representation is not, by itself, a compatibility promise. Host and process consumers should use the versioned projections and generated schemas described by [mant-protocol(5)](mant-protocol.md).

## Pipeline Position

```text
Markdown ─┐
man/mdoc ─┼─> mant-engine ─> mant-ir ─┬─> mant-ui
tldr ─────┘                           ├─> renderers
                                      └─> mant-protocol
```

Source parsers retain syntax-specific facts only until they can be expressed as shared document semantics. Protocol projections may omit internal data, add schema discriminators, or reshape fields for a stable host or process boundary.

## Document

`Document` is the root of a normalized manual body. It contains:

| Field | Meaning |
| --- | --- |
| `parser` | Producer name and version, when known |
| `source` | Source format and original path |
| `meta` | Normalized title, native manual metadata, names, and alias target |
| `diagnostics` | Recoverable parser and validation findings |
| `blocks` | Content before the first heading |
| `sections` | Recursive content-section tree |

`DocumentMeta.manual_section` is a native manual category such as `1` or `3p`. A `Section` is a heading-backed content node. The two concepts are intentionally distinct.

`SourceFormat` is one of `man`, `mdoc`, or `markdown`. Embedded and cached tldr pages are stored beside the main document in `ResolvedContent`, not disguised as document sections.

## Sections

A `Section` contains a document-local `NodeId`, a plain title, blocks, child sections, optional source coordinates, and source-requested spacing before the heading. Depth is derived from tree position rather than stored as mutable metadata.

The virtual ID `document-overview` addresses root blocks before the first section. IDs are unique only inside one document and may change when the source changes. Consumers should rediscover them through the current index or outline rather than persisting them globally.

## Blocks

The block union preserves structures that matter across renderers:

| Variant | Semantics |
| --- | --- |
| `paragraph` | Filled inline flow |
| `preformatted` | Literal flow with an optional language |
| `list` | Bullet, ordered, or plain items containing blocks |
| `definition-list` | Terms, aliases, identities, and block descriptions |
| `table` | Rows and block-capable cells with spans and alignment |
| `equation` | Normalized equation source |
| `vertical-space` | Explicit source-requested blank terminal rows |
| `thematic-break` | Semantic separator |
| `unsupported` | Visible source preserved when no lossless semantic lowering exists |

`LayoutHint` carries only portable presentation facts currently required for faithful terminal rendering: indentation columns and spacing rows before a block. It is not a general-purpose CSS or roff device model.

Lists contain block-capable items so nested lists and displays do not flatten into prose. Definition terms contain inline trees and descriptions contain blocks. Table cells likewise contain blocks even when a source parser currently produces a single paragraph.

## Inline Content

The inline union contains:

| Variant | Semantics |
| --- | --- |
| `text` | Plain visible text |
| `strong` | Strong importance or source bold semantics |
| `emphasis` | Emphasis or source italic semantics |
| `code` | Literal inline text |
| `link` | Visible children plus a typed destination |
| `anchor` | Zero-width document-local destination |
| `line-break` | Explicit break inside one flow |

Links use a closed `LinkTarget` union rather than stringly typed URLs:

| Kind | Destination |
| --- | --- |
| `external` | URI |
| `email` | Address without a `mailto:` prefix |
| `document` | Relative registered Markdown document and optional fragment |
| `manual` | Manual name and optional native manual section |
| `section` | Resolved `NodeId` in the current document |

Visible link children remain useful when a frontend cannot activate the destination.
Document validation requires RFC 3986 ASCII component characters and complete
percent-encoded triplets. It also rejects malformed external HTTP(S)
authorities, userinfo, hosts, IPv6 literals and ports, mailto targets without a
mailbox, and typed email addresses outside the supported ASCII dot-atom form.
Consumers may apply a narrower activation allowlist without reimplementing
that structural validation.

## Semantic Definitions

A definition-list item may carry `DefinitionIdentity` when ManT can identify a stable, addressable entry. The identity records:

| Field | Meaning |
| --- | --- |
| `id` | Document-local entry and anchor ID |
| `role` | Option, marker, operand, command, configuration key, environment variable, variable, value, or generic term |
| `case` | Sensitive or insensitive alias matching |
| `names` | Exact normalized aliases exposed to selectors |

The identity is assigned during lowering, before source-specific macro information is discarded. Ordinary prose definitions remain valid definition items without an identity.

For semantic definitions, the engine derives a role-qualified identity from the complete semantic name after source-specific parsing. Formatter navigation tags remain page-local anchors but do not become semantic IDs merely because their spelling is short or collides with a command. Collisions use a deterministic fingerprint of semantic identity and content rather than a source-order suffix; unrelated sibling insertion and reordering therefore cannot silently redirect an ID. Section and entry allocation are independent. These IDs identify the same logical content within one current document, but an independently updated host manual can change or remove that content, so consumers rediscover before reuse.

`SemanticIndex` is a rebuildable sidecar over these content definitions. It
classifies entries with `EntryKind`, preserves exact selector `aliases`
separately from complete author-written `forms`, and retains nested ownership
such as command → option → value. `EntrySummary` reports direct entries,
descendants, forms, and kind totals without materializing the indexed nodes in
an outline. `ValueDomain::Choices` describes observed nested choices;
`EntrySet` and `Union` are reserved for producers with explicit evidence and
are never guessed from prose.

## Addresses and Resolution

`DocumentAddress` identifies a discoverable candidate independently from its filesystem location:

| Address | Canonical catalog path |
| --- | --- |
| Root Markdown `guide/setup` | `documents/guide/setup` |
| Source Markdown `team/guide/setup` | `sources/team/guide/setup` |
| Native `printf(3)` | `manual/3/printf` |

`ResolvedContent` is the in-process handoff from the engine. It carries a display label, optional exact address, optional document body, and optional tldr page. The TUI consumes this value directly. Process clients receive versioned protocol projections; MCP tools present focused projections as compact text or CommonMark.

## Source Coordinates

`SourceSpan` uses one-based lines and columns for diagnostics. When a parser can provide exact offsets, `byte_range` is a half-open range over UTF-8 bytes in the original input and is the canonical machine-facing coordinate.

Native libmandoc nodes generally provide line and column positions but not exact byte ranges. Markdown lowering preserves byte ranges. Rendered search coordinates belong to the independent `mant.markdown/v1` projection and must not be confused with input spans.

## Diagnostics

Diagnostics have `style`, `warning`, `error`, or `unsupported` severity, an optional stable code, a message, and an optional source span. They describe recoverable source findings; fatal I/O, decompression, parsing, request, or transport failures remain ordinary errors outside the document.

`Block::Unsupported` and an `unsupported` diagnostic are used when ManT can safely keep visible source but cannot represent its semantics. Consumers should display the retained content and may surface the diagnostic separately.

## Validation

`validate_document` checks invariants after parsing or deserialization, including document-local identity validity, uniqueness, link targets, and structural consistency. Custom producers should validate before handing a document to indexes or frontends.

`DocumentIndex` is an immutable content-navigation index over a validated
document. `SemanticIndex` independently projects semantic definitions for
outline discovery and can always be rebuilt from the document. `NodePath` and
outline paths are ephemeral coordinates derived from the current tree; nested
semantic entries use paths such as `2/e3/e1`. These coordinates are not
long-term storage identifiers and can change when a source document inserts or
removes earlier entries. A product version therefore does not freeze paths in
host-provided manuals.

## Quick References

`TldrDocument` contains normalized description paragraphs, examples, platform, language, source path, and provenance. Each example retains both its complete command string and command parts so placeholders can be styled consistently by terminal frontends.

`TldrOrigin` distinguishes community tldr-pages cache content from a document-owned embedded quick reference. This controls attribution and update policy without changing the main document tree.

## Rust API

Add the crate when implementing an in-process parser, index, renderer, or trusted frontend:

```toml
[dependencies]
mant-ir = "^0.10.0"
```

Prefer constructors and visitors from the crate over recursively rewriting public fields by hand. Use `visit::Visit` or `visit::VisitMut` for whole-document passes and run validation after transformations that can affect identities or links.

## Stability

The crate has its own semver, independent of the `mant` executable and native wire protocol versions. Pre-1.0 Rust API evolution may require downstream source changes. The stable structured promise is the exact schema identifier exposed by `mant-protocol` and emitted by the executable, not semver inference from the IR crate alone.

## See Also

[mant(1)](mant.md), [mant-protocol(5)](mant-protocol.md), [mant-markdown(7)](mant-markdown.md), and [mant-roff(7)](mant-roff.md)
