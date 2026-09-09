# Semantic entries and explanation design

This document explains content ownership, name evidence and bounded explanation
collection. Normative authoring syntax lives in
[mant-markdown(7)](../manuals/mant-markdown.md); current query and wire contracts
live in [mant-protocol(5)](../manuals/mant-protocol.md), and public IR types in
[mant-ir(7)](../manuals/mant-ir.md).

`mant-codec` produces content and semantic annotations; `mant-ir` owns their
source-neutral validation, locations and derived indexes. `mant-query` collects
explanation evidence from existing IR, including caller-authored documents,
without a loader or host callback. `mant-protocol` defines the request and
response contracts, not query execution. `mant-render` presents the returned
DTOs without rerunning collection or consulting producer-private state.
`mant-loader` acquires documents, while `mant-engine` validates and composes
complete loading/query requests. The host alone authorizes external actions;
neither recognizing a name nor returning evidence executes an example, opens a
link or updates a source.

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

Independent physical ownership does not require an empty explanation. Native
producers may retain a bounded consecutive declaration group as a definition-list
annotation: empty heads followed by a described member. Explain can return all
of those original heads and the final member's complete description as recovered
context. This does not prove that every sentence applies to each member and does
not create aliases, children or value domains. Source boundaries and complete
head recognition determine grouping; neither equal indentation nor a search
for the next nonempty paragraph is sufficient.

Context belongs to the selected evidence page, not a later result page. Returned
support references are local to one document's response pool, also in scoped
queries. Original content coordinates and the description provider remain
explicit; omission of a known context is different from absence of context.
Markdown does not infer groups from adjacent items, and full-document rendering
does not change when native grouping metadata is present.

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
whole entry. Safe text projection precedes scalar-range calculation. All page
direct-match facts are reserved first, followed by direct bodies and necessary
declaration-group contexts, then optional metadata/windows and weaker evidence.
These payloads consume one shared serialized-byte budget;
window clipping and budget omission remain independent. Text/Markdown/MCP
render full direct/related content but only preview windows for mentions.
For example, GCC's `-Q` can mention `--help` without becoming its alias or
another direct definition. This does not mutate the full document renderer.

`content.kind: declaration-member` refers to a member of the document-local
`supports` pool. The original group body is copied once, including all heads,
tables, examples and the final member's description. The reference resolves
owner-local positions (outer item zero) into the actual returned member; it does
not change the physical owner, aliases or scope. Decoding rejects invalid pool,
member, identity and position references. An unavailable context sets
`supportOmitted` and overall content truncation, rather than claiming that the
source contains no explanation. Pool and group sizes are bounded by the page
owner limit; repeated references do not retry a group that failed the budget.

Source containment, not equal text or equal IDs, permits reuse across nested
contexts. `contained-declaration-group` records its own members and provider
while pointing directly into an owned source fragment. `owned-entry` can retain
a selected ancestor's body without inventing a reading group; `shared-entry`
uses typed block/item steps to address nested physical owners. Materialization
visits selected ancestors first, independently of evidence presentation order.
An inner-only page does not acquire its unselected ancestor. Metadata and
reference costs remain bounded; an already returned body is not copied again.

A support is usable only by direct evidence with matching content, owner ID,
forms and omission flags. The same validated resolver serves response decoding,
owner-local positions and offline rendering. Invalid in-memory DTOs cannot
print a different group's body merely because a numeric index exists. Each
scoped document retains its own pool; references cannot chain or cross pools.

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

## Explanation match positions

The unreleased v0.11 migration uses the following closed fields. Matching
decisions remain in collection; materialization projects recorded source
bindings, and renderers consume only the response. Ordinary name coloring and
actual query matches are independent dimensions.

| Object | Fields and meaning |
| --- | --- |
| Name basis | `kind: "name"`, `matches: [{name, occurrences}]`: actual authored names, not the query's spelling or all names of the owner |
| Form basis | `kind: "form"`, `matches: [{sourceFormIndex, text, occurrences}]`: actual complete forms, with their original owner ordinal |
| Identity basis | `kind: "identity"`, `fields: ["id", "path"]` (matched subset): values refer to the evidence's outline fields |
| Ordinary binding | `entry.nameBindings: [{nameIndex, occurrences}]`: validated names indexed in this returned entry, independent of the query |
| Occurrence | `{sourceOccurrenceIndex, forms: [FormRange], content: [ContentRange]}`: each domain preserves a complete ordered occurrence or omits it atomically; the source ordinal identifies a binding occurrence (zero for complete-form evidence), not its index in a filtered response array |
| FormRange | `{formIndex, startChar, endChar}` relative to returned `entry.forms[formIndex]`, not the original form ordinal |
| ContentRange | `{kind: "block-text", path, startChar, endChar}` or `{kind: "definition-term", path, itemIndex, termIndex, startChar, endChar}` |
| Content path | Starts at this response's `content.block`; typed `list-item`/`definition-item` (index) or `table-cell` (row, column) steps select a block array, followed by a `block` (index) step |
| Literal preview | Existing window/scalar coordinates plus `contentRanges`, referring to its reported match in the returned body, never the window offset reused as a full-body offset |

