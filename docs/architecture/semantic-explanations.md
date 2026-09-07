# Semantic entries and explanation design

This document explains content ownership, name evidence and bounded explanation
collection. Normative authoring syntax lives in
[mant-markdown(7)](../manuals/mant-markdown.md); current query and wire contracts
live in [mant-protocol(5)](../manuals/mant-protocol.md), and public IR types in
[mant-ir(7)](../manuals/mant-ir.md).

## Content is authoritative; annotations add facts

Ordinary Markdown owns visible words, punctuation, order, list structure,
paragraphs, hard breaks, code and links. ManT semantic comments must annotate
that content, not rewrite it into a different presentation model. The intended
invariant is that enabling semantic interpretation of the same parsed source
does not change its ordinary content projection, apart from non-visible
navigation markers and separately reported diagnostics.

This is not a pixel-equivalence or source-byte round-trip promise. Terminal
width, theme and documented safety handling remain presentation concerns.
Nor does it permit deleting comments and reparsing the result: comment
placement and blank lines participate in
[CommonMark block structure](https://spec.commonmark.org/0.31.2/#html-blocks).
Compare the same original event structure with annotation interpretation
enabled and disabled, not two differently structured source documents.

The model attaches common entry facts to both ordinary `ListItem` and
native `DefinitionItem` owners. Keep one authoritative content tree; derive
the semantic index and query views from it. Do not store independently mutable
original and semantic copies of the body. Read-only name/form bindings must
refer to validated positions in the final IR, not offsets into a discarded
parser buffer. Every consumer must support both owner kinds, including
nested entries, navigation, scope links, value domains and TUI anchors.

Producers carry the grammar's accepted names and original visible byte ranges
together. The binding mapper projects that evidence into final-IR slices; it
must not guess the names again with a different delimiter grammar. Native
inference and explicit Markdown admission may intentionally have different
rules. Preserve whitespace and UTF-8 offsets across transparent style wrappers.

Original item source positions survive annotation removal and structural
conversion. Merging heads transfers the first head's source along with its
content; unknown sources stay unknown and end ranges must not be fabricated.
Source positions are provenance, not persistent identity.

Share the content-owner boundary, not a universal traversal order. Direct-child
entry/choice traversal crosses unannotated containers and stops at annotated
owners, including invalid ones. Whole-document link traversal instead retains
source order throughout the tree. Validation and derived indexes borrow one
immutable document; invalid facts never transfer its content to another owner.

Metadata must not invent a documented name absent from the visible definition
head, move children, generate visible choices, or replace a description.
Unknown or invalid facts must not erase the underlying content. The implemented
metadata handling keeps invalid declarations non-visible and reports them through
diagnostics, using the original parser events without reparsing. This applies to semantic comments,
not a promise that existing heading attributes or embedded TLDR directives
are interpreted by every ordinary Markdown viewer.

## Names, shared descriptions and aliases are different

A content entry may describe several subjects. Its names identify that
content; sharing a description does not establish behavioral equivalence.
Facts and projected `names` fields expose selector spellings, not
proof that the corresponding options can be substituted for one another.
Complete forms preserve documented usage, not an executable argv grammar.

The implemented model separates documented names, explicit same-owner
`aliasGroups`, and explicit same-document `aliasOf` relationships between
independent entries. Markdown declares these fields in a closed `mant:entry`
JSON comment attached to an explicitly declared item. In particular, grouping `-S`, `--since`, `-U`, `--until`
in one item must not assert that since and until are equivalent. Explicit
groups could record those two pairs without splitting or duplicating content.

Relationship members must bind to visible names, preserve case and have
unambiguous ownership. Reject overlapping groups, dangling references, cycles
and ambiguous multi-subject targets; never use first/last-wins repair.
Object-level parse failures and field-level relationship failures need explicit
atomicity rules. Relationships do not rewrite parentage, inherit value domains
or establish argument count, option clustering or runtime parameter scope.
Fragment aliases, manual redirects and typed document links remain separate
concepts with their existing authority boundaries.

## Explanation is evidence collection, not navigation

The independent Rust `explain_query` collector serves CLI, request JSON and
MCP. `select_explanation` is a default-budget convenience returning the same
`QueryExplanation`; `select_excerpt` retains strict navigation.

The explanation collector returns relevant evidence from an immutable
document snapshot: documented names, authored forms, related IR content and
validated explicit relationships. Each result retains its location, context
and matching basis. Multiple independent or complementary records are normal;
never pick the first as the uniquely correct interpretation or merge distinct
owners merely because their names or descriptions resemble each other.

Keep exact node/read/outline-root navigation separate. A paragraph match is
supporting content, not a newly manufactured semantic entry. Relation-derived
evidence must be distinguishable from a direct name match. Define literal
matching, name boundaries, case policy, evidence ordering, pagination and
content budgets before implementing fallback retrieval; do not silently turn
explain into fuzzy search or arbitrary shell/natural-language interpretation.

Use a dedicated explanation result rather than disguising multiple records as
a unique excerpt. Valid readable queries with one, many or zero owners succeed
(CLI exit 0); `outcome` distinguishes evidence from no-evidence before paging.
Partial source failures retain available evidence and scope coverage. All
sources failing is a source error; invalid requests fail before loading.

The request is a literal of at most 512 Unicode scalars (no controls), plus
`ExplanationOptions`: 1–256 records (default 50), a zero-based offset, and
1 byte–4 MiB of copied forms/facts/previews/body payload (default 1 MiB). The collector
indexes at most 10,000 matching owners per document, follows at most 4,096 explicit edges per document
and retains chains of at most 32 edges. These bounds report independent
`truncation` fields. Oversized owner bodies are omitted atomically, with real
outline positions retained for strict reads; they are never partial valid IR.
Single-document and scope requests share the same `class-then-source` contract:
direct entries, explicitly related entries, mentions in other entries, then
ordinary content mentions. Within a class, scope uses document BFS order and
then original IR owner/block order. A borrowed collection plan completes
classification and global ordering before applying the one offset/limit and
copy budget. Its bounded priority pool replaces lower-priority mentions when
later direct/related owners would otherwise be lost, reporting candidate
truncation on every discard. No body is cloned merely to sort or skip it. These are payload
copy limits, not a bound on serialized envelopes and metadata. MCP character
paging slices the completed canonical presentation independently.

Direct name and complete-form evidence follow the owner's case policy;
ordinary paragraph, preformatted, equation and preserved-source support uses
case-sensitive literal token boundaries. It does not expand names, shorten
options or infer relationships from punctuation/prose. Root/section support
has a final-IR block/item/cell coordinate, not a manufactured entry ID. One
owner has one exclusive class and all retained bases, even when its copied
metadata is omitted or it has no names; parent and child remain separate. Explicit
aliasOf edges may be traversed in either direction to collect independent
owners, with declaration IDs recorded. This neither changes relation direction
in the IR nor inherits the remote owner's value domain. Quick-reference-only
content currently has no full-document semantic evidence; no-evidence is not a
claim that its command examples contain no useful information.

`ScopeExplanation.documents` contains BFS source reports without nested
queries, cursors or bodies. Its unique flat `evidence` page references those
reports using `documentIndex`. Counts retain all four classes, including zeros;
each total/returned column sums to the response and the source contributions.

Literal collection retains the actual matched block and range. Materialization
copies at most two representative windows, at most 1024 Unicode scalars each,
preserving a complete query match. `resolve_explanation_block` resolves their
absolute final-IR paths; source spans belong to the matched block, not the
whole entry. Safe text projection precedes scalar-range calculation. Facts,
windows and atomic original body consume the same budget, in that order;
window clipping and budget omission remain independent. Text/Markdown/MCP
render full direct/related content but only preview windows for mentions.
For example, GCC's `-Q` can mention `--help` without becoming its alias or
another direct definition. This does not mutate the full document renderer.

Validation findings, available source coverage, result truncation and unknown
fields are separate dimensions. Neither `semanticsComplete` nor a clean audit
proves complete name recall or correct knowledge of an executable's behavior.
No evidence collector may execute examples, inspect real environment values,
follow external URLs or enlarge the existing local source/query authority.

Each explanation builds one `mant_ir::DocumentValidation` for its immutable
document. Relationship expansion and final completeness diagnostics consume
the same index and typed findings. The sidecar borrows the original document,
does not accept independently supplied indexes, and never caches across calls.
External IR producers receive the same complete checks as built-in parsers.

## Lowering boundaries

| Source evidence | Required interpretation |
| --- | --- |
| Linked bold `-a, --all` | Traverse visible link children with the same style-aware name rules |
| man `.BI -L dir` | Bind `-L`, retain the adjacent parameter in the full form |
| A bold command invocation containing options | Styling alone does not make the entire run a command name |
| Declared Markdown heads with `:` / `—` / `\|` | Attach facts without replacing content or changing delimiters |

Implement these through a shared style-aware traversal and finite role-specific
lexical rules, not tool-name cases or progressively larger string templates.
Preserve explicit native role evidence at definition owners; a role-bearing
inline mention in prose does not itself define an entry. Do not infer alias
relations from English phrases such as “same as”. Unknown evidence stays
unknown while the source remains readable.

Separate fidelity normalization from semantic inference before removing
heuristics. Existing target, continuation, indentation, table and line-flow
repairs remain required. Do not erase a normalization module or all section
context merely because some classification rules are too broad.

## Regression obligations

Tests must compare original-event content with annotation enabled/disabled,
including bullet/ordered lists, starts, tightness, punctuation, hard breaks,
nested links and invalid declarations. Check IR serde/index rebuilding and
binding validity after transformations. Semantic Markdown export needs its
own explicitly supported subset and reparse tests; current Markdown output
is not a lossless semantic serialization.

Keep exact expected names independent of the extractor. Cover transparent
links, adjacent parameters, fixed names such as `-Wall` / `-O2`, declared
multiword commands, same-owner distinct subjects, same-name different owners,
unknown roles and bounded content-only evidence. Do not measure success by
entry counts, zero exit status or the absence of diagnostics alone. Preserve
the self-manual's independent clap comparison; distinguish one documented
option owner from additional explanation evidence mentioning that option.

Version decisions follow [the release policy](../releasing.md), not the
presence of a snapshot file alone. Released/frozen contracts must not be
overwritten. An explicitly unreleased family may be revised under the stated
policy with coordinated examples, schemas and changelog. Record the actual
release/freeze status and affected independently versioned crates before
choosing the family; this design does not choose a new version or authorize
publication.
