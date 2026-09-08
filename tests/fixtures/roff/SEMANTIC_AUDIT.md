# Roff semantic-entry precision audit

`SEMANTIC_AUDIT.csv` records an independent audit of semantic entries created
after native roff lowering. Target conservation proves that zero-width
destinations survive; it cannot prove that numbered prose, placement-only
definitions, or invalid value-domain children were not promoted into the
agent-visible semantic index.

## Oracle and scope

The separate [query gold](ENTRY_QUERY_GOLD.json) preserves the original 160
manually inspected queries and adds declaration-context coverage (199 queries
over 125 source paths). It
records raw source coordinates, complete returned forms, exact names/kinds,
bounded opening-body witnesses and all expected direct owners. Extra direct
owners fail too; supplementary mention evidence is allowed. Empty independent
declarations must stay empty, not borrow the next description. `expectedSupports`
separately checks exact member sources/forms, the final description's head,
middle and tail witnesses, required table/code/link structure and absence of
invented alias relationships. A `declaration-member` response is resolved back
to its physical owner before applying the unchanged original body assertions. These are
selected witnesses, not whole-page golden renderings or a precision/recall
estimate for all entries. Per-query notes distinguish exact common declarations
from accepted conservative classifications and local macro/format syntax.
Source-group, member path, SHA and original sampling reasons remain in the
manifest. Opening-body witnesses are normalized eight-word hashes, not copied
manual prose; inspect the original source and local result when adjudicating a
failure. These hashes guard the reviewed witness, not the entire body. Raw
third-party pages are not redistributed by the manifest.

`audit-roff-semantics.py --query-gold ENTRY_QUERY_GOLD.json` uses the existing
collector via optional profiler probes. The `--fixtures` subset selects only
the checked-in source root and is included in `scripts/check.sh`. Relocate the
external corpus with `--query-root corpus=/path/to/corpus`. Native AST matches
are coordinate candidates for a reviewer to inspect, not another classifier or
proof of owner equality. The comparator combines source coordinates with exact
forms, names, kinds and body witnesses; it refuses incomplete/omitted evidence.
Unreviewed or missing-input probes remain unresolved rather than silently clean.

### Fixed-panel acceptance: 2026-09-08

The final implementation/test producer is `cedbd7cd`, compared with the saved
`483dd1a0` baseline. [ENTRY_QUERY_CHANGES.json](ENTRY_QUERY_CHANGES.json) retains
the source-identical 120-path delta, manifest and binary hashes, per-query
before/after owners, names, kinds, forms, body hashes, IDs, paths and relationship
changes. It contains no full manual bodies. The baseline full QueryBundles were
captured before editing; the baseline *query* comparison replays those bundles
through the final collector, not the old CLI executable. Snapshot digests identify
the larger local evidence; the checked-in delta and current gold remain usable
without those snapshots. Body hashes in the page census include IR layout; query
body hashes represent visible text. Neither substitutes for source review.

All 160 selected queries passed, including 10 negative queries and exactly 229
expected/returned direct owners; none were unresolved. Eighty-three queries
changed. The page census grew from 24,676 to 28,484 owners: independent compact
heads no longer borrow bodies, complete PP/RS declarations gain owners, and TP
bullets/prose-like labels cease to be semantic entries. Names and types also
change with complete-head parsing and local/native evidence. No same-source
owner acquired an inferred alias group, alias reference or value domain.
These are selected-query results, not whole-page precision/recall percentages.
Normal controls, malformed-source negatives, multiple direct owners and empty
independent declarations are kept alongside positive recovery cases. Query notes
explicitly retain conservative long-tail classifications rather than hiding them
in an Option-or-Term assertion. The final delta review additionally caught the
logger `--sd-id` and Git `--trailer` compact-argument heads; their positive gold
and source-shaped regressions are included. Existing gold must not be regenerated
from observed output merely to clear a failure.

Replay the current panel and repository-only subset with:

```sh
cargo build --locked -p mant-engine --example roff_semantic_profile
python3 scripts/audit-roff-semantics.py \
  --query-gold tests/fixtures/roff/ENTRY_QUERY_GOLD.json
python3 scripts/audit-roff-semantics.py --fixtures \
  --query-gold tests/fixtures/roff/ENTRY_QUERY_GOLD.json
```

The full `scripts/check.sh` gate passed at this producer, with temporary package
builds kept under the repository's `target/` via `TMPDIR`. A separate workspace
`--all-features` run passed 1,558 tests, with 35 ignored (including 29 upstream
minus API doctests that do not describe the private embedding). The public UI
README doctest, native pager tests and real Unix PTY checks execute. Formatting,
strict Clippy/rustdoc, fuzz compilation, packaged-crate verification, release
build and smoke passed. Projection, target, semantic and structure audits each
checked all 51 current fixtures clean; the seven checked-in query probes also
passed. This does not supersede historical 45,036-page sweep evidence.

