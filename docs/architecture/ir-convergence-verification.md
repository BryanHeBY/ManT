# IR convergence implementation and evidence

This work implements the updated 2026-09-07 convergence guide. It revises the
unreleased v0.11 contracts, not any published protocol. Stages are committed
independently; this document distinguishes planned contracts from completed work.

## S0: baseline and frozen contract

For the post-implementation review and CI corrections, see the final section
below. The S0–S6 evidence remains historical and is not overwritten by reruns.

Baseline producer: `c9918c0dfd43d7f57cab5e8b32212a8ec8e68191`, clean before
adding the measurement harness. The retained release executable lives at
`target/ir-convergence/baseline-mant`; its native-load companion measures the
public `parse_manual_bytes` API without serialization. Neither benchmark builds
in a temporary directory. Cargo target directories are this repository's
`target/` and `fuzz/target/`; no CARGO_TARGET_DIR override is set.

The fixed corpus is `tests/fixtures/roff/real/archlinux/gcc.1.gz`.
`scripts/measure-ir-convergence.py` records compressed/decompressed hashes,
toolchain, OS, executable hashes, complete argv, seven warmed process timings,
per-mode peak RSS, and full JSON/text/Markdown/outline/explanation output under
`target/ir-convergence/<label>/`. The native executable additionally reports
seven same-process native-load timings, excluding decompression and output.
Later measurements must compare these retained binaries, not rebuild history.

Initial seven warmed process medians: full JSON 216.36 ms / 80,788 KiB peak,
outline 164.65 ms / 64,308 KiB, explain 175.58 ms / 64,480 KiB. The native harness
reported 123.32–131.08 ms per load (126.35 ms median), with 60,292 KiB maximum
process RSS across repetitions. Its 1,113.68 ms process median includes eight
loads and is **not** a single-load measurement. Source SHA-256:
`a86bd1d671aa2afff558eb3b7c8b6e2a8bbaf55cb9fb2a6549b7538dd56e0f0f`;
baseline CLI SHA-256:
`8c9fbf483e949e0191ae7bc189450ccfaccc39f098dd0f99645efb6452a5227e`.

The machine-readable transition examples are
`crates/mant-ir/src/entry/convergence-wire.json`. The `new` cases are the target contract,
not claims about S0's decoder. At S0 old `identity`/role/string list kinds still
decode; ordinary item unknown fields are not yet closed. Each field migration
must activate the respective positive and negative *real deserialization* tests
in the same commit, including duplicate raw keys and nested protocol content.

| Position | Final public/wire contract |
| --- | --- |
| ListItem, DefinitionItem | optional `entry: EntryFacts`, optional original item `source`; never `identity` |
| EntryFacts | `kind: EntryKind`, `case: NameCase`, `names`, explicit `forms` and `nameBindings` |
| SemanticEntry, semantic outline selectors | `names`; outline `entryKind` retains its contextual field name |
| Alias relationships / fragments | `aliasGroups`, `aliasOf`, `FragmentAlias`/`fragmentAliases` unchanged |
| DefinitionItem | `layout: DefinitionLayout`; defaults false/inherited, explicit spacing 0 retained |
| Block::List | tagged `kind` object; only ordered contains optional u64 `start`; no outer start |

Missing/null optional entry and source mean absent. Missing/empty forms mean
unknown, never implicit native terms; null forms are rejected. Missing/empty
layout means default, null layout is rejected. Unknown, duplicate, mixed old/new
fields are rejected. Bad content references remain decodable for typed semantic
diagnostics. Markdown `role=` remains an input spelling, not a second IR type.

## Implementation order

- S1: transparent structural containers and semantic owner boundaries.
- S2: explicit borrowed forms, independent validity, original item source.
- S3: sole facts/kind/name vocabulary and closed wire shapes.
- S4: separate definition layout shape without geometry changes.
- S5: ordered-only start with saturating numbering and excerpt offsets.
- S6: documentation, A01–A15 evidence, corpus and performance comparison.

S1 fixes both the index/choices walker and the contextual navigation walker:
unannotated definition items now behave like unannotated ordinary items. Direct
child traversal stops at annotated owners; full scope traversal is unchanged.
Regression coverage: `entry::tests::choice_validation_uses_direct_entry_ownership_through_containers`,
`list_entry_consumers`, `scope::references`, `entry_choices_coverage`,
`explanation_evidence`. This covers alternating owners, table cells, empty terms,
nearest explanation owner, excerpts, search, nested
choices and source-order links.

