# mant-ir

## Name

mant-ir — source-neutral document model shared by ManT parsers and in-process consumers

## Description

`mant-ir` is the normalized in-memory representation produced after Markdown, man, mdoc, and tldr input has been parsed. It lets the query engine, terminal UI, renderers, and indexes operate without depending on source-specific syntax trees.

The Rust crate is a library contract for trusted semantic components. It is not the versioned structured integration contract, and its Serde representation is not, by itself, a compatibility promise. Host and process consumers should use the versioned projections and generated schemas described by [mant-protocol(5)](mant-protocol.md).

## Pipeline Position

```text
Markdown ─┐
man/mdoc ─┼─> mant-codec ─> mant-ir ─┬─> mant-ui
tldr ─────┘                          ├─> mant-render (body text)
                                    └─> mant-query ─> mant-protocol DTOs
```

Source parsers retain syntax-specific facts only until they can be expressed as shared document semantics. Protocol projections may omit internal data, add schema discriminators, or reshape fields for a stable host or process boundary.

`mant-loader` acquires source input; `mant-engine` validates and composes loading
and queries. `mant-render` also formats already materialized protocol results.
Shared IR geometry and original text coordinates remain independent of rendering
styles, actual terminal wrapping and viewport state. Reusable link addresses
come from `LinkTarget::to_uri`, not a renderer's human-readable label.

## Document

`Document` is the root of a normalized manual body. It contains:

| Field | Meaning |
| --- | --- |
| `parser` | Producer name and version, when known |
| `source` | Source format and original path |
| `meta` | Native bibliographic title, manual metadata, names, and alias target |
| `heading` | Optional original visible document heading with full inline content |
| `fragmentAliases` | Exact source fragments resolving to the normalized document root |
| `diagnostics` | Recoverable parser and validation findings |
| `blocks` | Content before the first heading |
| `sections` | Recursive content-section tree |

`DocumentMeta.manual_section` is a native manual category such as `1` or `3p`. A `Section` is a heading-backed content node. The two concepts are intentionally distinct.

`SourceFormat` is one of `man`, `mdoc`, or `markdown`. Embedded and cached tldr pages are stored beside the main document in `ResolvedContent`, not disguised as document sections.

## Sections

A `Section` contains a normalized document-local `NodeId`, optional exact `fragmentAliases`, an authoritative `Heading`, blocks, child sections, optional source coordinates, and source-requested spacing before the heading. `Heading.content` uses ordinary Inline nodes, preserving links, styles, anchors and hard breaks; optional `Heading.inlineLayout` carries owner-local row corrections, and `Heading.source` records actual heading provenance. Plain titles are derived, not independently mutable fields. Depth is derived from tree position rather than stored as mutable metadata.

The virtual ID `document-overview` addresses the original document heading and root blocks before the first section, including a heading-only document, and may carry exact source aliases from the extracted Markdown H1. Its content moves to `Document.heading`; it is not discarded or duplicated in metadata. Native bibliographic titles remain metadata and do not manufacture a document heading. IDs are unique only inside one document and may change when the source changes. Consumers should rediscover them through the current index or outline rather than persisting them globally.

## Blocks

The block union preserves structures that matter across renderers:

| Variant | Semantics |
| --- | --- |
| `paragraph` | Filled inline flow |
| `preformatted` | Literal flow with an optional language |
| `list` | Bullet, ordered, or plain items containing blocks |
| `definition-list` | Terms and block descriptions with item layout, source, and optional `entry` facts |
| `table` | Rows and block-capable cells with spans, alignment, and optional `columnPreferences` |
| `equation` | Parsed expression and its checked readable text projection when available; legacy producers may supply text alone |
| `vertical-space` | Executed blank terminal rows |
| `thematic-break` | Semantic separator |
| `unsupported` | Visible source preserved when no lossless semantic lowering exists |