Fresh differential checks used groff 1.24.1 through man-db 2.13.1 and mandoc
1.14.6 (host package `mandoc-noconflict 1.14.6-1`). Groff fidelity reported
46 clean, four reviewed differences and one expected recursive-input formatter
failure; groff layout reported 49 clean, one reviewed difference and that same
failure. Mandoc fidelity reported 45 clean and six reviewed differences; its
layout reported 50 clean and one reviewed difference. The per-source ledgers
retain their review conclusions, including the `Fa` prose punctuation/spacing
comparison. These reference differences are **not** relabeled clean. Commands
use `--fixtures --recheck-recorded --findings-only`; mandoc additionally uses
`--reference mandoc --reference-kind mandoc --reference-id mandoc-1.14.6-1`.

Five sequential debug runs of each real `gcc --manual` route, with output
discarded, yielded the following median seconds and maximum RSS in KiB. The
checked-in delta includes all samples and exact command arguments.

| Route | Before / after seconds | Before / after peak KiB |
| --- | --- | --- |
| Full text | 1.093 / 1.197 | 92,088 / 95,452 |
| Outline JSON | 1.064 / 1.174 | 91,936 / 95,392 |
| Explain JSON | 1.148 / 1.229 | 92,456 / 96,036 |
| Colored explain | 1.138 / 1.226 | 92,732 / 96,032 |

This is a measured 7–10% time and 3.6–3.9% peak-memory increase, not a zero-cost
refactor. The same GCC source now contains 4,918 independent owners instead of
3,837, among other recognition changes; the measurements do not isolate one
cause. Runs used a warm machine, not controlled cold caches. Native Windows
and macOS execution was not available in this Linux acceptance run.

The development-only `roff_semantic_profile` example parses and lowers each
page once, builds the final `SemanticIndex`, and records each entry's ID, kind,
selectable names (the legacy `aliases` field), explicit `aliasGroups` / `aliasOf`, visible forms, targets, containing section, nested depth, and
value-domain origin. Profile schema `mant.roff-semantic-profile/v4` names the
selectable spellings `names`, aligned with the IR and outline rather than
implying alias equivalence. Historical v1/v2/v3 ledger rows remain readable; a new
scan records v4 explicitly. Like v2, the profiler also walks
the IR definition lists independently so an ordinal that failed to become a
list cannot hide merely because semantic discovery declined it. Independently,
it derives every punctuated ordinal candidate from the original owned mdoc AST,
including the authored `-tag`, `-diag`, `-hang`, `-inset`, or `-ohang` subtype
and complete term sequence, then matches that candidate to the final IR block
at the same source line. This source-side conversion ledger detects both a
qualifying `-tag` sequence that was not recovered and a non-tag definition
whose terms were incorrectly deleted by over-conversion.

Version 4 adds independent bidirectional declaration-run accounting. Every
candidate run records native AST paths, source coordinates, retained/rejected
status and a reason; every observed IR group must match one source run. A
recognized but unexplained run, invalid group or unbacked group is review
evidence. Missing groups and relocated owners have adversarial profiler tests.
Nameless/template and non-definition candidates retain their rejection reasons
for inspection rather than becoming newly inferred names. This is not a full
syntax oracle: a reasoned rejection does not prove the author's intent, and
native ASTs can omit source-only whitespace requests. The query gold's original
source witnesses and explicit-boundary product tests are independent checks.
Gold result reports include per-source run counts and decision histograms, so
retained-group counts alone cannot stand in for explanation completeness.

The following are high-confidence review findings:

- a punctuated integer definition such as `1.`, `2)`, `(3)`, or `[4]` remains
  in a definition list;
- such a form becomes a `term` or `value` semantic entry;
- an entry has neither a semantic alias nor any visible form; or
- a `Choices` value domain contains a child whose kind is not `value`;
- a complete consecutive `Bl -tag` ordinal sequence does not become an ordered
  list; or
- an ordinal candidate from any other mdoc definition-list style does not
  remain a definition list.

The additive `relationshipCounts` object counts names, explicit groups, group
members and aliasOf declarations independently. Native shared names never
become declared aliases merely because they share content. Shared IR semantic
validation findings are deduplicated into
`semanticViolations` and included in `violations`; `semanticsComplete` is false
when these findings are present. Producer coverage (for example deliberately
unclassified native terms) can also make it false without a precision violation.
The driver validates this category explicitly,
so an invalid name binding is review evidence, not an invalid profiler response.
An independent Markdown test exercises
the same final-index counter with shared names, declared groups and aliasOf;
zero native relation counts are expected, not a recall metric.