The additional table-search gap found in S1 is closed by `8200a503`. Byte ranges
are now composed with the rendered text, rather than recovered from HTML anchor
events. Tests in `list_entry_consumers` cover ordinary and definition owners in
adjacent/nested cells, a table inside a transparent list, parent content before
and after a child, both search scopes, and unchanged text/Markdown with facts
removed. Private mapping tests cover Unicode, CRLF, prefixes and unowned adjacent
text. The bounded table slot plan is shared with plain text output.

S2 implementation uses explicit native term references, a borrowed `EntryForms`
view (including nonconsecutive borrowed terms), exact source-slice name binding,
and original owner source spans. Empty/invalid forms preserve semantic owners
and children; invalid names cannot suppress independent valid Form evidence.
Names, local groups and rejected cross-owner aliases are filtered in derived
metadata, while original facts remain available in content for diagnostics.
The primitive name comparison walks referenced visible text without constructing
styled inline copies. Native `[-+]O` binds the sign and suffix as separate source
pieces, and style boundaries retain adjacent arguments without losing `-L`.

New tests cover pointer-identical borrowing, unknown/invalid form ownership,
missing bindings, independent Form/Name evidence and LF/CRLF/CR original item
positions after removed leading comments. Existing native consumer source tests
now assert the `.It` line, not its containing `.Bl`. UI test fixtures explicitly
record their displayed forms instead of relying on the removed fallback.
S2 verification: workspace/all-features and strict workspace Clippy passed;
the real GCC fixture exposed 18 truncated names containing `+`, fixed separately
in `c2635af4` with man/mdoc regressions. This changes 16 affected/colliding IDs,
not the field migration's ID allocator. All 3,836 owners remain, with text and
Markdown byte-identical to baseline. Repeated validated-name work in selectors
was removed in `67792609`, retaining only snapshot-local borrowed names.

Final S2 comparison uses producer `c5391365`, default-feature release binaries,
and seven alternating old/new rounds recorded in
`target/ir-convergence/s2-cached/measurements.json`:

| Mode | Baseline / S2 median ms | Baseline / S2 peak KiB |
| --- | --- | --- |
| full JSON process | 214.49 / 234.27 | 80,744 / 90,584 |
| outline process | 165.77 / 175.04 | 64,216 / 67,092 |
| explain process | 188.43 / 197.32 | 64,476 / 67,236 |
| native harness (eight loads) | 1,143.30 / 1,194.61 | 60,224 / 63,272 |

The 49 individual timed native loads have median 135.10 / 140.56 ms and ranges
123.68–166.15 / 131.98–194.36 ms. Process times above are not pure parsing.
No final time increase crosses both investigation thresholds. Full JSON peak
memory does: explicit forms/bindings/source metadata expands output from
19,493,365 to 26,809,501 bytes; materialization/serialization requires additional
space. It is not claimed memory-equivalent. Native and non-full-JSON memory
remain below the 5 MiB threshold. No complete body copies or global caches were
introduced into the borrowed forms path.

S3 (`f156bc01`) removes the transitional public types and closes entry/item/
outline decoding, including nested protocol negatives. Workspace all-features
passed 1,328 tests with six intentional ignores; focused post-adaptation CLI,
engine and example tests, strict Clippy and schema guards also passed.
The final S3/S2 alternating comparison is recorded under `target/ir-convergence/s3/`:
full JSON 249.73 / 251.80 ms, outline 187.43 / 182.29 ms, explain 202.09 / 202.26 ms,
native eight-load process 1,209.19 / 1,180.70 ms. Peak RSS respectively
90,960 / 89,984, 66,968 / 66,512, 67,252 / 66,556 and 63,220 / 63,196 KiB.
No mode crosses both investigation thresholds. The 3,836 IDs and both complete
text/Markdown outputs match S2 exactly; this naming migration does not change
the ID allocator. The native figures in this paragraph are process costs.

S4 organizes the two existing item-level presentation fields into
`DefinitionLayout`, preserving every original value and renderer algorithm.
Direct and nested decoders reject old top-level fields; actual default/null/
explicit-zero and duplicate-key tests are in `entry/wire.rs` and
`mant-protocol/tests/entry_convergence.rs`. Existing UI row-coordinate matrices,
native continuation/inline-term tests and the real Arch fixture suite exercise
the migrated consumers. GCC full text and Markdown remain byte-identical to S3.

