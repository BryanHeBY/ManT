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
| `fragmentAliases` | Exact source fragments resolving to the normalized document root |
| `diagnostics` | Recoverable parser and validation findings |
| `blocks` | Content before the first heading |
| `sections` | Recursive content-section tree |

`DocumentMeta.manual_section` is a native manual category such as `1` or `3p`. A `Section` is a heading-backed content node. The two concepts are intentionally distinct.

`SourceFormat` is one of `man`, `mdoc`, or `markdown`. Embedded and cached tldr pages are stored beside the main document in `ResolvedContent`, not disguised as document sections.

## Sections

A `Section` contains a normalized document-local `NodeId`, optional exact `fragmentAliases`, a plain title, blocks, child sections, optional source coordinates, and source-requested spacing before the heading. Depth is derived from tree position rather than stored as mutable metadata.

The virtual ID `document-overview` addresses root blocks before the first section and may carry exact source aliases from a removed Markdown H1 title. IDs are unique only inside one document and may change when the source changes. Consumers should rediscover them through the current index or outline rather than persisting them globally.

## Blocks

The block union preserves structures that matter across renderers:

| Variant | Semantics |
| --- | --- |
| `paragraph` | Filled inline flow |
| `preformatted` | Literal flow with an optional language |
| `list` | Bullet, ordered, or plain items containing blocks |
| `definition-list` | Terms and block descriptions with item layout, source, and optional `entry` facts |
| `table` | Rows and block-capable cells with spans and alignment |
| `equation` | Normalized equation source |
| `vertical-space` | Explicit source-requested blank terminal rows |
| `thematic-break` | Semantic separator |
| `unsupported` | Visible source preserved when no lossless semantic lowering exists |

`LayoutHint` carries only portable presentation facts currently required for faithful terminal rendering: indentation columns and spacing rows before a block. It is not a general-purpose CSS or roff device model.

A standalone definition description block adds `DefinitionItem::DESCRIPTION_INDENT_COLUMNS` (four columns) before applying its own layout hint, whether or not the term is inline. `DefinitionItem::inline_description()` identifies the first paragraph that may share the term's line: explicit leading spacing or a non-paragraph block prevents that presentation. Only that first paragraph hangs from the displayed label; later paragraphs, nested blocks and code use the structural four-column origin, not the label's width. Native continuation normalization, plain text, and the TUI share this distinction. Their first-line typography and paragraph wrapping remain presentation policy. Markdown expresses ownership through its own block syntax rather than terminal-column geometry.

`DefinitionItem.layout: DefinitionLayout` groups these item-level choices:
`inlineTerm` defaults to false; optional `spacingBeforeLines` defaults to
inheriting list compactness. Explicit zero spacing is preserved and does not
mean inheritance. Missing layout and `{}` have the same default; `layout:null`,
unknown fields, and the former top-level `inlineTerm`/`spacingBeforeLines` fields
are rejected. Canonical output omits empty layout but retains
`"layout":{"spacingBeforeLines":0}`. Semantic annotation never changes layout.

Lists contain block-capable items so nested lists and displays do not flatten into prose. Definition terms contain inline trees and descriptions contain blocks. Table cells likewise contain blocks even when a source parser currently produces a single paragraph.

### Consecutive declaration context

`Block::DefinitionList.declarationGroups` is optional reading metadata, not a
change to physical content. Each `DeclarationGroup { startItem, endItem }`
addresses a half-open range of at least two items in that exact list. Every
member has a readable head; earlier members have no readable description and
the last supplies the context. `resolve()` checks these bounds and content
conditions; document validation additionally rejects overlapping ranges with
`ir.invalid-declaration-group`. Anchors and spacing alone are not readable text.

Items keep their own identities, source spans, forms, descriptions and children.
A group has no permanent ID, does not prove alias equivalence or complete value
choices, and never copies the final description into earlier items. It survives
IR serialization without native parser pointers. Excerpts retain/rebase only
whole groups; a single-owner excerpt may therefore remain empty while explain
separately returns useful group context. Renderers ignore the annotation for
full-document layout. Native producers supply bounded source evidence; ordinary
Markdown lists are not automatically grouped. IR validation checks structure,
not roff syntax or the applicability of each sentence to each member.