`LayoutHint` carries portable presentation facts: signed indentation in display cells and spacing rows before a block. `indentColumns` is relative to the actual parent's content origin, not a cumulative source margin. Consumers compose it exactly once, retain signed intermediate origins, and bound padding only at a visible leaf. Reparenting changes the moved root's relative offset, never its already-relative descendants. Source unit expressions and formatter state do not enter this closed object; unknown fields are rejected. It is not a general-purpose CSS or roff device model.

Paragraph `continuationIndentColumns` is an additional signed displacement from its first-line origin (default zero). Each logical hard row has two signed origins: its first visual line and its soft-wrap continuations. With parent origin P, block indentation B, hanging displacement H, and row correction R, the first hard row uses P+B+R and P+B+H+R respectively. Later hard rows use the paragraph's continuation policy before applying their own correction. For P=0, B=2, H=4 and R=1, the first visual line begins at column 3 and its wraps at column 7. A row correction changes both origins without cancelling hanging layout. Later sibling blocks inherit neither H nor R. A terminal applies wrapping at its current width; unbounded text preserves hard breaks only.

The TUI reserves a readable content area when indentation would leave fewer
than 16 columns, or fewer than half the columns in a narrow viewport. It
reduces the line's first and continuation presentation origins by the same
amount, then bounds them at zero. The logical origins and IR remain unchanged;
link hits, selection and search positions follow the resulting displayed cells.
At sufficient width, these logical origins are presented without that reduction.

Block `spacingBeforeLines` is already resolved by the producer: zero means a tight boundary, including when omitted from JSON. It is not an invitation for a frontend to supply paragraph spacing. Independent `VerticalSpace` blocks add their already-resolved blank rows to that boundary, including repeated equal blocks; each executed row has one IR consumption point. Empty anchors and transparent containers do not reset the boundary. Presentation bounds each accumulated gap at 4096 rows, independently of literal blank lines inside text. Native lowering reports `manual.vertical-spacing-limit` when this loses requested spacing. Definition-item optional spacing is different: absence inherits list compactness, while explicit zero suppresses that default.

A definition description starts at its resolved `layout.bodyIndentColumns` relative to the label origin (generic default: four cells), before applying each child's layout. `minTermGapColumns` controls minimum separation after a run-in label (default: one). `DefinitionItem::inline_description()` identifies the first paragraph or literal fragment that may share the term's line when the producer records a shared row; explicit leading spacing prevents that presentation. A literal fragment shares the row only with native continuation evidence, so ordinary no-fill input remains on separate lines. The first line clears the displayed label; hard and wrapped continuation lines, later paragraphs, nested blocks and code use the structural body origin, not the label's width. Separate source term roots retain their original lines rather than acquiring invented commas. Native continuation normalization, plain text, and the TUI share this distinction. Markdown expresses definition ownership through its own block syntax. Inferring a semantic definition from separate source paragraphs preserves their line boundary; it does not authorize run-in presentation.

`DefinitionItem.headBodyRelation` records the content boundary and defaults to
`{"type":"separate"}`. A `shared` object contains only `wordBoundary`
(`joined` or `separated`). `DefinitionItem.layout: DefinitionLayout` carries
the independent `bodyAlignment` preference (`after-term` or `indented`, default
`indented`) and geometric spacing. Joined words receive no invented separator;
accepted authored label padding remains content. Separated words retain at least the
minimum term gap. `after-term` uses that gap, while `indented` also considers
the preferred first BODY origin. Alignment has no effect on a separate row;
changing it cannot establish sharing or change the word boundary. Joined words
ignore alignment and the minimum gap while preserving those layout values.
This choice does not change continuation origins or semantic ownership.
Plain text and TUI use the same bounded gap
calculation without guessing whether a native field flushed. Optional `spacingBeforeLines` defaults to
inheriting list compactness. Explicit zero spacing is preserved and does not
mean inheritance. Missing layout and `{}` have the same default; `layout:null`,
unknown fields, the retired `layout.headBodyRelation` and shared
`headBodyRelation.bodyAlignment`, former string relations (`separate`, `run-in`,
`joined-no-space`, `flush-at-body`), the former `layout.inlineTerm`, and the former top-level `inlineTerm`/`spacingBeforeLines` fields
are rejected. Canonical output omits empty layout but retains
`"layout":{"spacingBeforeLines":0}`. Semantic annotation never changes layout.

