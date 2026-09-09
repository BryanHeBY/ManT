# Document content and reference navigation

This is the implementation contract for the unreleased v0.11 navigation work.
The migration is ordered: authoritative headings, bounded reference traversal,
protocol projection and selectors, scope/CLI/MCP, then interactive navigation.
It does not introduce a new crate or a second document model.

## Content and positions

`Heading { content: Vec<Inline>, source: Option<SourceSpan> }` owns displayed
title content. `Document.heading` is optional; `Section.heading` is required.
Section title strings are derived, never independently stored. A leading
Markdown H1 moves into `Document.heading`, not into metadata or a duplicate
paragraph. Native TH/Dt metadata remains in `DocumentMeta`; a native document
does not acquire a synthetic content heading. Plain labels use the visible
heading when present and bibliographic metadata otherwise.

`ContentLocation` addresses the final IR with a closed content-root union:
`document-heading`, `section-heading`, or `content`. Section coordinates are
zero-based child indices; an empty section path denotes root content. Content
paths use typed block, list-item, definition-item and table-cell steps. A final
`inlines` or `definition-term` root plus inline child indices identifies the
actual inline node. Every step is checked against its container and bounds.
Positions never depend on a semantic name, navigation filter, page number,
viewport or source-line heuristic.

Entry-local term/block slices map through their actual owner into these paths.
An excerpt/explanation producer explicitly rebases positions into returned
content; the frontend does not search for a matching label. Source UTF-8 bytes,
final IR leaf byte slices, projected Unicode scalar ranges and terminal cells
remain separate units. An empty-label link still has a structural location.

## Reference facts

An occurrence is a borrowed `Inline::Link` plus its `ContentLocation` and nearest
valid content/semantic owner. Its structural location is its snapshot-local
identity. No persistent duplicate link array is added to Document. A streaming
visitor can be stopped before another node or path is materialized; callers
need not build `SemanticIndex` or project all forms to obtain a summary.

The neutral `DocumentReference` type is shared by semantic relations and scope.
Only document/manual targets are accepted by entry-set relations. Entry sets
remain semantic relations, not visible link occurrences. Valid form slices
associate with original link occurrences; they never create a second edge.

Occurrence inventory preserves duplicates and source order. Navigation may
group by owner and complete typed target, including fragment, but preserves
all occurrence locations and labels. Scope separately deduplicates loaded
DocumentAddress values and retains its bounded BFS. No layer infers entry
names, aliases, or definitions from ordinary links.

## Projection and budgets

`ReferenceProjection` independently selects `none`, `summary` (default), or
`all`, with target types (default document/manual), occurrence offset and limit.
`QueryOutline.references` contains policy, scan coverage, occurrence/target
counts, page metadata and records. Records include structural origin, label,
typed target, optional validated form association and resolution state. A
`sourceRead` selector identifies a containing readable source subtree; it is
not the occurrence position and does not follow the target.
Entry-kind filtering does not prune the source range of reference traversal;
an explicit outline root does. Pagination counts occurrences, not groups.

Counts are closed variants `exact { value }`, `lower-bound { value }` and
`unknown { reason }`. Scan completion, count precision and returned truncation
are separate facts. A fully scanned page can have an exact occurrence count
but only a lower bound for distinct targets.

Initial operation limits are deliberately independent of the input-file cap:

| Resource | Default / hard ceiling | Exhaustion |
| --- | --- | --- |
| Traversal steps, including skipped offsets | 250,000 / 1,000,000 | Stop scan; occurrence count becomes lower bound |
| Structural/inline depth | 256 / 256 | Mark coverage limited; never recurse unboundedly |
| Inspected target/label/form bytes | 8 MiB / 32 MiB | Stop affected scan/association with explicit coverage |
| Distinct target set | 4,096 targets and 1 MiB keys / same | Freeze proven set; target count becomes lower bound |
| Materialized records | 100 / 1,000 | Return bounded occurrence page |
| One encoded position | 8 KiB / same | Report position limit, never substitute guessed identity |
| One label | 4 KiB / same | UTF-8-safe display truncation, explicitly marked |
| Total record/position/label materialization | 256 KiB / 1 MiB | Return truncated page independently of scan state |

Summary does not clone labels or full occurrence paths for every link. Form
associations inspect bounded original bindings, not materialized form copies.
Offset work consumes the same scan budget. A continuation is advertised only
when a fresh bounded scan can reach it; exhausted scan budgets do not promise
an executable cursor. No incomplete cache is treated as a complete inventory.
Git/GCC/PowerShell and repeated-target stress inputs are required performance
checks; limits are not justification for hidden unbounded preprocessing.

## Read selectors and snapshot scope

`ContentSelector` is a closed union, serialized as
`{"kind":"path","path":"4.2/e27"}` or
`{"kind":"id","id":"option-x-734e1feafda9"}`.
CLI uses `path:4.2/e27` and `id:option-x-734e1feafda9`; canonical bare structural
paths may be accepted syntactically, with no fallback after failure. Read,
excerpt and outline-root share the same contract. Names, shorthand, URI,
fragments and reference locations are not read selectors. Explain retains its
independent semantic evidence policy.

Read returns an existing local content owner, including an entry's original
item. It never follows that item's document target. Local zero-width anchors
and references may be revealed but are not fabricated readable subtrees.
Remove diagnostics serving only obsolete name-based read selection; retain
duplicate identities, invalid bindings and conflicting explicit relationships.

This release guarantees selection only within the actual loaded IR snapshot.
It does **not** deliver cross-call expected-snapshot tokens or stale-address
detection. After edits, callers must rediscover: an old path can remain legal
but name a different item. Typed selectors alone cannot detect that situation.
Occurrence positions likewise must not be reused as cross-version identities.

## Resolution and authority

Reference resolution is a closed staged union: external/email are
`not-applicable`; local targets are checked against the loaded source document;
cross-document targets begin `not-queried`, `missing-context` or
`logical-address`; forbidden namespace/address syntax is `restricted`.
Outline does not perform catalog existence lookup. A later explicit host open
can fail because the document is missing or ambiguous; those are host errors,
not invented reference-inventory states. Only a loaded
target can contain a fragment result of `absent`, `valid`, `missing` or
`ambiguous` (or `limited` when the target scan is incomplete); unloaded targets
with a fragment remain `unchecked`.
Read failure is not fragment absence. Valid results carry an exact reveal
location in that loaded document. Original typed targets and fragments are
never replaced with guesses. A section-less manual goes through the resolver;
the UI must not supply section 1 or 8.

Ordinary outline never loads target documents. Direct-file inputs without a
registered namespace expose references without arbitrary path resolution.
External/email targets are never probed for existence. URI decoding occurs
once at the existing input boundary, not again during traversal or activation.

TUI content selection keeps local preview/reveal behavior. Reference groups
are collapsed by default; selecting a reference reveals its source, and Enter
explicitly opens it. Failure or ambiguity retains the source page/selection.
Copy content copies local source content; copy reference copies its target.
On a reference row, Shift+Y copies the target. Resolved catalog addresses are
opened exactly, without falling back to another source or a suffix match.
Back/forward and resize use content positions, never screen-row identities.

MCP remains read-only: it returns references, local resolution information and
subsequent read parameters, never launches browsers, shells or URI handlers.
Only an explicit user action in the interactive host reaches the safe opener.
A typed action in a DTO is data, not permission to execute it.