`ListKind` is `Bullet`, `Plain`, or `Ordered { start: Option<u64> }`. Only ordered
lists carry a start. Its JSON is a tagged object, for example
`"kind":{"kind":"ordered","start":0}`, not a string plus a block-level
start. Missing/null ordered start is unknown and canonical output omits it;
display uses one without rewriting the source. `ListKind::ordinal` and
`for_excerpt` share saturating u64 arithmetic: a selected later item preserves
its effective original number, including zero-based and u64::MAX lists.
Bullet/plain with any start (even null), old string kinds, outer start, duplicate
fields, and negative/fractional/oversized starts are rejected during decoding.

Every canonical ID in `DocumentIndex` is a local navigation target, including entries attached directly to ordinary list items or native definitions. A `LinkTarget::Section` may target any such ID; its historical variant name does not restrict links to heading-backed sections. Entry targets do not need an additional inline anchor. Producers resolve exact authored fragments to canonical IDs before validation; duplicate identities and incompatible roles remain separate errors.

`TableGrid` supplies shared sparse logical-column coordinates for table consumers. Horizontal spans omit covered cells; vertical continuations retain explicit empty cells in subsequent rows, while `rowSpan` remains on the content owner. Covered content is never repeated. Callers can request dense column slots with an explicit budget, so large span values need not allocate a dense grid.

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

| Kind | Fields | Navigation class |
| --- | --- | --- |
| `external` | `uri` | Host-activated absolute URI |
| `email` | `address` without a `mailto:` prefix | Host-activated mailbox |
| `document` | `name`, optional `fragment` | Cross-document graph edge |
| `manual` | `name`, optional `manualSection` | Cross-document graph edge |
| `section` | resolved document-local `id` | Current-document destination |

Entry identities, `Inline::Anchor` and section IDs define destinations inside the current
document. Their `id` is always the normalized internal identity. Optional
`fragmentAliases` preserve exact source-authored destinations such as mdoc
`.Tg Mixed.Target`, `--option`, or a Markdown heading ID without admitting
those spellings into the `NodeId` namespace. `DocumentIndex::fragment_target`
maps a canonical ID or exact alias to one target and refuses ambiguous aliases.
An anchor can also retain the source span of the paragraph, definition, list
item, table cell, or standalone target that owns its destination. This
`ownerSource` provenance is distinct from the inline's eventual placement and
allows consumers to verify that zero-width navigation did not drift to a
neighbouring structure during lowering.
`LinkTarget::Document` and `Manual` connect logical catalog entries
and are the only link kinds followed by bounded multi-document operations.
`External` and `Email` are host actions: they never expand a documentation
scope. This classification prevents a query from turning arbitrary URI text or
a local filesystem path into an implicit document edge.

A document target retains the extension-free relative name derived from its
Markdown source. It does not store an absolute cache path. Resolution therefore
requires the referring document's `DocumentAddress`; the engine resolves the
target within that registered source and rejects traversal across its root. A
manual target records a topic and optional exact native manual section, normally
from mdoc `Xr`, man `MR`, or another source construct with equivalent evidence.
The IR records this intent, while the engine owns catalog lookup, ambiguity, and
source-confinement policy.

Visible link children remain useful when a frontend cannot activate the destination.
Document validation requires RFC 3986 ASCII component characters and complete
percent-encoded triplets. HTTP(S) host names use the RFC 3986 `reg-name`
grammar, including underscores, a terminal DNS root dot, and percent-encoded
triplets. Internationalized host names must be supplied in their ASCII
punycode form; raw Unicode belongs to IRI syntax and is rejected. Validation
also rejects malformed external HTTP(S) authorities, userinfo, IPv6 literals
and ports, mailto targets without a mailbox, and typed email addresses outside
the supported conservative ASCII dot-atom and DNS-domain form.
Mailto recipients are percent-decoded exactly once before mailbox validation;
the shared typed-email serializer percent-encodes URI-sensitive local-part
characters so accepted addresses remain activatable without raw concatenation.
Consumers may apply a narrower activation allowlist without reimplementing
that structural validation.

Typed targets let every consumer use the same document graph. The TUI can
activate local destinations and request cross-document resolution while keeping
back/forward history; CLI and MCP scopes can traverse `Document` and `Manual`
edges breadth-first under explicit document, depth, and byte limits; renderers
can preserve visible children without pretending an unsupported destination is
clickable. Consumers must not recover navigation semantics from rendered text.