S5 moves starts into the ordered kind only and centralizes saturating ordinal
and excerpt-offset calculation on `ListKind`. Frozen raw JSON cases now test
tag closure, duplicate keys, null/default starts, negative/fractional/oversized
values and legacy outer start through direct and nested decoders. Excerpt tests
cover unknown/zero/non-one/MAX starts and multiple selected owners, while
retaining the original source kind. GCC text/Markdown are byte-identical to S3;
IR/protocol/UI, engine unit, Arch/Fedora and list-consumer tests pass.

## S6: acceptance coverage

The implementation commits are `a05d6d34` (S0), `beaf79a1` (S1), `3eb4202f`
(S2), `c2635af4` (separate native-name correction), `67792609` (snapshot-local
name reuse), `c5391365` (alternating benchmark harness), `f156bc01` (S3),
`8b031e60` (S4), `93686b80` (S5), `8200a503` (consumer mapping closure),
`8ec1e784` (S6 public documentation), `49e28596` (angle-argument bindings),
`feaaa77f` (semantic audit classification), and `f7df5ca1` (skip absent-group
revalidation after the performance investigation). Published schemas and crate versions were
not changed. The unreleased v0.11 structural snapshot was regenerated during
the field migrations; real decoder tests, not just snapshots, enforce closure.

Crate-prefixed paths below are relative to `crates/`; `tests/fixtures` is at the
repository root. Bare module/test names inherit the named crate. These are focused behavioral evidence,
not a claim that every possible third-party producer has been proved correct.

| Acceptance | Regression / evidence |
| --- | --- |
| A01 public types and closed wire | `mant-ir/src/entry/wire.rs`, `mant-protocol/tests/entry_convergence.rs`, frozen crate-owned `mant-ir/src/entry/convergence-wire.json`: actual direct/nested old, unknown, duplicate, mixed and null decoding |
| A02 content orthogonality | `mant-engine/tests/list_entry_consumers.rs`, `mant-ir/src/entry/content.rs`: facts removed from identical original content, unchanged rendering and owner shape |
| A03 original Markdown events | `mant-engine/src/markdown/tests/source_contracts.rs`: same original event tree with interpretation enabled/disabled; numbering, delimiters, nesting and links |
| A04 explicit forms | `mant-ir/src/entry/content.rs`: borrowed complete/nonconsecutive terms, unknown and invalid forms, UTF-8/path validation, owner/child survival and independent names |
| A05 names and relations | `mant-engine/tests/entry_metadata.rs`, `explanation_evidence.rs`, `mant-ir/src/entry/relations.rs`: exact bindings, groups, target/cycle rejection, independent Form evidence |
| A06 owner boundaries | `mant-ir/src/entry.rs`, `list_entry_consumers.rs`, `scope/references.rs`: transparent definitions/tables, nearest nested owner, full link traversal distinct from direct children |
| A07 original provenance | `entry_choices_coverage.rs::leading_removed_comments_do_not_change_item_ownership`, original-event tests, native source assertions: removed leading comments, LF/CRLF/CR, item rather than containing list |
| A08 local coverage | `entry_choices_coverage.rs`: original and no-first-paragraph rejection cases, valid controls, transparent nesting, independent and nested domains |
| A09 identity and fragments | `entry_fragment_links.rs`, IR index/validation tests, GCC retained outputs: no extra anchor required, dangling/collision findings retained; S3–S5 IDs unchanged |
| A10 ordinal model | IR wire tests and `list_entry_consumers.rs::excerpt_ordinals_preserve_unknown_zero_and_saturated_source_starts`: missing/null/zero/MAX, offsets, multiple excerpts, malformed actual JSON |
| A11 geometry | `inline_terms.rs`, `definition_continuations.rs`, `mant-ui` layout/terminal tests, Arch/Fedora GCC/Git fixtures: first inline versus independent body origin, spacing/code/nesting and retained full outputs |
| A12 all consumers | full workspace (CLI/process/MCP/UI), engine examples, search mapping, explanation class/evidence/preview tests; retained GCC measurements below |
| A13 remote domains | `scope.rs`, `scope/references.rs`, `entry_metadata.rs`, `mant-ir/src/entry.rs`: valid/invalid/unresolved typed edges, source order and bounded acquisition; remote values are not copied into local children |
| A14 round trips | IR/protocol decoder tests, `semantic_markdown_export.rs`, `semantic_export_attached.rs`, protocol snapshots, CLI/MCP stdio tests |
| A15 documentation | executable ordinary/annotated/definition example in `mant-ir/README.md`; engine README, IR/protocol/Markdown/roff manuals, architecture and CHANGELOG reviewed; rustdoc and self-manual tests |