Canonical Markdown preserves joined paragraph words instead of adding a
readability separator. Joined HEAD and BODY phrasing uses one Markdown inline
encoding context, so adjacent styles and code delimiters do not become visible
text on readback. Their IR roots, name bindings and link ranges remain separate.
The first effective BODY block carries its resolved leading boundary across
empty and destination-only roots. A separate prose row becomes a hard break;
positive leading `VerticalSpace` or block `spacingBeforeLines` becomes a
paragraph boundary. Markdown may simplify a positive vertical distance to one
blank line, but never joins that prose back onto the HEAD row. A literal
description remains a separate fenced code block in the corresponding list
item, preserving its payload and type. The
fence's formatting lines are export syntax, not extra hard rows in the IR;
Markdown semantic readback does not promise native DefinitionItem identities.

`ListItem.layout: ListItemLayout` has optional `spacingBeforeLines` with the same inheritance and closed-object rules. An explicit value precedes the entire marker and body, including a display or nested list as the first block; it is not extra spacing inside the body. This preserves per-item native paragraph distance without splitting a list or changing its entry paths.

Lists contain block-capable items so nested lists and displays do not flatten into prose. Each definition term is a `DefinitionTerm { content, inline_layout }` with one authoritative inline tree and optional row corrections; its JSON object uses `content` and `inlineLayout`. `DefinitionItem.terms` is an array of these objects, not an array of inline arrays. Descriptions contain blocks. Table cells likewise contain blocks even when a source parser currently produces a single paragraph.

An equation expression retains the parser's box kind, font, position operator,
fences, decorations, argument counts, and ordered children. Default font size
and an unbounded grammar argument count are absent rather than exposed as
parser-specific sentinel integers. Matrix children
retain their columns and rows. `EquationExpression::readable_text()` supplies
the shared, source-neutral text projection; a block's `value` is a compatibility
cache and must agree with that projection whenever `expression` is present.
Validation reports a content-coverage error for a mismatch. This projection is
readable text, not a math layout or a second independently editable source.
The roff producer counts JSON container depth from the final document position,
including enclosing sections, lists, definitions, tables, and styled inlines.
When a complete formula exceeds the remaining reader budget, the affected
subtree becomes a text leaf retaining its complete readable text.
`summarizedOperandGroup` records whether its parent must still group the operand;
`manual.equation-structure-depth-summarized` reports lost structure as
`semantic-coverage`. Deep surrounding document containers are likewise reduced
to readable text with `manual.document-structure-depth-summarized`, including
when no formula is present. The producer checks the final query-shaped JSON
against the reader's recursion limit and retains a flat readable body if an
unusual remaining structure still exceeds it. These reductions leave
`contentComplete` true and `semanticsComplete` false. If the owned tree was
already truncated, its separate `content-coverage` diagnostic still reports
missing source content.

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
choices, and never copies the final description into earlier items. It is only
produced for source-backed declaration roles with a concrete subject (such as
options, commands, variables, or configuration keys). Under a generic context,
generic terms and values remain individually addressable but never gain group
context merely because a later sibling has prose: generated indexes and
taxonomies frequently use that layout. A group survives IR serialization
without native parser pointers.
Excerpts retain/rebase only whole groups; a single-owner excerpt may therefore
remain empty while explain separately returns useful group context. Renderers
ignore the annotation for full-document layout. Native producers supply bounded
source evidence; ordinary Markdown lists are not automatically grouped. IR
validation checks structure, not roff syntax or the applicability of each
sentence to each member.

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

