# Native annotated output: P0 admission contract

Status: R00 frozen on `dev` at `864aa042` (2026-09-23). This is the active
implementation contract for P1. The earlier
[`native-structured-rendering-contract.md`](native-structured-rendering-contract.md)
and [`libmandoc-structured-internals.md`](libmandoc-structured-internals.md)
describe the superseded structured path and its historical implementation;
they are not additional display-admission requirements. The full migration
and removal plan is the 2026-09-23 annotated-output guide. This document
freezes the result shape and stage gates needed to start R01; private field
spelling may change with evidence, but not the ownership or display authority.

The baseline, oracle identity, inputs, outputs and current measurements are
recorded in [`baselines/native-annotated-output-p0/README.md`](baselines/native-annotated-output-p0/README.md).
The disposition of each current module and the now 40 vendor patches is in the
[`native-annotated-removal-ledger.md`](native-annotated-removal-ledger.md).
The current product still uses AST lowering. An `annotated` result is a private
migration path until R08; no P1 test is a claim that production has switched.

## Authority and one-copy rule

One native parse and UTF-8 terminal render performs roff execution. The
post-device safe `DisplaySurface` is the sole owned visible roff body. Native
node, buffer and source hooks attach marks to that surface; they do not supply
another visible body. The adapter may interpret the device's emitted glyphs,
spaces, backspaces and newlines to produce safe cells and style. It may not
decide line breaking, table widths, tag fit, indentation or macro behavior,
and may not force `term_flushln()` to close a semantic object. No final raw
stream, logical full-text copy or permanent placement/event history is kept
beside the surface. Explicit, bounded query artifacts are temporary.

The root result owns metadata, `SourceTable`, `DisplaySurface`, diagnostics,
`AnnotationCoverage`, and compact section/owner/link/anchor/region marks.
The surface owns visible text once, preferably in a compact arena. A mark
owns keys, native facts and bounded ranges into the surface, not copied body
strings. Authored target phrases and finite declaration hints may be retained
as metadata where the native tree will be unavailable after return.

| Object | Unit and identity | Required invariant |
| --- | --- | --- |
| `SourceKey` | one-based result-local source table key | source spans resolve against the same table, including repeated includes |
| `RowKey`, `RunKey` | result-local final display keys | minted after font folding, overprint, run coalescing and decoration filtering; empty surface still has a document-end point |
| `DisplayRow.breakAfter` | whether native emitted a newline after this row | preserves EOF without newline and trailing blank rows; only the last row may be `false` |
| `OutputSlice` | `(RunKey, [start_byte,end_byte))` in final UTF-8 run | both ends are UTF-8 boundaries; zero width uses an explicit boundary point |
| `TextSelection` | ordered output slices plus per-adjacency `TextJoin` | joins state exact consumed separator bytes, direct contact or hard/unknown boundary; not layout |
| `HeadingMark` | native section key, parent, level hint, title selection | parent is root or earlier section; no derived depth stored as a competing source |
| `OwnerMark` | native candidate key, parent, head/body and role | not itself a classified `EntryKind`; direct body and subtree reading differ |
| `LinkMark` | occurrence key, optional destination, label selection, source | one macro instance can have multiple surviving slices; equal destinations do not merge occurrences |
| `AnchorMark` | native declaration, source and final zero-width point | authored `.Tg` source differs from migrated display location |
| `RegionMark` | table/cell/other native identity, enclosing section and final selection/point | an ownerless table still belongs to its section; a cell need not own a physical row or another text copy |

All offsets name their units: source bytes, output UTF-8 bytes, Unicode
scalars, graphemes and terminal columns are distinct. `SourceTable` records
authorized logical paths only; generated/unknown provenance is explicit. A
link occurrence remains one native macro instance, but its clickable label
ends when pinned HTML execution closes the phrase (paragraph/list/table
boundary or fill-mode change), even if terminal traversal continues inside
the same macro body. A later nested link may open independently; returning
to the outer AST frame never reopens its closed label. The mark and its
decoded destination, if any, remain even when the final label is empty. A
native `.MR` with no operands still renders an `Xr` link instance with `()`
but no `href` (`man_html.c::man_MR_pre`); its Fixed `LinkMark.target` is
`null`, not a forged URI or an omitted occurrence.

A `direct-contact` join inserts no byte, including a proven native soft wrap
between physical rows; the row keys carry that layout fact. An
`authored-separator` join carries exact native-consumed source-authored bytes;
`generated-separator` carries exact formatter-generated `AUTO_SPACE` bytes
only when a surviving buffer write is consumed at WRAP. Both use a bounded
`text` payload of one or more ASCII spaces; generated bytes have no authored
source position and count toward join-byte and query work budgets. A separator
already visible inside a selected slice is not repeated in the join. Thus
one and three spaces swallowed by `term_flushln()` at WRAP remain distinct
even when the final surface rows are identical. Hard/unknown never licenses
a cross-slice content match. A generated `AUTO_SPACE` cannot masquerade as
authored, and a reused blank without a surviving write proves neither join.
Native indentation may produce blank runs between connected slices; only
runs carrying the explicit native `layout` role may be skipped by Fixed
selection validation, not merely unowned or source-unknown spaces. The
layout-only gap check uses one per-surface prefix index, not a rescan for
each section, owner or link.
Physical adjacency alone proves none of these states. A wrong display byte,
row, UTF-8 or source range is a hard invalid result. A wrong optional mark
key or selection relation is rejected independently after the complete native
surface is checked: the affected fact is removed, or, if its scope cannot be
isolated, all annotations are cleared while the checked display remains.
The native handle owns borrowed views; checked transfer produces one owned
result, then the handle is released. No slice or C pointer survives the handle.
R01 must test size/align/offset, checked `ptr/count`, integer overflow and
failure cleanup before exposing this model.