All text ranges are half-open Unicode scalar offsets in safe visible text.
Inline wrappers/anchors add no text, source LineBreak adds one scalar; unsafe
controls are replaced one-for-one while legitimate tabs/newlines remain.
Definition terms are independent roots, not synthetic paragraphs. When an
original item N becomes a single-item excerpt, its response index is zero.
Original `blockPath` and source spans remain provenance, not response paths.

Display-cell origins and visual wrap rows are a separate projection. The shared
geometry policy composes parent-relative offsets and resolved gaps without
rewriting these content ranges. CLI text preserves hard lines without width
reflow; the TUI maps the same logical content onto a width-specific cell plan.
Its search/link overlays use that plan, not a second search over decorated text.
Separate source term roots remain separate lines in full-body presentation;
compact outline labels are not substitutes for those roots.

`matchDetailsOmitted` reports omitted Name/Form records or positions applicable
to returned targets. `nameBindingsOmitted` independently reports omitted
ordinary bindings. Both participate in overall content truncation. If an
entire entry/body is absent, its existing details/content omission flag explains
unavailable targets; locations must not reference absent payloads. Actual
matched spellings can still be returned without entry metadata.

Per evidence, Name/Form records jointly cap at 32; ordinary bindings separately
cap at 32 names; each record has at most 32 occurrences; each occurrence has
at most 32 fragments per target domain. All retained match and ordinary
positions, including preview-to-body mappings, jointly cap at 1,024 fragments.
These limits never truncate the original entry's names/forms or affect owner
classification, counting, ordering or pagination.

Copy retention order reserves page direct-match facts, then direct atomic
bodies/contexts, then optional metadata/bindings/windows and weaker evidence.
Position objects, paths and
arrays consume the same serialized-byte budget; duplicate serialized positions
are charged separately. Positions and their target payload are accepted
together, without retries that reorder material. One incomplete multi-slice
occurrence must never be displayed as a complete name match. Invalid external
positions are conservatively left unhighlighted, not re-guessed from text.

Human projection follows the shared [entry presentation](entry-presentation.md)
roles. It distinguishes generated metadata from original body/forms/previews;
report framing is not inserted into document IR. A serialized-and-decoded
response must render offline without loading the original document, consulting
hidden maps or rerunning any business matching.

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

Native definition lowering records an operation-local head witness before
discarding the macro wrapper: `Fl` proves an option role, `Ev` an environment
role, while `Ic`/`Cm` only prove literal naming evidence. The latter remain
context-dependent and may produce a named Term. A proved role is not erased
merely because its template or argument syntax has no supported name binding.

Witness lookup uses the complete source coordinate and exact styled head,
including argument ancestry and link structure; line/column is only a bucket
index. Zero-width navigation anchors are ignored because target allocation
changes them without changing the head. Owner moves and body nesting preserve
this evidence, while changed/split heads and conflicting witnesses invalidate
it. No temporary role, synthetic semantic ID or separate evidence index enters
the public IR. Body macros are not registered as definition witnesses.

Preparation normalizes each container before reading its heads, derives child
context from that same recognition result, and retains one private plan per
final definition. Counting and ID/name-binding allocation consume these plans
in the frozen traversal order instead of re-running the grammar. Allocation
verifies source and styled head equality; only zero-width anchors may change.
There is no intervening topology/context mutation API. Any new normalization
must run before preparation or rebuild the affected plans and counts, not
reuse stale ranges. Temporary validation copies contain heads, never bodies
or a second full document, and are dropped during allocation.

Role selection is a small precedence decision, not a probability score:
proved native role, complete local declaration, then inherited context.
Names are retained independently when the appropriate result is Term. A
parent option does not prove that every nested definition is an accepted
value, nor does assignment syntax alone prove an environment variable.
Native literal `Cm -` is not automatically a standard-input operand.
Topical literal heads with explicit option groups can establish a command;
adjacent literal runs form one name up to the first argument. Complete dotted
keys in variable/configuration sections and mixed-case assignment labels are
local configuration evidence, independent of font choice. These rules do not
promote body references or classify arbitrary filenames as configuration keys.
Uncertain classifications must be reviewed with their query/owner evidence;
they must not be hidden by dropping Terms or manufacturing value domains.

Whole-head acceptance keeps styled arguments opaque without inserting new
whitespace: adjacency and separators in `-L<start>,<end>:<file>` still belong
to the invocation. A bounded template such as `-<number>` can occur beside a
concrete flag, and a terminal `,...` is repetition rather than another name.
Environment groups may contain literal/placeholder templates such as
`GIT_CONFIG_KEY_<n>` alongside concrete names. Templates preserve the declaration
and form but produce no prefix name or invented expansion; rejected prose still
invalidates the group. Path, assignment and colon-delimited metavariables do not
license general prose suffixes as heads.

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