The table abbreviates crate-relative test/module paths after their first use.
Tests and producer inputs are checked in, not dependent on a temporary report.
The standalone example covers three complete constructions and executes as a
doctest. Bad form references intentionally deserialize and generate typed
diagnostics; bad structural wire shapes fail deserialization instead.

Code-path inspection: `definitions::content_entries` validates names once per
borrowed location snapshot; explanation collection reuses that slice and its
owner lookup, not a `SemanticIndex::build` per candidate. Complete native forms
use borrowed original inlines. Explanation planning retains bounded borrowed
candidates; selected materialization/excerpts copy requested content, and full
JSON necessarily owns/serializes its response. No claim is made that those
explicit output copies are zero-copy. Search rendering owns one output string
plus composed byte ranges and operation-local keys, not an additional IR body.

### Final fixture findings and verification

Producer: `f7df5ca1fe60761a837867ab1db51b24fa2aae57`, 2026-09-08. The final
evidence commit only updates documentation, measurements and generated schema descriptions.
The first final fixture pass exposed two Clang binding failures: recognized
prefixes such as `-D` and `-fno-builtin-` were not allowed to bind immediately
before `<argument>`. `49e28596` adds that boundary, six original minimal cases
and assertions over both existing licensed Clang fixtures. Complete forms and
discovery/ID allocation are unchanged. The audit driver previously misreported
shared semantic failures as malformed profiler responses; `feaaa77f` gives
them an explicit tested category. Neither finding was resolved by accepting a
new golden or rewriting a historical ledger.

Final local results:

- `cargo test --locked --workspace --all-features`: **1,338 passed, six ignored**;
  `cargo test --locked -p mant-engine --examples`: **32 passed**.