## Display, section and owner examples

These are coordinate examples, not fabricated fixed-CVS output assertions.
Exact visible bytes for behavioral tests must first be run through the
registered reference. In the examples `rN[a,b)` means byte range in final
run `rN`, and `@rN:b` is a zero-width boundary.

| Situation | Required shape |
| --- | --- |
| A title link crosses two style runs and a soft wrap | one `HeadingMark` and one link occurrence select `r3[0,4), r4[0,5), r5[0,2)`; known joins preserve the full title, not a second title string |
| Two `.TP` owners print on one device line | `OwnerMark(o1)` and `OwnerMark(o2)` select disjoint subranges of that row; row identity is not owner identity |
| Empty list/definition owner | distinct `OwnerMark` and point `@r8:0`; no text from the following item is borrowed |
| Link spans a table cell boundary | one occurrence can retain separate slices in cells `c1` and `c2`; the cell boundary is a hard logical join unless native facts prove otherwise |
| Root-level preface, section and subsection | preface belongs to document root; section `s1` has direct body excluding child `s2`; subtree reading `s1` traverses title, direct body, enclosed owners/regions and `s2` once, in display order |
| No parent `.SH/.Sh` for `.SS/.Ss` | subsection is attached to root, retaining its native level evidence; no synthetic visible parent title |
| Root and include use the same line/byte offset | their spans have distinct `SourceKey`s; repeated include can reuse a source key but never merges output occurrences |

Sections are structural navigation marks, not boxes with their own copied
body. Title and direct-body selections may contain fragments from multiple
rows, and one row may contain multiple sections or owners. Parent/child
structure never adds display indentation. Root preface, empty and repeated
sections keep identities. Source order, output order and selection reading
order are specified independently; no section close or owner close flushes
the native device. A direct-body read excludes descendants, while the normal
section read includes the title, owner heads/bodies and transparent list,
literal, table and equation regions with descendants, each final byte once.
An owner explanation's `readingBody` includes enclosed owners and regions;
`OwnerMark.direct_body` remains the narrower semantic selection. Its bounded
DTO fragments retain native physical row, checked terminal column/width and
final style. Text/ANSI/Markdown presentation uses those physical coordinates,
including blank rows, never `TextJoin` as a layout instruction.

For man(7) hanging paragraphs, the native first-paragraph boundary and the
executed positive `.RS` offset establish a presentation candidate and an
ownerless continuation region. The candidate and continuation keep a checked
bidirectional relation; neither reparents the displayed RS subtree. A complete
head declaration and a non-table description are required before the candidate
becomes a query entry. Owner reading follows the continuation's native region
descendants (including literal content and nested owners) once, while section
reading still sees those same final-display slices once.

## Safe output and overwritten marks

The exact pinned paths for R01 are `term.c::term_field/encode1/term_flushln`,
`term_ascii.c::ascii_letter/utf8_letter/ascii_advance/ascii_endline`,
`man_term.c` and `mdoc_term.c` head/foot, `tbl_term.c` direct draw and
`eqn_term.c::eqn_box`. Device writes, not FIELD width requests, determine
actual bytes and rows. `ascii_advance()` can truncate or emit no space.
Buffer labels must be bound when content enters the native buffer, survive
partial consumption and retirement, and remain identifiable when the footer
callback flushes earlier body content. Header/footer glyphs and their own
spacing are filtered by output role; body flushes, `.sp`, `.mc`, table borders
and equations survive.

Font overstrike encoding folds to style while retaining the underlying
content identity. Real `\z`/`\o` are ordered display writes. A later write
may replace, split or combine visible cells; only surviving glyph portions
retain their original owner/link/source. A replaced portion does not pass
its identity to a later equal glyph and does not remain searchable, clickable
or copyable as hidden text. A separately declared anchor may keep its own
point, but a deleted link or name is not fabricated as a point. Partial name
or form loss invalidates that visible binding rather than borrowing later
owner text. If survival cannot be established, emit semantic coverage loss;
invalid references are hard errors. Temporary buffer/row IDs are not public
keys. Final keys are stable only after all display filtering/coalescing and
full result validation. Ambiguous overlapping link identities must not be
silently unioned or assigned by screen position alone.

R01 reference matrix: same and different overprinted glyphs across owners
and links, partial labels, font+real overstrike, wide-to-narrow then another
write, combining marks, direct table drawing, independent anchors, and
footer-triggered body flush. It also checks `FIELD_SKIP` whitespace held for
later `advance`, indentation versus authored separation, `ascii_advance()`
truncation or zero-byte output, and direct table spaces: actual byte count
and label must agree. The upstream multi-file-only `terminal_sepline()` is
outside the one-page annotated session; a future multi-file API must
classify it explicitly. Check raw stream against the independent CVS
execution and final cells/style against a separately specified safe-terminal
normalizer. Hook on/off must preserve native raw bytes. The adapter has one
writer for active buffer labels, one for current display line and one for
marks; session alone owns render/TLS/cleanup. No module creates a parallel
renderer, TLS state or global parse lock.

## Coverage, errors and budgets