The profile separately counts aliasless generic terms and entries below
NOTES, FOOTNOTES, or REFERENCES headings. Those are sampling signals, not
automatic failures: real manuals can intentionally define terms in those
sections, and a source-neutral audit must not turn an English heading guess
into a lowering rule.

Rows use `(corpus, path, decompressed-source SHA-256)` identity. A `review` or
`hard-failure` starts as `pending` and requires a durable `false-positive`,
`confirmed-open`, or `confirmed-fixed` conclusion with a useful note. Confirmed
product defects become licensed real fixtures and focused Rust tests. The JSON
report additionally records the producer commit, profiler binary SHA-256,
timestamp, corpus roots, page count, entry count, and kind totals so a broad
scan cannot be cited independently of the code that produced it.

## Reproducible fixture gate

The checked-in corpus is part of the Unix verification boundary:

```sh
cargo build --locked -p mant-engine --example roff_semantic_profile
python3 scripts/audit-roff-semantics.py --fixtures --recheck-recorded \
  --verify --findings-only
```

The fixture gate at implementation/test commit `05d542df` contains 37 clean pages and 10,725 semantic
entries. It has no punctuated ordinal definitions or entries, empty entries,
or value-domain violations. Aliasless generic and note-like counts remain
visible in the command summary for deliberate sampling.
The two-count difference from the earlier recorded inventory is not an alias
relationship count; names and explicit equivalence remain separate measures.
See the [migration acceptance record](../../../docs/architecture/semantic-explanations.md#acceptance-record-2026-09-07)
for the exact verification baseline and limits of this run.

## Distribution sweep

Run a complete local hierarchy as release-time evidence rather than making
host manuals a CI dependency:

```sh
python3 scripts/audit-roff-semantics.py --manpath /usr/share/man \
  --corpus archlinux-host --recheck-recorded --findings-only \
  --json /tmp/mant-semantic-archlinux-v2.json
```

Keep the JSON report under `/tmp`; it can contain host-specific paths and
bounded entry samples. Record the exact producer commit, profiler hash, corpus
identity, page/entry totals, and reviewed findings here when a complete sweep
is accepted. A clean automated scan is evidence for the checks above, not a
general proof that every source's semantic classification is ideal.

On 2026-09-04 the complete local Arch Linux hierarchy was scanned at producer
commit `c51be9d8a539080dd8f53e8812386d8c7b4f782e`. The profiler binary SHA-256
was `2d81071c571963bbbef607908665b3392febb7f037aa5715b1bbb09412b6e80e`.
All 28,712 pages were clean. Their 347,943 semantic entries contained no
punctuated ordinal entries, retained ordinal definitions, empty entries, or
value-domain violations. The informational census found 241,323 aliasless
generic terms and 6,914 entries in note-like sections; these remain sampling
populations rather than automated defects. The exact JSON report is
`/tmp/mant-semantic-archlinux-v1-c51be9d.json` on the audit host.

The broader 45,036-path restored logical inventory described in
[TARGET_AUDIT.md](TARGET_AUDIT.md) was rescanned on 2026-09-04 at producer
commit `9233968b21f046bdcf301f5b34954ee43cdd2292`. The v1 semantic profiler
binary SHA-256 was
`597b0db7de4380bafe1e3d39ab1aed219da59f9274e241828c607c6eb52d5b52`.
All 45,036 pages were clean. Their 651,428 semantic entries contained zero
punctuated ordinal entries, retained ordinal definitions, empty entries, or
value-domain violations. The informational census contained 439,875
aliasless generic terms and 10,063 entries in note-like sections; these remain
review populations, not automatic defects. The exact reports on the audit host
are `/tmp/mant-semantic-full-45036-9233968.json` and
`/tmp/mant-semantic-full-45036-9233968.csv`.

After the source-side mdoc ordinal-conversion ledger was added, the same
45,036-path inventory was rescanned with the v2 profiler at producer commit
`a07f6529a399dbd9927edc84b148afdd38a4703f`. The profiler binary SHA-256 was
`a7b259fbcd074c992ea10fc9434bd079ec153e44faf82b75aafb01c38092378e`;
the inventory manifest SHA-256 remained
`a5cd7919d1ac2774a335d077611b47680de8c1babe1b031d31ff8808f568879c`.
All 45,036 pages were clean. Their 651,428 semantic entries contained zero
punctuated ordinal entries, retained ordinal definitions, empty entries, or
value-domain violations. The source/IR ledger observed one eligible
`.Bl -tag` conversion and zero conversion-policy violations; a non-tag conversion
would therefore fail even after its original ordinal terms disappeared from
the IR. The informational aliasless-generic and note-like populations remained
439,875 and 10,063 respectively. The path-bearing local reports are
`/tmp/mant-semantic-full-45036-a07f652.json` and
`/tmp/mant-semantic-full-45036-a07f652.csv`; this checked-in summary and the
manifest digest are the durable, host-independent evidence.