`Table.column_preferences: ColumnPreferences` carries optional reading preferences,
serialized as `columnPreferences`. Its closed object has `widths`, an ordered
array of preferred content widths excluding the gap; `gapColumns`, the preferred
inter-column blank count; optional `advanceLimitColumns`, limiting one positioning
advance; and optional `extraWidthColumns`, the content capacity for fields beyond
the declarations, excluding any added gap. Those fields share the preferred
origin after all declared fields. Every supplied number is an integer from 0
through 65535. These are display-cell preferences, not final viewport coordinates
or another cell body.
The source-neutral default is `{ widths: [], gap_columns: 2,
advance_limit_columns: None, extra_width_columns: None }`. Empty widths select
content-derived layout, where a custom gap still applies. Nondefault advance and
extra-field constraints remain in canonical JSON even with empty widths, but
apply only to declared-field layout. Explicit zero is retained;
`Some(0)` differs from an absent constraint. Optional constraints accept null as
absence and serialize by omission. The object and its widths/gap cannot be null.
Missing preferences and `{}` use the same default, and canonical output omits
only that complete default. Positional arrays are rejected even when empty.
Unknown or duplicate fields, the retired
`columnWidths` array, and mixed old/new shapes are rejected.

The producer resolves source escapes and source-specific gap/advance rules.
For mdoc column lists it supplies measured widths, a 4/3/1-cell gap, a 256-cell
advance limit and extra-field width 10. Generic consumers use the supplied
preferences without interpreting a macro or inferring a gap from the width count.
They measure and wrap at the actual viewport before collision and placement.
More than 256 declared or actual columns, incompatible spans/rules, and signed
descendant origins use the existing source-order fallback; every cell keeps its
content and owner. The independent 4096-cell generated-padding budget uses that
fallback when exceeded, retaining every later cell as well as the cells already
prepared. It does not impose a source advance limit. ANSI decoration never changes
measurement. TUI resizing may stack cells while retaining links, anchors, search
ranges and selection coordinates.
Ordinary trailing separator spaces do not force a field wrap, but preserved
output spaces still advance the visible cursor. A later cell never starts
before the preceding cell's actual output ends.
Completed blank rows are distinct from an open trailing line: a later cell
cannot reuse a row already completed by the formatter. CLI and TUI placement
consume the same row-completion facts, including empty final fields.

`TableCell.break_after` (`breakAfter`) closes the current physical data row after
that cell, before the next cell starts. The row may be occupied by this or an
earlier cell, or be an empty row already present in the table structure. Its
default is false and canonical JSON omits false. It adds no body scalar or
additional empty row: an empty first cell with this boundary retains its existing
data row before the next cell. Navigation-only carrier rows remain nonprinting.
An inline hard break may leave an open tail for the next cell; this closure
prevents reuse of that tail. Already completed empty rows retain their existing
single consumption point.
The closure consumes a resolved gap once before the next cell begins a new
budget. Unclosed cells in an origin-preserving stacked row share the active
parent budget; ordinary column cells remain independent. The whole data-row
boundary also consumes pending gaps, even when its content roots are empty.

`TableGrid` supplies shared sparse logical-column coordinates for table consumers. Each row has a closed `kind`: `data`, `horizontal-rule`, `double-horizontal-rule`, or `layout-rule`. A layout rule retains one `horizontal` or `double-horizontal` strength per logical column, including mixed `_`/`=` layout rows. Data-row cells also have a closed `kind`: omitted/`text`, `horizontal-rule`, `double-horizontal-rule`, `isolated-horizontal-rule`, or `isolated-double-horizontal-rule`. These cell roles preserve partial layout rows and tbl data-cell rule tokens without inventing text blocks; a rule cell must have no ordinary block content. A data row with no cells is an intentional physical blank row; it is not interchangeable with a rule. Horizontal spans omit covered cells; vertical continuations retain explicit empty cells in subsequent rows, while `rowSpan` remains on the content owner. Covered content is never repeated. Callers can request dense column slots with an explicit budget, so large span values need not allocate a dense grid.

