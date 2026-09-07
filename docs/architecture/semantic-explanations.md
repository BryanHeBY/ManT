# Semantic entries and explanation design

Status: design direction and implementation constraints, recorded on 2026-09-07
against `7b7ad00944902142cafb37b1e64102ef2dd53406`. This is not an implemented
API or authoring reference. Current syntax lives in
[mant-markdown(7)](../manuals/mant-markdown.md); current query behavior lives in
[mant-protocol(5)](../manuals/mant-protocol.md).

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

The present implementation does **not** fully satisfy this invariant. It
converts accepted Markdown `List` items into `DefinitionItem` values and
separates their terms from descriptions. For example, visible `:` and `—`
delimiters disappear and a form-separating `|` can render as a comma. This
needs a content/semantic ownership change, not renderer punctuation patches.

The target model attaches common entry facts to both ordinary `ListItem` and
native `DefinitionItem` owners. Keep one authoritative content tree; derive
the semantic index and query views from it. Do not store independently mutable
original and semantic copies of the body. Read-only name/form bindings must
refer to validated positions in the final IR, not offsets into a discarded
parser buffer. Every consumer must support both owner kinds, including
nested entries, navigation, scope links, value domains and TUI anchors.

Metadata must not invent a documented name absent from the visible definition
head, move children, generate visible choices, or replace a description.
Unknown or invalid facts must not erase the underlying content. The planned
comment handling must keep invalid metadata non-visible and report it through
diagnostics; that is a target requirement, not a claim that current handling of
all HTML comments already does so. This principle applies to semantic comments,
not a promise that existing heading attributes or embedded TLDR directives
are interpreted by every ordinary Markdown viewer.

## Names, shared descriptions and aliases are different

A content entry may describe several subjects. Its names identify that
content; sharing a description does not establish behavioral equivalence.
Current `names` / projected `aliases` fields expose selector spellings, not
proof that the corresponding options can be substituted for one another.
Complete forms preserve documented usage, not an executable argv grammar.

The proposed model separates documented names, explicit same-owner
`aliasGroups`, and explicit same-document `aliasOf` relationships between
independent entries. These names are design vocabulary, **not supported
Markdown fields yet**. In particular, grouping `-S`, `--since`, `-U`, `--until`
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

Current explain deliberately selects one semantic entry and reports selector
ambiguity. Returning several supporting records is a product/API change, not
a correction to that existing documented resolver contract.

The proposed explanation collector returns relevant evidence from an immutable
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
a unique excerpt. The proposed normal no-evidence response, partial-source
failures and multi-result success need explicit outcomes and a coordinated
CLI/JSON/MCP migration, including exit codes. Until that migration lands,
current help, error behavior and client examples remain authoritative.

Validation findings, available source coverage, result truncation and unknown
fields are separate dimensions. Neither `semanticsComplete` nor a clean audit
proves complete name recall or correct knowledge of an executable's behavior.
No evidence collector may execute examples, inspect real environment values,
follow external URLs or enlarge the existing local source/query authority.

## Lowering rules and confirmed gaps

Review probes reproduced these issues in the baseline release-profile binary:

| Input evidence | Observed problem | Required boundary |
| --- | --- | --- |
| Linked bold `-a, --all` in a native definition | Only `-a` is selectable | Traverse visible link children with the same style-aware name rules |
| man `.BI -L dir` | Name becomes `-Ldir` | Retain the adjacent parameter boundary while preserving the full form |
| man `.B launch -p [\fB-x\fP]` | Name becomes `launch -p [` | A bold run is not necessarily a complete command name |
| Declared Markdown entries with `:` / `—` / `|` | Delimiters or structure change on rendering | Attach facts without replacing the ordinary content owner |

Fix these through a shared style-aware traversal and finite role-specific
lexical rules, not tool-name cases or progressively larger string templates.
Preserve explicit native role evidence at definition owners; a role-bearing
inline mention in prose does not itself define an entry. Do not infer alias
relations from English phrases such as “same as”. Unknown evidence stays
unknown while the source remains readable.

Separate fidelity normalization from semantic inference before removing
heuristics. Existing target, continuation, indentation, table and line-flow
repairs remain required. Do not erase a normalization module or all section
context merely because some classification rules are too broad.

## Implementation order and acceptance

1. Preserve independent minimal regressions and inventory existing rules.
   Decide migration scope and request/result budgets before changing fields.
2. Introduce common content-owner facts and validated bindings; update every
   index/visitor/consumer without a second authoritative content tree.
3. Fix the style-aware extraction gaps with positive and negative cases.
4. Replace Markdown structural conversion with annotation, then introduce
   explicitly specified metadata and its failure/relationship validation.
5. Add the evidence collector and coordinate single-document, scope, CLI,
   JSON and MCP contracts. Retain strict navigation independently.
6. Migrate docs, Rustdoc, schema, self-manual assertions and independent audits;
   run the complete existing verification boundary before claiming completion.

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
the self-manual's independent clap comparison while adapting its uniqueness
assertions only when the new explain contract is implemented.

Version decisions follow [the release policy](../releasing.md), not the
presence of a snapshot file alone. Released/frozen contracts must not be
overwritten. An explicitly unreleased family may be revised under the stated
policy with coordinated examples, schemas and changelog. Record the actual
release/freeze status and affected independently versioned crates before
choosing the family; this design does not choose a new version or authorize
publication.

## Migration decision and rule inventory

The maintainer authorized implementation on 2026-09-07 and confirmed that
the current `v0.11` family is unreleased. Revise that family in place, including
its structural snapshot, examples and changelog; retain unrelated source-update,
doctor and native parser contracts. Crates remain independently versioned.
No tag, publication or main-branch synchronization is part of this work.

The initial rule inventory distinguishes the following responsibilities:

| Existing owner | Preserve | Change |
| --- | --- | --- |
| `definitions/syntax/forms` | Styled source order, explicit separators, opaque parameters | One transparent-wrapper traversal; stop names at parameter boundaries |
| `definitions/syntax/options` | Literal fixed names and conservative prefix grammar | Never derive names by flattening styled arguments |
| `definitions/syntax/commands` | Declared multiword names and authored forms | Bold runs cannot bypass invocation-boundary checks |
| `definitions/context` | Explicit roles, parent ownership, native name evidence | Treat title/name heuristics as inference, not behavioral or alias proof |
| `definitions/normalize` | Continuation, nesting and content conservation | Do not use layout repair to manufacture alias equivalence |
| `markdown/entries` | Original parser topology, explicit role/case/value-domain checks | Annotate each ordinary item; no destructive signature extraction |
| `selectors` | Unique exact navigation and deterministic ambiguity | Do not use uniqueness as explanation collection policy |
| `projection/excerpt`, `scope/execute` | Source/context provenance and authority limits | Dedicated multi-evidence results; coverage distinct from validation |

Implementation commits must state which boundary they migrate and retain
independent regression expectations. Transitional internal helpers are permitted
only while their consumers migrate; they must not become a second parser or
independently mutable body. The full migration is not complete until ordinary
list owners participate in every consumer and the public explanation surfaces
agree.