Native `AnnotationCoverage` records a finite reason, affected dimension and
best-known source/owner/region range; unknown required dimensions are not
complete. The frozen dimensions are `section`, `owner-boundary`,
`declaration`, `link`, `anchor`, `relation`, `source` and `join`. Frozen issue
reasons are `not-observed`, `unverified`, `rejected` and
`ambiguous-survival`. A checked dimension with no issue is `checked`; a
capability not promised by the producer is explicitly `not-applicable`,
never an implicit missing check. `native`, `codec` and `validator` are
distinct producers. Native checks its section/owner/link/anchor/source/join
facts; codec checks declaration/classification and source-neutral relations;
shared validators check final references. R01 records its own check states
and pending downstream producers, not a default whole-page `complete`.

At final IR construction, each promised producer/dimension without a
`checked` or justified `not-applicable` state becomes `unverified`. Each
issue maps to `DiagnosticImpact::SemanticCoverage` with stable code
`annotated.coverage.<dimension>.<reason>`, for example
`annotated.coverage.link.unverified`. Its `scope` is tagged `document`,
`section`, `owner`, `region` or `source`. The source-neutral IR `Diagnostic`
gains an optional strictly validated `coverageScope` for all five; a
source-scoped gap may name only its known `SourceKey`, without inventing a
line/column. When the exact authored location is known, the existing
`source` also retains its qualified `SourceSpan`.
Parser-generated diagnostics and marks can retain a source identity without
an authored position. Their optional `sourceKey` carries only that
`SourceTable` member; it is mutually exclusive with an authored `source`
span and is validated against the same table. Neither a macro-expansion
offset nor a generated inline equation's reparse offset is converted into a
fabricated authored line or byte range. Display runs already carry their
source key separately from authored mark locations.
Absence of local scope means document-wide. Invalid display or source
keys/ranges remain hard errors; an invalid optional entry, link or navigation
relation instead loses the unsafe semantic fact, records
`annotated.internal-annotation-rejected` with `semantic-coverage` impact, and
never leaves a stale clickable link or misattributed owner. When a relation
cannot be isolated locally, only the strictly checked native display survives.
An annotation-only failure in native validation or FFI transfer follows the
same body-only rule; transfer budgets, allocation failure, unsafe UTF-8 and
incomplete display output never do. Legitimate zero-glyph macro instances
produce no name and no internal-error diagnostic. Codec merges these
diagnostics with common name/form/link validation; `semanticsComplete` is
computed *only* from the merged set. Frontends do not set it independently.
Conservative `Term` or a non-entry explanation does not alone mean a known
extraction failure.
Correctly retired invisible glyphs are not missing extraction. A failed
capture, source/display safety check or budget is not recoverable coverage.

A response with truncated diagnostic detail retains one document-scoped
diagnostic `annotated.coverage.summary` with impact `semantic-coverage` and
an exact `u32 coverageDetailsOmitted` count. Counting work is charged to the
response budget; overflow or inability to count is an error. A positive
omission count requires the
summary; the summary is not emitted on a complete page. If even this pair
cannot be returned safely, fail.
No-hit, excerpt, pagination and cross-document projections must retain the
document-level completeness state. Example: complete surface plus a link
whose required target/label association is unknown yields a coverage
diagnostic and `semanticsComplete=false` in both outline and explain even if
their response budgets omit that link's detailed diagnostic. A normal page
with only conservative Terms may still report `true`.

Schematic v0.12 fragments frozen for R02b; messages are not authoritative,
but codes, impacts, scopes and omission state are:

```json
{"coverage":{"checks":[{"producer":"native","dimension":"link","state":"unverified"}],"issues":[{"dimension":"link","reason":"unverified","scope":{"kind":"owner","key":3}}]},"diagnostics":[{"level":"unsupported","impact":"semantic-coverage","code":"annotated.coverage.link.unverified","message":"link association unavailable","coverageScope":{"kind":"owner","key":3}}]}
{"semanticsComplete":false,"coverageDetailsOmitted":4,"diagnostics":[{"level":"unsupported","impact":"semantic-coverage","code":"annotated.coverage.summary","message":"coverage detail omitted","coverageScope":{"kind":"document"}}]}
```

Budget categories are input, native execution work, native allocation,
source records, output bytes/cells/runs, active slots, marks and joins, FFI
transfer, IR indexing and response projection. Cumulative work is never
refunded by a failed attempt; active and peak storage are separate metrics.
Overflow, malformed UTF-8, invalid pointer/count, range or association are
explicit errors. Output-budget failure returns no partial owned document;
cleanup restores TLS, callbacks and the next call. No promise is made about
upstream fatal OOM or arbitrary cancellation.

## Exclusive IR body and wire

`Document` keeps common metadata, source table, root source and diagnostics;
its sole *primary document* body is `Flow` or `Fixed`. `Flow` owns the
existing Markdown content store and tree. `Fixed` owns the safe surface and
source-neutral marks. `ResolvedContent.tldr` remains an optional, separate
Flow quick reference, including when `document=None`; it is not another
primary body or a fabricated Fixed section. Fixed+TLDR and TLDR-only are
valid query combinations. TLDR provenance stays in its own origin/source
path, not the primary document's `SourceTable`; TLDR-only results omit the
primary `SourceContext`.
It is not `Code` and does not contain roff tokens. No `blocks=[]` plus hidden
text, no second semantic index, no full AST retained for the normal roff
load. `Visit`, source validation, content resolver, index and true consumers
must branch explicitly. A Fixed branch may not silently return an empty walk.

Frozen v0.12 address shapes (all byte ranges half-open UTF-8). Under `flow`,
`location` retains the *complete* existing `ContentLocation` fields, not an
outline-path abbreviation. `tldr` has its own path and never forges a manual
source key; its byte range is within that path's validated TLDR logical text
unit, not within the path identifier. The zero-row point is the document end,
not a fictitious run:

```json
{"kind":"flow","location":{"kind":"content","sections":[0],"blocks":[{"kind":"block","index":1}],"root":{"kind":"inlines"},"path":[0]},"startByte":0,"endByte":4}
{"kind":"tldr","path":"0","startByte":0,"endByte":4}
{"kind":"fixed","parts":[{"run":9,"startByte":0,"endByte":4},{"run":10,"startByte":0,"endByte":2}]}
{"kind":"fixed-point","at":{"kind":"run-boundary","run":9,"byte":4}}
{"kind":"fixed-point","at":{"kind":"row-column","row":8,"column":0}}
{"kind":"fixed-point","at":{"kind":"document-end","rowCount":0}}
```

These addresses are snapshot-relative and explicitly discriminated. A Fixed
`row-column` point names an actual final body row and terminal-column boundary
(including row end); it does not borrow a neighboring run's UTF-8 bytes. The
existing `run-boundary` cannot represent a blank row, a visible gap, or an
anchor whose original glyph was later covered. A trailing point with no next
body row uses `document-end`, never an earlier owner's last run. Empty table
cell offsets remain native layout hints until independently bound to a final
row in the subsequent cell-position unit.

A Fixed term does not masquerade as `EntryInlineRoot::Term`; arbitrary runs
or links are not `--node` structural nodes. Form/name bindings require exact
surviving visible spelling. Deserialize validates body exclusivity, UTF-8
boundaries, key closure and density, zero-width points, source-qualified
references and all cross-mark relations. Old top-level
`contentStore`/`blocks`/`sections`,
mixed bodies, unknown discriminants and dangling slices are rejected on the
wire, not accepted as an empty document.
For the primary `Document`, `body.kind` is required and exactly `flow` or
`fixed`; fields of the other arm are forbidden. TLDR remains outside this
primary body. An old top-level content tree or old three-optional-coordinate
search occurrence is rejected by tagged deserialization even if unknown
fields would otherwise be ignored.

Declaration recognition operates on a borrowed complete visible HEAD. The
source-neutral option scanner receives checked UTF-8 byte intervals for final
style and independent native operands; it returns bounded name intervals, not
another body or a durable cache. Flow maps those intervals back to original
inline content; Fixed maps them to surviving display slices. Invalid evidence,
no accepted name, and a name-budget overflow are distinct outcomes. A failed
complete scan may not publish its first apparent name through another prefix
fallback. Unknown joins never license a reconstructed complete HEAD.

Native Flow macro-role hints are conversion-local and are not serialized in
`DefinitionItem`. After a Flow JSON round trip or in-memory edit, read-time
checks validate retained kind/field constraints and exact name/form/content
bindings; they cannot certify which original Ev, Va or other macro produced a
structurally valid category. Fixed retains owner/component evidence and
rechecks optional entry facts against the current surface and that evidence.
Neither path treats a previous immutable-operation validation result as a
cross-edit `validated` flag. Markdown explicit entries retain their author's
category authority, subject to the same binding and relation checks. Existing
coverage diagnostics survive a round trip; the absence of Flow's ephemeral
macro hint alone is not a semantic-coverage failure. This paragraph freezes
the validation boundary.

EN02 keeps `Ev`, `Va` and `Dv` as distinct, source-qualified Fixed HEAD
component roles (`environment`, `variable`, `defined-variable`); `Dv` still
maps to the existing `Term` entry kind. An executed macro with no surviving
glyphs contributes no name. When such a first authored instance precedes a
different visible component, projection records the first *visible* role in
the typed owner while the checked native mark retains the raw execution
instance. A missing raw owner role is never promoted by a later component.
Read-time proof requires the persisted owner role and each visible component
role to agree, so editing JSON cannot reclassify an existing binding.

In an ENVIRONMENT section, a complete `.IP`/`.TP` HEAD can yield several
independent environment names (for example `TMPDIR, TEMP, TMP`) with one full
form and one exact surviving selection per name; this does not create aliases.
The nearest recognized section heading controls lexical context, including a
nested OPTIONS subsection overriding an outer ENVIRONMENT section. Repeated
native `Ev` occurrences remain repeated bindings of one name; binding-array
order is immaterial, but each occurrence must still match the checked native
HEAD. Fixed currently proves these non-option groups against a single complete
form, while existing multi-form option rules remain strict. The shared
environment grammar stops before a 65th member is allocated. Flow applies
that occurrence cap across all `.TP`/`.TQ` terms merged into one owner, not
once per term. Both Flow and Fixed retain the native text and report
semantic-coverage loss instead of silently publishing a partial group.

## Search and response coordinates

For R02a, `scope=visible` retains Flow/TLDR's current canonical Markdown
visible-text extractor and matcher, including matches across Flow roots when
both ends belong to the same renderer owner. A whole match need not fit one
logical root, and synthetic visible separators must not be assigned a false
authored address. Matches spanning different owners, or lying only in an
unpresentable synthetic separator, remain filtered as today. Fixed searches
surviving selections with known `TextJoin`s instead. One match is one
occurrence even if it projects to several runs or rows; independent cells and
unknown joins block a fabricated cross-boundary match.
`scope=markdown` searches the canonical addressable user-exportable
artifact. For Flow/TLDR this is the existing exporter selected by
`--format markdown --preserve-anchors`, not the default clean Markdown export;
the request has no presentation-option switch. Fixed uses the same safe-length literal fence
exporter as R03. Fence-only hits are valid Markdown coordinates without a
native display projection. Fixed's soft-wrap visible match and literal
Markdown-artifact match intentionally can differ.