- Focused IR, entry coverage/fragment/name/continuation/list-consumer tests,
  `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  `RUSTDOCFLAGS=-Dwarnings` workspace/all-feature rustdoc, read-only engine
  check, and all fuzz binary compilation passed in the repository targets.
- The existing check.sh script syntax commands, all six audit `--self-check`
  commands, audit coverage and native symbol namespace checks passed.
- Rebuilt projection, target and semantic profilers passed the exact existing
  `--fixtures --recheck-recorded --verify --findings-only` gates: **37/37 clean**
  each; projection includes **106 excerpts**; all **14,985** native target
  obligations classified with zero missing/unexpected/collision/invalid/duplicate/
  dangling findings; semantic inventory **10,725**, with zero ordinal, empty,
  invalid-domain or conversion violations.
- Help TLDR and v0.11 schema generation completed. Help has no generated diff;
  the final schema diff only adds backticks to the `u64::MAX` description. The
  intentional structural snapshot migrations belong to S3/S4/S5.
- `bash scripts/build-and-smoke.sh release` passed with strict native warnings;
  the default-feature native-load example was then built separately, preserving
  the same feature configurations as the retained baseline executables.

“Clean” above means those specific audit oracles pass, not that every native
term has a confident semantic kind. The 1,846 aliasless generic terms remain
visible sampling data; producer incompleteness is not silently converted to
full coverage. No historical 45,036-page sweep is claimed as a new run.

Final debug profiler SHA-256 values:

| Profiler | SHA-256 |
| --- | --- |
| projection | `05c87b46e3bb0a6720731c41f3c93df95b8ca9769abf33ce0e269b220c18890e` |
| target | `2afab030c040efb4412e204ab17a8895f689831d5eace57506846ea985be9947` |
| semantic | `063a96cdd911ec7eefdc151fdc3743744dc04d4a8c1f2058c630cef9a4c56982` |

Detailed command output is retained in `target/ir-convergence/*-final.log`.
These local artifacts are disposable; this checked-in summary, source fixtures,
profiler hashes and regression tests are the durable evidence.

### Final performance and retained output comparison

The [raw timing record](ir-convergence-measurements.json) preserves the two
investigation runs and final run, including all samples, exact commands,
toolchain/OS, producer and executable hashes. Final artifacts are under
`target/ir-convergence/s6/`; the earlier `final/` and `final-repeat/` directories
are explicitly **pre-optimization investigations**, not the final producer.
Every run alternates the retained baseline and candidate for seven warmed
rounds, with no concurrent compilation or tests.

The initial two outline medians increased from 175.55 to 193.32 ms and from
175.99 to 194.59 ms, crossing both investigation thresholds. Inspection found
that the semantic index and rendered metadata validated all names again for
empty alias groups. `f7df5ca1` skips only that absent relationship; it does not
skip name validation or nonempty group validation. The four-way regression
matrix in `entry/content.rs` fixes that distinction. The native-load binary is
byte-identical to the pre-optimization binary: this change affects derived
consumers, not native parsing. Cross-run differences alone are not claimed as
isolated speedups; the table compares the paired baseline in the final run.

| Mode | Baseline / final median ms | Baseline range ms | Final range ms | Baseline / final peak KiB |
| --- | --- | --- | --- | --- |
| full JSON process | 222.15 / 256.08 | 212.41–235.29 | 245.84–272.43 | 80,508 / 91,076 |
| outline process | 165.74 / 180.47 | 160.94–173.83 | 172.72–184.04 | 64,228 / 67,148 |
| explain process | 185.13 / 195.49 | 176.51–197.37 | 182.93–202.44 | 64,364 / 67,344 |
| native process (eight loads) | 1,147.16 / 1,208.05 | 1,132.04–1,185.95 | 1,188.40–1,220.22 | 60,292 / 63,188 |

The 49 individual native loads have median **135.76 / 140.18 ms**, ranges
129.00–151.32 / 133.53–155.60 ms. Native process figures include all eight loads
and are not single-parse times. Final outline is +8.9%, explain +5.6%, and
individual native loading +3.3%; these are measurements, not an equivalence
claim or a portable CI threshold.

Full JSON still crosses the thresholds: **+15.3% time and +10.3 MiB peak RSS**.
The new explicit forms/bindings/item provenance and structured layout/list
metadata increase this fixed response from **19,493,365 to 27,414,259 bytes**
(+40.6%). The extra owned response metadata and serialization work are a
documented cost of this contract, consistent with the measured full-output
increase; no allocator-level attribution to individual fields is claimed.
Repeated name validation was investigated and reduced, rather than hiding the
measurements or removing validation. Non-full-output peak increases remain
below 5 MiB. Any future full-output streaming optimization is separate work,
not a deferred correctness requirement of this migration.

Both complete plain text (**1,350,272 bytes**) and Markdown (**1,339,805 bytes**)
match the original baseline byte for byte. All **3,836 owner IDs** match S3
(therefore S4/S5 and final consumer work); the only differences from S0 are the
16 corrected/colliding IDs explicitly recorded for `c2635af4`. Real GCC tests
still verify the nine `--help` children, tail examples and restrictions, distinct
help definitions, and no accidental exhaustive value-domain claim.

Final CLI SHA-256:
`4edacf518bd730c912c190b59b224c6e3cf9a9345f2aa168cb7cf0aeff15ec1b`;
native harness SHA-256:
`871056bf97a3df115ac17e5ec69b23b911769aab3010b2b6dcd104456cbc210f`.

## Local verification boundary

Compile only in repository targets. Run the allowed checks individually;
`scripts/check.sh` includes packaged-crate compilation in a temporary extraction
tree and is therefore not run locally. Do not weaken that CI gate, override
TMPDIR, or claim native Windows/macOS verification on this Linux machine.
The initial implementation did not authorize automatic publication. The later
review follow-up explicitly authorizes pushing `dev` and waiting for CI; it does
not authorize main sync, tags, or release.

## Post-implementation review and CI corrections (2026-09-08)

Review baseline: `fe9315ca`. Final functional producer for this follow-up:
`5c965751`. These changes close the reported counterexamples without reopening
the completed type/layout/list-kind migrations or changing a published schema.

| Finding | Change and executable evidence |
| --- | --- |
| Linux packaged-crate compilation | `9408e269`: move the frozen JSON fixture into `mant-ir/src/entry`, within the declared package include set; five `entry::wire` decoder tests pass, and `cargo package --list --allow-dirty -p mant-ir` includes the asset. Full isolated package compilation remains CI-only. |
| macOS pager signal restoration | `478d751e`: the pager lacked termination handlers and emitted alternate-screen entry before enabling raw mode. Reuse deferred signal registrations, restore only after setup is complete, and keep stdout locked through signal termination. `terminal_display` tests both early setup and active raw mode for SIGINT/SIGTERM, retaining exact termios and screen checks. Linux passed; native macOS verification is pending the new CI run. |
| IR-R01 / A04, A05, A12 | `82283e8d`, `149f879a`, `5c965751`: carry grammar-selected visible ranges into binding mapping; derive native names and evidence from one recognition result and retain parsed Markdown occurrences in the signature. `entry_name_boundaries` checks brace/angle/optional arguments, curly quotes, leading/whitespace-only styling, split sign/suffix runs, projected binding spelling, Name/direct-entry evidence and selected descriptions. Existing styled-argument, literal, export and invalid-IR tests remain active. Marker admission stays exact rather than widening to prefixes. |
| IR-R02 / A07, A12 | `1c5ba2ac`: hanging conversion inherits its head source; shared man/mdoc head merging transfers the first source with the terms. `entry_owner_sources` checks two/three-head TP/TQ and It, PP/RS, non-merged controls, and actual explain/search source spans. A list-lowering unit test preserves unknown first-head sources without fabricating ranges. |
| IR-R03 / A14, A15 | `2002526d`: replace old normative protocol examples in place. `self_manuals::protocol_owner_examples_are_decodable_valid_ir_not_parallel_test_copies` reads both real JSON examples from the manual, decodes blocks/document envelopes, validates IR, round-trips, and rejects mixed legacy fields. Extraction accepts LF and CRLF checkouts. |

Local verification used only the repository `target/` and `fuzz/target/`:

- Workspace/all-features: **1,344 passed, 6 ignored**; HTTP fixture tests ran
  with local-loopback permission. Strict workspace/all-targets/all-features
  Clippy and formatting passed.
- Engine example tests: **32 passed**. Rebuilt the three profilers; all **37**
  fixture pages passed projection, target and semantic verification without
  ledger writes. Projection checked 106 excerpts; all 14,985 target owners were
  classified, with no target differences. Semantic count remains 10,725,
  including 1,846 aliasless generic terms, and zero reported violations.
  Generic terms are not a claim of complete semantic discovery.
- Six audit self-checks, audit coverage, native symbol namespace, installer/
  maintenance script syntax, read-only engine feature check, strict rustdoc,
  and fuzz compilation passed. No new 45,036-page sweep is claimed.
- `scripts/check.sh` and its temporary-tree packaged-crate compilation were
  deliberately not run locally; the unchanged full gate is assigned to CI.

Local logs are under `target/ir-convergence/final-*.log`; these are supporting
artifacts, not substitutes for the exact pushed commit's native CI results.

Release/default build and `scripts/build-and-smoke.sh release` passed. A fresh
seven-round alternating comparison with the retained S6 executables is recorded
in [review measurements](ir-convergence-review-measurements.json), including
all samples, hashes, argv and reference samples. No tests or compilation ran
during these measurements. This compares the review fixes with S6, **not** with
the original S0; the previously disclosed S0 JSON expansion/cost still applies.

| Mode | S6 median ms | Review median ms | S6 peak KiB | Review peak KiB |
| --- | ---: | ---: | ---: | ---: |
| Full JSON | 252.25 | 249.06 | 91,160 | 91,148 |
| Outline | 185.36 | 185.72 | 67,256 | 67,248 |
| Explain | 204.54 | 204.20 | 67,272 | 67,140 |
| Native process (eight loads) | 1,194.62 | 1,211.04 | 63,220 | 63,192 |

These small timing differences are observations, not a performance guarantee.
GCC full text and Markdown remain byte-identical to S0, and all 3,836 owner IDs
match S6. The 37-page semantic census also matches the pre-review producer.

The local Vim gzip with SHA-256
`f3732161727a85a2ebba77acee97a62d6bd613c09fc741dbd676e1bb5b906609`
was checked using `review-mant --input /usr/share/man/man1/vim.1.gz --explain=-w
--format json --display direct`: both `-w{number}` (source line 393) and
`-w {scriptout}` (396) now produce direct Name evidence. This local real-page
check supplements, rather than replaces, the redistributable source-input
regressions in `entry_name_boundaries`.