CLI text and TUI stack cells in source order when a table's composed origin falls outside the final padding bounds, a cell subtree contains a negative relative displacement, or a descendant origin would cross those bounds. The check includes nested containers, list markers, definition bodies, and continuation lines. In this fallback, ordinary block rendering composes the real parent origin before clipping visible text, preserving outdents, links, anchors, and hard lines. Rendering cells at local column zero and translating them afterwards would lose that geometry. Tables with nonnegative origins and displacements that remain within the bounds retain the ordinary column layout unless expanding multiline cells across dense logical slots would exceed the renderer's bounded physical-slot budget; text output then uses explicit `column N:` rows so every physical line remains visible without width-by-height amplification.

## Inline Content

The inline union contains:

| Variant | Semantics |
| --- | --- |
| `text` | Plain visible text |
| `strong` | Strong importance or source bold semantics |
| `emphasis` | Emphasis or source italic semantics |
| `code` | Literal inline text |
| `equation` | Parsed equation in its original position between neighboring text |
| `link` | Visible children plus a typed destination |
| `anchor` | Zero-width document-local destination |
| `line-break` | Pure explicit hard break inside one flow; no layout fields |

`Inline::LineBreak {}` serializes as `{"type":"line-break"}`. The retired
`indentColumns` field is rejected, including zero. A break remains one original
text scalar regardless of presentation.

### Owner Row Layout

Paragraph and Preformatted blocks, document and section Headings, and each
DefinitionTerm may carry `inlineLayout` beside their original `children` or
`content`. Its `rowHints` contains sparse `{row, indentColumns}` objects. `row`
is a zero-based logical hard-row position within that owner; `indentColumns`
is a signed relative display-cell correction. Missing rows use zero without
inheriting a preceding correction. Ordinary indentation and hanging layout
remain structural, so ordinary Markdown paragraphs and lists need no hints.
Strong, Emphasis and Link wrappers share their owner's row counter; anchors
do not advance it. Each LineBreak and each actual newline in Text, Code or
an equation's readable text closes the current row and opens the next.

| Original content | Addressable row positions | Rows closed by hard breaks | Open tail |
| --- | --- | --- | --- |
| `[]` or anchors only | 0 | 0 | No body row is requested |
| `A` | 0 | 0 | Row 0 contains A |
| `A\n` | 0, 1 | 1 | Row 1 is empty and open |
| `A\n\n` | 0, 1, 2 | 2 | Row 1 is a completed empty row; row 2 is open |

An empty owner's row 0 and an open trailing row may carry a bounded hint, but
the hint adds no glyph, blank row or clickable region. An explicit empty
literal text leaf remains authored content. Standalone literal output settles
its authored trailing row at the container boundary; a table fragment can
instead let the following cell occupy its open tail. A completed empty row
cannot be reused. `VerticalSpace` is separately owned completed block spacing
and does not advance an inline owner's row counter.

Actual owner decoding requires strictly increasing, unique, in-range `row`
values and integer displacements from -65535 through 65535. Null layouts,
unknown or duplicate fields, and more than 4096 hints in one owner are rejected.
Zero corrections are validated before canonical output omits them; owners
also omit empty or all-zero `inlineLayout`. Roff production retains at most
65536 nonzero hints across a document, also respecting the per-owner limit.
Excess optional hints are omitted with `layout.row-hint-budget` and diagnostic
impact `none`; authoritative text and content completeness are preserved.
65536 is a producer budget, not an additional whole-document JSON input limit.