The response carries a tagged authoritative coordinate: `visible-flow`,
`visible-fixed` or `markdown-artifact`, plus subordinate display fragments.
No Fixed row/run is written into an old Markdown-range field. A bounded
response-local projection closes any referenced content. `offset`, `limit`,
`total`, `returned`, `ordinal` and `nextOffset` all count *occurrences* in
Flow, Fixed, TLDR and cross-document scope. Search units have deterministic
document/selection order; resize, run styling and viewport offset do not
change pagination. `total` is exact only after a complete scan; scanning,
counter overflow or required-fragment budget failure is an error, not a
saturated count or silently truncated match. Fixed builds the Markdown
artifact only for a requested export/Markdown search and then releases it;
R02a's Flow/TLDR visible matcher still uses its canonical Markdown render.

Example: a Fixed word split across `r7[5,8)` and `r8[0,3)` with a proven soft
join returns one visible occurrence and two display slices. A search for the
literal fence delimiter returns one `markdown-artifact` occurrence and no
Fixed slice. A mixed Fixed manual and Flow TLDR has one ordered query snapshot:
the Markdown export is `# <escaped label>`, then TLDR blocks, then `---`,
then the Fixed fence, joined between blocks with exactly `\n\n`, and no
final newline. TLDR-only omits the separator and Fixed fence. This preserves
the existing application-heading/TLDR-first order. TLDR is selected at path
`0` and its own source; the manual's root/sections follow in snapshot order.

The Fixed fence uses bare backticks of length
`max(3, longest consecutive backtick run in surface + 1)`. Its bytes are
`fence + "\n" + surface bytes + ("" if already newline-terminated else
"\n") + fence`. `DisplayRow.breakAfter` distinguishes EOF with and without
a native final newline. The artifact has exact UTF-8 bytes, while public
match ranges count Unicode scalars from zero; its required delimiter newline
is presentation only, not a native
glyph. A Fixed surface of `body` with no final native newline, label `Demo`
and one TLDR description `Quick` exports these exact bytes, with no newline
after the closing fence:

~~~~text
# Demo

## TLDR

Quick

---

```
body
```
~~~~

With `document=None`, the exact artifact is
`# Demo\n\n## TLDR\n\nQuick`, with no primary source context. For a Fixed
manual and TLDR together, a Markdown-only fence match can have no native
location; visible matches retain typed manual or TLDR locations. Export
search never joins two table cells just because literal bytes are adjacent.

The v0.12 search request keeps existing fields but replaces `SearchHit`
line groups with occurrence-shaped `matches`. The mandatory coordinate is
one tagged `location` object. Both `visible-flow` and `visible-fixed` contain
a response-local `unit` and a unit-relative half-open Unicode scalar range;
`markdown-artifact` contains an export scalar range and one-based scalar
line/column. `displaySlices` are subordinate fragment-relative scalar
locations, not a substitute for the authoritative range. Each visible unit
is closed by a bounded, ordered
projection of exact UTF-8 fragments and explicit join facts. The unit text
is the concatenation of fragment text and exact join text, with no inferred
separator. The occurrence range must be nonempty and within that
unit, and equal to `matchedText`; a reader can check this without fetching
the original snapshot. Only retained occurrences require projected units;
count-only scans and zero-hit pages do not materialize a fake unit or copy
the whole visible document into the response.

For `visible-flow`, each fragment's source is tagged `flow` with a complete
`ContentLocation` and root-relative byte range, `tldr` with its own path and
range, or `render-derived` for visible bytes whose exact Flow/TLDR origin
cannot be proved. Flow and TLDR source ranges must resolve within the
response-local projection; a Markdown parse transformation or synthetic
block separator must never masquerade as authored root bytes. Exact
separator text between fragments is represented once by a
`render-separator` join with its exact UTF-8 `text`, not by a duplicate
fragment and not by Fixed's ASCII-space-only `AuthoredSeparator`.
`render-derived` fragments cover transformations within a rendered root.
A same-owner match across Flow roots may therefore occupy one unit with
several typed fragments and an exact render-separator join. For the Flow
source `# Demo\n\nalpha\n\nbeta\n`, the unit below validates the single
`alpha\nbeta` occurrence at `[0,10)`; the canonical visible extractor has
already collapsed the paragraph boundary to one newline:

```json
{"contentProjection":{"fragments":[{"key":1,"text":"alpha","source":{"kind":"flow","location":{"kind":"content","sections":[],"blocks":[{"kind":"block","index":0}],"root":{"kind":"inlines"},"path":[0]},"startByte":0,"endByte":5}},{"key":2,"text":"beta","source":{"kind":"flow","location":{"kind":"content","sections":[],"blocks":[{"kind":"block","index":1}],"root":{"kind":"inlines"},"path":[0]},"startByte":0,"endByte":4}}],"units":[{"key":1,"fragments":[1,2],"joins":[{"kind":"render-separator","text":"\n"}]}]},"location":{"kind":"visible-flow","unit":1,"startScalar":0,"endScalar":10},"matchedText":"alpha\nbeta"}
```

The current matcher still rejects a match that
crosses the TLDR-to-manual owner boundary; the wire's capacity to describe
both source kinds does not authorize a new match. For `visible-fixed`, each
fragment's original row/run range is navigation metadata checked against
the snapshot by the producer, and joins retain the native `TextJoin` rules.
A complete response also carries `label`, optional source/meta context,
`semanticsComplete`, bounded coverage diagnostics and an exact
`coverageDetailsOmitted` count. The following
JSON objects are frozen *field-shape* examples; their run/unit and line
values refer to separate miniature snapshots, not to the `Demo` artifact:

```json
{"pattern":"foobar","scope":"visible","syntax":"literal","case":"sensitive","word":false,"contextLines":0,"limit":10,"offset":0}
{"schema":"mant.search/v0.12","label":"Demo","query":{"pattern":"foobar","scope":"visible","syntax":"literal","case":"sensitive","word":false,"contextLines":0,"limit":10,"offset":0},"render":{"schema":"mant.fixed/v1","format":"fixed-visible","scope":"full","lineBase":1,"columnBase":1,"lineCount":2},"contentProjection":{"fragments":[{"key":1,"text":"foo","source":{"row":1,"run":7,"startByte":5,"endByte":8}},{"key":2,"text":"bar","source":{"row":2,"run":8,"startByte":0,"endByte":3}}],"units":[{"key":1,"fragments":[1,2],"joins":[{"kind":"direct-contact"}]}]},"total":1,"returned":1,"offset":0,"truncated":false,"nextOffset":null,"semanticsComplete":true,"coverageDetailsOmitted":0,"diagnostics":[],"matches":[{"ordinal":1,"matchedText":"foobar","location":{"kind":"visible-fixed","unit":1,"startScalar":0,"endScalar":6},"displaySlices":[{"fragment":1,"startScalar":0,"endScalar":3},{"fragment":2,"startScalar":0,"endScalar":3}],"preview":"foobar","context":[]}]}
{"schema":"mant.search/v0.12","label":"Demo","query":{"pattern":"^```","scope":"markdown","syntax":"regex","case":"sensitive","word":false,"contextLines":0,"limit":10,"offset":0},"render":{"schema":"mant.markdown/v1","format":"markdown","scope":"full","lineBase":1,"columnBase":1,"lineCount":3},"total":1,"returned":1,"offset":0,"truncated":false,"nextOffset":null,"semanticsComplete":true,"coverageDetailsOmitted":0,"diagnostics":[],"matches":[{"ordinal":1,"matchedText":"```","location":{"kind":"markdown-artifact","startScalar":0,"endScalar":3,"startLine":1,"startColumn":1,"endLine":1,"endColumn":4},"displaySlices":[],"preview":"```","context":[]}]}
{"schema":"mant.search/v0.12","label":"Demo","query":{"pattern":"absent","scope":"visible","syntax":"literal","case":"sensitive","word":false,"contextLines":0,"limit":10,"offset":0},"render":{"schema":"mant.fixed/v1","format":"fixed-visible","scope":"full","lineBase":1,"columnBase":1,"lineCount":2},"total":0,"returned":0,"offset":0,"truncated":false,"nextOffset":null,"semanticsComplete":false,"coverageDetailsOmitted":1,"diagnostics":[{"level":"unsupported","impact":"semantic-coverage","code":"annotated.coverage.summary","message":"coverage detail omitted","coverageScope":{"kind":"document"}}],"matches":[]}
```

The old `SearchHit.occurrences` grouping, three nullable occurrence
coordinates (`root`/`logical`/`markdown`) and line-group ordinal are rejected,
not reinterpreted. A TLDR-only visible hit has a `visible-flow` unit with
`tldr` fragment sources and no primary `SourceContext`. A Markdown-only
hit on the `Demo` artifact's opening fence would have an export scalar range
and zero `displaySlices`; the example above illustrates its shape, not the
literal offset in `Demo`. A partially overprinted `--help` head whose final
visible text is `--he` yields no `--help` name binding or direct match;
the owner and its source remain, and a correctly accounted native overwrite
does not by itself create `SemanticCoverage`.
For Fixed+TLDR visible search, the independent TLDR units precede native
Fixed units in one occurrence cursor and never join across that boundary.
The result's `fixed-visible` render and `lineCount` describe the primary
native surface only; a `visible-flow` TLDR hit retains its own unit and
TLDR-local Markdown context lines, not a fabricated native row. The result
`scope=full` means neither arm was viewport-clipped. In the single Markdown
artifact, a hit wholly inside the TLDR range has the TLDR outline path `0`;
the export scalar coordinates remain artifact-global.
For scope search, the `documents` array still contains only retained-hit
groups, but a separate `coverageByDocument` array contains **every scanned
document in scope order**, including zero-hit and globally paginated-away
documents. Each entry has its `address`, `depth`, `semanticsComplete`, exact
`coverageDetailsOmitted` and bounded `diagnostics`; the top-level
`semanticsComplete` is the conjunction of these entries. It cannot be inferred
from the returned hit groups. A scope scan unable to account for every
document or its completeness fails instead of returning an apparently
complete subset. A two-document scope whose first document has no hit and an
unverified link association, while the second has one retained hit, has this
required response shape (the second document's hit details are abbreviated):

Coverage diagnostics and hit-group source tables also share a 32 MiB
aggregate serialized-metadata budget across the whole scope response. The
per-document diagnostic bound is not an independently reusable allocation.

```json
{"schema":"mant.scope-search/v0.12","total":1,"returned":1,"offset":0,"truncated":false,"nextOffset":null,"semanticsComplete":false,"coverageByDocument":[{"address":{"kind":"manual","name":"first","manualSection":"1"},"depth":0,"semanticsComplete":false,"coverageDetailsOmitted":1,"diagnostics":[{"level":"unsupported","impact":"semantic-coverage","code":"annotated.coverage.summary","message":"coverage detail omitted","coverageScope":{"kind":"document"}}]},{"address":{"kind":"manual","name":"second","manualSection":"1"},"depth":1,"semanticsComplete":true,"coverageDetailsOmitted":0,"diagnostics":[]}],"documents":[{"address":{"kind":"manual","name":"second","manualSection":"1"},"depth":1,"matches":[{"ordinal":1,"location":{"kind":"visible-fixed","unit":1,"startScalar":0,"endScalar":3}}]}]}
```

This is a field-shape example, not a complete deserializable scope response:
the query, render and bounded content projection for the hit group are
omitted here only for readability. R02a must implement them and validate the
`coverageByDocument`/`documents` address relation together.
R02a must not expose a new v0.12 DTO while Flow/TLDR or cross-document search
still paginates by the old line-group unit. Either migrate those entry points
in the same runnable unit or keep the new wire private until their atomic
switch; R02b's Fixed sample then uses the same count core. R00 approves an
unpublished wire rewrite, not two concurrent v0.12 contracts.
The Flow/TLDR visible-unit closure above is a P0 contract-evidence correction
for the existing same-owner matcher, not a claim that the new search DTO or
projection has been connected in product code.

## Stage gates, module and patch ownership

| Gate | Required true consumer and evidence |
| --- | --- |
| R01 | `annotated` native handle → checked owned rows/runs + basic marks; raw renderer unchanged; man/mdoc/table/eqn/footer, failures, ABI and four representative native captures |
| R02a | exclusive IR Flow/Fixed body → Visit/validation/content view and real render/query reader; Markdown remains unchanged; typed wire and occurrence pagination are coherent |
| R02b | real annotated → Fixed IR → section read/outline/breadcrumb and one owner/form → shared index/explain; Fixed visible cross-line search, coverage flag and source tests |
| R03 | same Fixed result through CLI direct/ANSI, pager, real TUI Buffer and literal Markdown exporter; 20/40/78/120 are viewports of one 78-column native result |
| G1 | GCC/Git/Clang/rclone complete display from true native and IR paths, no duplication or phantom whitespace in CLI/TUI; key marks valid; load/RSS/phase costs recorded and substantial regressions explained |
| R04 | Flow/Fixed share checked declaration grammar; Fixed names, forms, reading groups and mentions use native owner and text-selection evidence; source-bound real-page query gold and positive/negative declaration cases pass without switching the default loader |

R04's reviewed query scope passed 51/51 source-bound Fixed gold cases at
`390481d6` using `cargo build --locked -p mant --features annotated-preview`
and `python3 scripts/annotated_fixed_query_gold.py --cli target/debug/mant`.
`scripts/check.sh` now runs that same preview-path gate. This proves the
reviewed query results, **not** that every page has complete semantic
annotation: document-level `unverified` coverage may still make
`semanticsComplete=false`. Coverage must remain visible to consumers; native
display remains authoritative and the default loader is unchanged. The
collector state-owner split is a separate pre-R05 unit; link and selection
migration belong to R05/R06, not this R04 query gate.

`libmandoc-rs` session alone owns parse/render, TLS, callback restoration and
cleanup; source owns authorized bundle/include and diagnostics; buffer owns
active token/slot/pending/free-list and partial consumption; display owns
safe current/final rows; marks own result-local identities; result owns
validation/view/free; budget owns controlled allocation and work accounting.
Rust FFI owns ABI, borrowed view lifetime and checked owned transfer. Codec
owns source-neutral identification; IR owns body/addresses; query/protocol
own one semantic query pipeline; UI only displays an already laid-out Fixed
surface. Public parser/AST and raw Renderer remain independent capabilities.

The current Rust module seams mirror those owners: `ffi/structured/annotated`
keeps raw ABI declarations, a handle-bound checked view, coverage transfer,
and owned transfer separate; `mant-codec/annotated_fixed` separates identity
allocation, typed mark projection, and diagnostics under one assembly entry;
`mant-ir/fixed_body` separates the public model, read-time owner/head evidence
closure, and surface/mark validation.
These are internal moves, not new response models or another render path.
The native collector remained one state writer through R04. Its pre-R05
internal split keeps one per-render session and one event order:
`mant_mandoc_annotated_collector.c` coordinates accounting and final cleanup;
`_buffer.c` alone writes active columns/slots, pending glyphs, point chains and
the terminal sink; `_marks.c` writes AST frames, owner/table/link marks and
structural event state; `_declarations.c` supplies parser-alive declaration
and reading-neighbor evidence without classifying final names. Their private
header carries per-render state and narrow cross-module operations; no module
installs its own TLS or causes a semantic node close to flush the formatter.
Mark and point arrays reserve the same one-based key before mark insertion
commits it. Partial consume, truncate-point transfer, reset, and column free
remain one buffer lifecycle; cumulative work and mutation budgets do not
refund retired slots. `FIELD_PLACE` holds its label until pending device
spaces are emitted, then `LETTER` computes the final predecessor edge.
Selection, final display, result validation and transfer remain separate
existing owners. The UI's Fixed/Flow lowering split belongs with R06's
selection consumers, not with this native refactor.

Patch ownership at P1: 0020 supplies raw output capture; 0029 and 0032–0036
supply current observation/region boundaries; 0030–0031 supply target
origin/source. R01's 0037 labels actual device writes; 0038 marks the
body drain within the footer callback for the annotated sink. R01 does not
prune the 38-patch series. Potential 0017/0024–0026/0033–0036
removals belong to the later R09–R10 consumer-led removal ledger, with
parser/render/security capabilities checked before any deletion.

## Frozen P1 acceptance IDs

These IDs are separate checks, not permission to call a passed old path a
passed new path. R01 covers the native/FFI subset, R02a/b cover the IR/query
subset, R03 covers true display, and G1 combines them on representative
pages. All behavior expectations require the exact minimal fixed-CVS run
before the assertion is written.

| ID | Acceptance boundary |
| --- | --- |
| A01 | one main parse/render, no full owned AST or fallback on annotated loading |
| A02 | man/mdoc prose, headings, literal, tbl and eqn all retain native body |
| A03 | role-filtered head/foot without losing footer-flushed body, `.sp` or `.mc` |
| A04 | soft wrap, real hyphen, zero-width and hard joins do not fabricate words |
| A05 | font, `\z`/`\o`, wide/narrow and combining safe cells, no executable control output |
| A06 | nested `.Bd -literal`/`.Bl`, same-line multiple owners and table handoffs |
| A07 | native width/spacing/span/empty/rule/T{T}/border/link table display and points |
| A08 | forms, empty/nested owners, alias and reading-context query evidence |
| A09 | link occurrence/target, `.Sx`/`.Tg`, source and round-trip without display rewrite |
| A10 | root/include/repeated-source, generated/unknown and authorization |
| A11 | unchanged Flow Markdown/TLDR behavior, strict Flow/Fixed wire rejection |
| A12 | CLI plain/ANSI/pipe/pager and real TUI resize/hit/reveal/copy |
| A13 | four representative pages' full body and fair load/RSS/phase comparison |
| A14 | ABI/lifetime, fallible failure, budgets, reentry/concurrency and feature matrix |
| A15 | final legacy removal and replay (later R09/R10, not a P1 gate) |
| A16 | section parent/direct/subtree/root/empty/repeated heading without duplicate body |
| A17 | no TTY/stdout dependency; full owned success or error, next call recovers |
| A18 | visible/Markdown Flow/Fixed/TLDR search, occurrence pagination and exact budgeted total |
| A19 | coverage loss persists through query, excerpt, truncation and wire |
| A20 | overwritten marks, stable keys, visible search/link/name/source, bounded remap |

P1 uses a temporary, feature-gated `mant/annotated-preview` developer
entrance in the *actual* CLI process: a hidden `--annotated-preview` selector
is valid only with explicit roff `--input` and routes that one page through
the private annotated→Fixed chain. It feeds the same query, presentation,
pager and TUI consumer functions as ordinary input. The default `--input`
path remains old until R08; R08 removes the preview selector while making
annotated the only normal roff route. The preview has no per-page fallback,
no external source authority and is not a third renderer. R03/G1 run both
the true preview process and the unchanged default as different evidence.
Library-only Buffer or debug-dump tests cannot substitute for that process
check. Fixed pager must not soft-wrap; real interaction verifies horizontal
view, not only its initial screen buffer.

G1 command shape once `annotated-preview` exists, for each of the four
root-relative fixture paths in the P0 baseline:

```sh
cargo build --locked --release -p mant --features annotated-preview
target/release/mant --annotated-preview --input PATH --input-format roff --format text --display direct --color never
target/release/mant --annotated-preview --input PATH --input-format roff --outline --outline-entries all --format json --compact
target/release/mant --annotated-preview --input PATH --input-format roff --explain=QUERY --format json --compact
target/release/mant --annotated-preview --input PATH --input-format roff --search=QUERY --scope visible --format json --compact
target/release/mant --annotated-preview --input PATH --input-format roff --format markdown
target/release/mant --annotated-preview --input PATH --input-format roff --display pager
target/release/mant --annotated-preview --input PATH --input-format roff --display tui
```

`PATH`/`QUERY` are placeholders, not literal shell variables; the baseline
lists exact paths and query terms. The P1 viewport probe must inject *that
same Fixed result* into a real Ratatui Buffer at widths 20/40/78/120. Retain
per-page full body byte hash, row count, output/mark/sidecar counts, raw CVS
hash, framed CLI body hash, per-viewport Buffer sample and the probe's
`complete` flag. `geometry_audit` currently limits its JSON to 20,000 rows
and 1,000,000 cells; `complete=false` is sampling, not a full G1 pass.
Interactive pager/TUI checks record terminal behavior separately. An
unmodified old CLI pass and a library-only Fixed test do not satisfy G1.

`A15` is recorded now but cannot pass until the later removal stage; P1 must
not falsely claim it. R01 native input without TTY may be captured in memory;
the API exposes no half-result or live streaming. Parser-only, render-only,
annotated-only and render+annotated feature builds remain independent. Before
`annotated` exists, baseline checks use the actual current `structured`
feature rather than claiming a planned command ran.

Stage commands after the new feature exists:

```sh
cargo fmt --all -- --check
git diff --check
cargo test --locked -p libmandoc-rs --no-default-features
cargo test --locked -p libmandoc-rs --no-default-features --features render
cargo test --locked -p libmandoc-rs --no-default-features --features annotated
cargo test --locked -p libmandoc-rs --no-default-features --features render,annotated
cargo test --locked -p mant-codec --no-default-features
cargo test --locked -p mant-codec --features roff
cargo test --locked --workspace --all-features
```

Native output differentials, package contents, strict lint, replay and true
CLI/TUI consumers are separate checks; an all-feature workspace run does not
replace the four independent libmandoc feature runs. P1 does not switch the
default loader or delete the old patches. No push or release is implied.

Build in the repository `target` only. Before a roff behavior assertion,
run that exact input through the registered fixed CVS reference and inspect
its execution path. R01 cross-review covers native output and FFI; R02a
covers FFI/IR/wire; R02b covers native-to-query semantics; R03/G1 cover real
consumer geometry and performance. Reviews are read-only against a fixed
candidate; the main developer coordinates builds. All results distinguish
executed, statically checked and CI-only verification.
