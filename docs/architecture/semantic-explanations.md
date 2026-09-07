# Semantic entries and explanation design

Status: implemented content ownership, Markdown relationship authoring and
multi-evidence explanation contract for the unreleased v0.11 family. This
document records design decisions; the normative syntax lives in
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

The reviewed baseline did **not** satisfy this invariant. It
converted accepted Markdown `List` items into `DefinitionItem` values and
separates their terms from descriptions. For example, visible `:` and `—`
delimiters disappear and a form-separating `|` can render as a comma. This
required a content/semantic ownership change, not renderer punctuation patches.

The ordinary-owner migration now removes this conversion. Regression tests
compare the same original parser events before and after annotation, including
ordered/nested lists, invalid siblings, line endings and head punctuation.
Per-entry relationship authoring now uses original list-item identities across
comment removal. All public explanation adapters now use the independent
collector rather than strict navigation.

The follow-up owner checks now distinguish partial child extraction from an
author's exhaustive-choice claim, validate links to all indexed content owners,
and keep inline first-paragraph hanging layout separate from later block
coordinates. These correctness repairs remain independently tested alongside
the explanation collector.

The target model attaches common entry facts to both ordinary `ListItem` and
native `DefinitionItem` owners. Keep one authoritative content tree; derive
the semantic index and query views from it. Do not store independently mutable
original and semantic copies of the body. Read-only name/form bindings must
refer to validated positions in the final IR, not offsets into a discarded
parser buffer. Every consumer must support both owner kinds, including
nested entries, navigation, scope links, value domains and TUI anchors.

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
Current `names` / projected `aliases` fields expose selector spellings, not
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
1 byte–4 MiB of copied forms/facts/body payload (default 1 MiB). The collector
indexes at most 10,000 matching owners per document, follows at most 4,096 explicit edges per document
and retains chains of at most 32 edges. These bounds report independent
`truncation` fields. Oversized owner bodies are omitted atomically, with real
outline positions retained for strict reads; they are never partial valid IR.
Scope requests share one result offset, limit and content-copy budget over
breadth-first document order, then each document's IR order. These are payload
copy limits, not a bound on serialized envelopes and metadata. MCP character
paging slices the completed canonical presentation independently.

Direct name and complete-form evidence follow the owner's case policy;
ordinary paragraph, preformatted, equation and preserved-source support uses
case-sensitive literal token boundaries. It does not expand names, shorten
options or infer relationships from punctuation/prose. Root/section support
has a block/item/cell coordinate, not a manufactured entry ID. Evidence follows
IR source order; a parent and its matching child remain separate. Explicit
aliasOf edges may be traversed in either direction to collect independent
owners, with declaration IDs recorded. This neither changes relation direction
in the IR nor inherits the remote owner's value domain. Quick-reference-only
content currently has no full-document semantic evidence; no-evidence is not a
claim that its command examples contain no useful information.

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

The subsequent internal refactor and its exact-output checks are recorded in
[semantic refactor verification](semantic-refactor-verification.md).

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

## Acceptance record (2026-09-07)

Implementation and regression baseline:
`05d542dfb7a0de3953af02b43e28b52285d6005a`. The subsequent record-only
documentation commit does not change the tested code. This is a development
verification record, not a release or publication assertion.

The original-item repair, closed metadata authoring, semantic Markdown export,
independent collector, CLI/request/scope/MCP adapters, explicit relationship
projection and independent audit counters are implemented. The old unique
explanation helper and its prose-only failure path have been removed; strict
navigation remains independently tested. Review documents were retained for
external re-review, not deleted or treated as automatically approved.

Completed against this baseline on Linux:

```sh
cargo run --locked -p mant --example generate_help_tldr
bash scripts/update-protocol-schema-snapshot.sh
cargo test --locked --workspace --all-features
bash scripts/check.sh
```

- Full-feature workspace: 1,290 passing tests across 54 suites, six existing
  ignored tests. The source-update tests use local loopback HTTP servers;
  sandbox-denied binds were rerun with permission rather than called defects.
- The complete gate passed formatting, script/audit self-checks, workspace
  and profiler tests, optional native renderers, symbol checks, independently
  packaged crate tests, read-only feature checks, strict rustdoc and Clippy,
  fuzz compilation, optimized build and executable smoke tests.
- Fixture projection, target and semantic audits each examined 37 pages:
  all clean, zero review/hard findings. Projection included 106 excerpts;
  semantic audit observed 10,725 entries with no ordinal, empty-entry,
  value-domain or conversion violations. Counts are not recall proofs.
- Independent process tests cover shared-name owners, explicit relationships,
  strict navigation, global offset/limit/content budgets, no-evidence,
  partially/wholly unreadable sources, and MCP character pages. Metadata tests
  compare original parser events and preserve bindings/serde/export semantics.
- Existing licensed GCC/Bash fixtures now also pin independent explanation
  owners. GCC's two named help definitions both survive, while all nine classes
  and qualifiers and the trailing examples remain under parameterized help.
  Bash's `history` builtin and nested option value remain distinct evidence.

Local installed-manual spot checks additionally covered GCC `--help`, Bash
`bind`/`echo`/`enable`/`set`/`history`/`complete`, and Git, ls, cp and tar `--help`.
Assertions inspect explicit `name` bases and role/body ownership, not just exit
status or whichever record is first. These decompressed source SHA-256 values
identify the inputs under `/usr/share/man/man1/`:

```text
gcc.1   f31a9ae03e8e20baad471d47c4e17391038d852e32719c101001da6eb52b3083
bash.1  23f7bd155c57125864094d2cb4e1f6ec9f8c2ea002e18bebeb3ed4bcfaad1971
git.1   2736b9d20cd9a36c7971ccdbd7ba5e71cea543960340bb9a27984369df559862
ls.1    b38365a9b33e2fe2f6773b08e7d72f11f7706e8f91e8a78c40557cce2e862fab
cp.1    990b23666751eb0990de858776adccbf132cb59128af616e3e3b3376541d2d0f
tar.1   3a85ebdd1601114e7c8c1dfa35726f8394d59412887242ffc6ef49ec021017ee
```

The final `target/release/mant` (0.11.0) SHA-256 was
`b511736c6c24f7dfe8680966545c2060d00593e447c4d0773dba02f851f47436`.
Its GCC explanation succeeded with nine evidence owners, including two `name`
matches at `4.2/e38` and `4.2/e40`; strict navigation still disambiguates them.
Detailed transient logs were kept under `/tmp/mant-migration-*`; the baseline,
commands and conclusions above do not depend on those files surviving.

This run did not perform native Windows/MSVC verification or a new all-host
45,036-page corpus sweep. Linux checks and targeted examples are not substitutes
for those independent platform/corpus checks. No push, main synchronization,
tag or release was performed.