Consumers compose each correction with both signed visual origins, then bound
padding only at the visible leaf. Text, located explanations and TUI use the
same owner facts. Ordinary Markdown paragraph, heading and definition-term
phrasing omits row hints. It retains authored text, NBSP, hard rows and the
author row-edge whitespace; hints cannot create link occurrences or
word separators. Fenced preformatted payload and flattened table output retain
positive corrections as ASCII spaces before occupied rows. Negative corrections
produce no spaces, and empty rows or an open tail do not acquire text from hints.
These generated literal spaces become payload on Markdown readback, a recorded
formatting difference from the original IR. Artifact and search coordinates
address those actual exported bytes; there is no reverse map from generated
spaces to authored scalars. Original owner content and name/link bindings stay
unchanged. Ordinary row-edge ASCII spaces and tabs are protected as character
entities, decoding to the original characters rather than non-breaking spaces.
Layout metadata cannot authorize trimming authored whitespace. The former
hint-driven multi-Link formatting split is retired; a retained multiline label
keeps one complete wrapper and its original owner.

Inline children are the only visible body. Reading, Markdown export, search,
semantic recognition and TUI copy traverse the same accepted glyphs, styles,
links and hard boundaries. There is no replacement spelling or hidden URI
suffix. Source-specific macro execution is completed before constructing this
source-neutral IR; ordinary Markdown links do not acquire roff-generated text.
Entry forms and name-binding paths address the final accepted inline tree.
A selected partial name keeps its style and destination ancestry without
expanding into the complete label. The retired `portable-display` wire shape
and `display` fields on current variants are rejected by the unreleased v0.12
contract.

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

An accepted inferred head with nonzero paragraph continuation displacement
(`H`) retains its original `Paragraph` as block zero of a marker-free plain
`ListItem`. Its complete form references that block. The original block origin
(`B`), `H`, sparse row hints, source span and description origins remain intact;
zero `H` retains the existing `DefinitionItem` shape. When presentation evidence
does not establish a semantic owner, the original head and description run stay
ordinary content. Preferred names and collision-free generated IDs retain their
spelling. Only a duplicate preferred ID or a reserved author anchor requires a
collision fingerprint: changing the owned content shape from term to paragraph
can then change the generated ID suffix. That suffix change does not change
the owner coordinates, names, scalar bindings, authored anchors or links.
Identification is stable on a second pass over the retained owner shape.

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

`content_entries` returns borrowed `ContentEntry` locations in semantic preorder, with validated names, semantic ancestors and exact physical block/item coordinates. Transparent lists, definitions and table cells retain their physical paths without consuming semantic ordinals. `content_entry_locations` follows the same owner walk without validating names, so empty or invalid names cannot hide an addressable owner. Both scans borrow original content; serializing a location measures the same single-owner block as its explicit `content()` copy, preserving list numbering and layout without first cloning the body. The location's fields are read-only accessors, not independently constructible coordinates.

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

`ResolvedContent` is the in-process handoff from the loader and complete engine workflows, or a caller-authored snapshot. It carries a display label, optional exact address, optional document body, and optional tldr page. Pure queries borrow it; the TUI can share immutable snapshots through `Arc` without cloning their document bodies. Process clients receive versioned protocol projections; MCP tools present focused projections as compact text or CommonMark.

## Source Coordinates

`SourceSpan` uses one-based lines and columns for diagnostics. When a parser can provide exact offsets, `byte_range` is a half-open range over UTF-8 bytes in the original input and is the canonical machine-facing coordinate.

Native libmandoc nodes generally provide line and column positions but not exact byte ranges. Markdown lowering preserves byte ranges. Rendered search coordinates belong to the independent `mant.markdown/v1` projection and must not be confused with input spans.

## Content positions and reference traversal

`ContentLocation` identifies original inlines under three closed roots: document heading, section heading, or root/section content. Zero-based section indices and typed `ContentBlockStep` transitions distinguish blocks, ordinary list items, definition descriptions and table cells. `ContentInlineRoot` then identifies paragraph/preformatted inlines or a particular definition term, followed by an inline child path. Empty paths select a whole inline container; a link occurrence always has a nonempty path to its actual `Inline::Link`, including links with no visible label.