## Semantic Definitions

A semantic entry passes through three deliberately separate representations:

```text
ListItem.entry / DefinitionItem.entry   optional EntryFacts on original content
                         │
                         └─> EntryOwner   borrowed facts and content view
                                └─> SemanticIndex   rebuildable entry hierarchy
                                       └─> outline/query projection
```

This separation keeps the document tree authoritative. An index can be rebuilt
without reparsing, and an outline can omit entries or include only summaries
without deleting their definitions from the document.

Content ownership is not behavioral equivalence: several names can share one
description while referring to different options or subjects. In the current
model, `names` in both facts and projections provides selectable spellings, not a
verified alias relationship or a complete command grammar. Indexes must not
invent hidden names or regenerate the authoritative body from these fields.
Ordinary `ListItem` and `DefinitionItem` values both carry optional `EntryFacts`
through `entry`; neither shape is a compatibility wrapper. `EntryForm` and
`EntryContentSlice` bind forms to direct owner
blocks or native terms, preserving styled inline ancestry and validating UTF-8
leaf ranges. Out-of-bounds, overlapping or reordered pieces are invalid, not
partial forms; an empty forms collection instead means unrecorded. Markdown attaches these references without converting its
ordinary lists to native definition items, as described in
[mant-markdown(7)](mant-markdown.md).

Navigation, excerpts, search ownership, scope links and TUI anchors support
both owner kinds. Excerpts retain the original single-item container and its
numbering/layout, not a synthesized definition. `Block::entry_owner()` borrows
the facts and content from such a selected block without copying its body.

Either content shape may carry `EntryFacts` when ManT can identify an
addressable entry. The facts record:

| Field | Meaning |
| --- | --- |
| `id` | Document-local owner ID; no additional inline anchor required |
| `kind` | Shared structured `EntryKind`; parameter variants contain `ParameterKind` |
| `case` | `NameCase`, exact spelling or ASCII-insensitive name lookup |
| `names` | Exact normalized names exposed to selectors; not an equivalence relation |
| `nameBindings` | Name indices, evidence kinds and occurrences bound to final-IR authored forms |
| `forms` | Explicit ordered owner-relative content slices; empty means unrecorded, never implicit terms |
| `aliasGroups` | Explicit disjoint groups of at least two visible, uniquely bound names; absent means unknown |
| `aliasOf` | Explicit same-document relation to a unique, compatible single-subject entry; not content redirection |
| `valueDomain` | Optional source-declared value space |

The facts are assigned during lowering, before source-specific macro information is discarded. Ordinary content remains a valid list or definition item without `entry`. Unannotated items and table cells are transparent to direct semantic-child discovery. An annotated child establishes a new owner boundary, even if its forms or names are invalid; its grandchildren are not siblings of that child. Full scope-link traversal still visits all content in source order.

Both item types carry their own optional `source` span. It refers to the original
item, not the first remaining paragraph after annotation comments are consumed.
Synthetic owners can leave it unknown; source positions never determine IDs.

Forms and names fail independently. Unrecorded forms retain the owner and its
children without inventing usage text. Invalid nonempty references produce
`ir.invalid-entry-content` and suppress the entire forms projection, not the
owner. Every selectable name needs exact occurrences within valid explicit
forms. Invalid or missing name bindings suppress the complete names field and
dependent aliases, while valid Form evidence, ID/path navigation and literal
content remain available. Case policy affects lookup, not the exact spelling
required by a binding. Links become form-linked document targets only through
valid explicit forms; ordinary description links still participate in scope.

`EntryOwner::forms()` returns `EntryForms::Unrecorded`, borrowed/projected valid
forms, or `None` for invalid references (also for an unannotated owner). Native
producers use `EntryForm::term(index)` explicitly. Complete consecutive terms
borrow their original slice; nonconsecutive complete forms borrow individually.

Shared validation rejects name bindings into descriptions or mismatching text,
overlapping/hidden group members, incompatible or missing relationship targets,
self-references and cycles. Source spans are evidence, not replacements for
owner-relative content references. `entry_relation_issues` exposes typed,
owner-specific failures using the same rules as document diagnostics.
Markdown `mant:entry` authoring rejects invalid fields through this shared
validator; explanation collection is a separate consumer of validated facts.