Every transition is checked against the actual container and bounds. `EntryOwnerLocationRef::map_slice` maps a valid owner-local `EntryContentSlice` to the original document position; its optional UTF-8 leaf range is validated but does not become a node identity. Invalid and oversized paths return no target, never a label-based guess. Source bytes, IR leaf bytes, projected Unicode scalars and terminal cells remain separate coordinate domains.

`ContentLocation::inline_content(document)` and
`EntryOwner::inline_content_root(root)` borrow an `InlineContentRef` containing
the complete owner's original content and layout. A checked child path does
not create another row coordinate space. Whole-owner copies retain their
layout; a partial scalar selection that becomes a new root uses
`InlineContentRef::sliced_layout` to rebase its row hints, including an open
tail. Slice coordinates count Unicode scalars, not UTF-8 bytes or display
cells. `InlineContentRef::unpositioned` provides an explicit text-only view.

For original inline text, `project_content_slice` converts the same checked owner-local slice to a `RootTextRange` of Unicode scalars. `inline_scalar_len` counts an authored hard break as one scalar and wrappers or anchors as zero additional positions. These shared IR operations do not infer names, apply styles, or include renderer-generated padding; query response coordinates are mapped separately into the returned payload.

`scan_reference_scope` visits the selected document, overview, section, block or item under `ReferenceScanLimits`. The callback borrows the original target, label, position and nearest content/attached-semantic owner. It receives one shared `ReferenceWorkBudget` for optional work such as `reference_form_associations`; exhausted work stays exhausted even at the final occurrence. Default scanning is capped at 250,000 steps, depth 256 and 8 MiB of inspected target/form/label bytes; hard ceilings are 1,000,000 steps and 32 MiB. Summary traversal need not inspect or copy labels. Retained positions have an independent 8 KiB encoded-size limit. Callers must separately bound their retained records and labels.

Repeated targets remain separate occurrences. Valid form associations point back to those original occurrences rather than producing extra links; invalid or incomplete form validation returns no partial associations. An attached semantic owner is not a claim that all its metadata is valid. Entry-set relationships remain separate from visible links. No filesystem, catalog, URI opener or parser is invoked by these APIs.

These addresses are valid within the actual loaded snapshot and rebuild after IR serialization. They do not promise cross-edit or cross-call stale detection. Explanation responses reuse the block/item/cell traversal but explicitly root their positions in the returned content, not in the whole document.

## Diagnostics

Diagnostics have `style`, `warning`, `error`, or `unsupported` severity, a required `impact`, an optional stable code, a message, and an optional source span. They describe recoverable source findings; fatal I/O, decompression, parsing, request, or transport failures remain ordinary errors outside the document.

`DiagnosticImpact::SemanticCoverage` (`semantic-coverage`) records rejected or incomplete semantic declarations, facts or extraction coverage. `ContentCoverage` (`content-coverage`) records known loss of visible source content, including a rejected or truncated structural transfer. `None` (`none`) leaves both coverage signals intact. Producers set the effect explicitly; consumers use `semantics_complete` and `content_complete` without matching source-specific codes or guessing from severity. Known content loss also makes semantic coverage incomplete. Shared IR validation supplies the appropriate effect for structural and semantic failures. Custom producers must retain these findings after validation. An absent or unknown `impact` is rejected during deserialization, not silently treated as complete. These signals do not claim exhaustive discovery, rendering fidelity or complete behavioral knowledge.

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
mant-ir = "^0.12.0"
```

Prefer constructors and visitors from the crate over recursively rewriting public fields by hand. Use `visit::Visit` or `visit::VisitMut` for whole-document passes and run validation after transformations that can affect identities or links.

## Stability

The crate has its own semver, independent of the `mant` executable and native wire protocol versions. Pre-1.0 Rust API evolution may require downstream source changes. The stable structured promise is the exact schema identifier exposed by `mant-protocol` and emitted by the executable, not semver inference from the IR crate alone.

## See Also

[mant(1)](mant.md), [mant-protocol(5)](mant-protocol.md), [mant-markdown(7)](mant-markdown.md), and [mant-roff(7)](mant-roff.md)