For semantic definitions, the engine derives a role-qualified identity from the complete semantic name after source-specific parsing. Formatter navigation tags remain page-local anchors but do not become semantic IDs merely because their spelling is short or collides with a command. Collisions use a deterministic fingerprint of semantic identity and content rather than a source-order suffix; unrelated sibling insertion and reordering therefore cannot silently redirect an ID. Section and entry allocation are independent. These IDs identify the same logical content within one current document, but an independently updated host manual can change or remove that content, so consumers rediscover before reuse.

`SemanticIndex` is a rebuildable sidecar over these content definitions. It
maps each owner to one `SemanticEntry` and retains nested ownership such as
command → option → value. The same `EntryKind` is used by facts and projections;
Markdown `role=` remains an authoring spelling, not a second public IR enum:

| Markdown `role=` | `EntryKind` |
| --- | --- |
| `option` | `parameter { parameterKind: option }` |
| `marker` | `parameter { parameterKind: marker }` |
| `operand` | `parameter { parameterKind: operand }` |
| `command` | `command` |
| `configuration-key` | `configuration-key` |
| `environment-variable` | `environment-variable` |
| `variable` | `variable` |
| `value` | `value` |
| `term` | `term` |

Option, marker, and operand are parameter families at every IR layer.

Each `SemanticEntry` contains:

| Field | Meaning |
| --- | --- |
| `id` | Current-document semantic identity |
| `kind` | Role-aware index category shown above |
| `names` | Exact selectable spellings derived from validated facts |
| `aliasGroups` | Explicit owner-local equivalence groups, copied from facts; shared names imply none |
| `aliasOf` | Explicit same-document relationship to another independent entry; no inherited content, children or domain |
| `case` | Alias matching policy |
| `forms` | Complete author-written terms, including argument layouts |
| `documentTargets` | Explicit cross-document destinations carried by linked terms |
| `children` | Entries semantically owned by this entry |
| `valueDomain` | Optional value-space evidence |

The entry `id` is also the document-local address of its authoritative item. Names answer “how can this content be selected?”, forms answer “what usage did the source display?”, and document targets answer “which other document did an explicitly linked term name?”. Consumers must not reconstruct one field from another. In
particular, a complete form such as `[+-]O [shopt_option]` is not necessarily a
safe selector. Description links remain ordinary content links rather than entry destinations.

`EntrySummary` describes a scope without materializing individual entry nodes:

| Field | Meaning |
| --- | --- |
| `direct` | Entries directly owned by the document root or section |
| `descendants` | Entries nested below those direct entries |
| `forms` | Complete authored forms across direct and nested entries |
| `byKind` | Recursive counts grouped by `EntryKind` |

`ValueDomain::Choices { exhaustive }` says child entries are observed choices
and records whether the source declares the set complete. Explicit choices
must have nonempty direct semantic children consisting only of values;
`DefinitionItem::has_value_choices()` uses the same ownership walk as the
semantic index, and shared validation reports `ir.invalid-entry-choices` for
incompatible producer claims. `EntrySet` holds a
restricted source-neutral document reference, selected entry kinds in that
document, and an optional source span for the declaration. The span preserves
authored relationship order; it is not part of the destination identity. The
reference is resolved against the referring logical address only at the
engine/protocol boundary. Producers must not infer a cross-document set from
prose.

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
semantic entries use paths such as `2.3/e4/e2`. These coordinates are not
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
mant-ir = "^0.11.0"
```

Prefer constructors and visitors from the crate over recursively rewriting public fields by hand. Use `visit::Visit` or `visit::VisitMut` for whole-document passes and run validation after transformations that can affect identities or links.

## Stability

The crate has its own semver, independent of the `mant` executable and native wire protocol versions. Pre-1.0 Rust API evolution may require downstream source changes. The stable structured promise is the exact schema identifier exposed by `mant-protocol` and emitted by the executable, not semver inference from the IR crate alone.

## See Also

[mant(1)](mant.md), [mant-protocol(5)](mant-protocol.md), [mant-markdown(7)](mant-markdown.md), and [mant-roff(7)](mant-roff.md)
