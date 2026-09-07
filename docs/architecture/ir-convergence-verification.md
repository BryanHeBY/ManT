# IR convergence implementation and evidence

This work implements the updated 2026-09-07 convergence guide. It revises the
unreleased v0.11 contracts, not any published protocol. Stages are committed
independently; this document distinguishes planned contracts from completed work.

## S0: baseline and frozen contract

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
`tests/fixtures/ir/convergence-wire.json`. The `new` cases are the target contract,
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
nearest explanation owner, excerpts, search outside flattened tables, nested
choices and source-order links.

Additional observed gap to resolve during consumer convergence: table cells are
flattened into fenced Markdown text without entry byte ranges. Search therefore
attributes cell content to the section, although outline/excerpt/explain now
reach the actual nested owner. Do not call A12 complete until this is addressed.

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

Not yet completed: S6. Current successful baseline verification is recorded
in `remaining-review-verification.md`; it is not evidence for later changes.

## Local verification boundary

Compile only in repository targets. Run the allowed checks individually;
`scripts/check.sh` includes packaged-crate compilation in a temporary extraction
tree and is therefore not run locally. Do not weaken that CI gate, override
TMPDIR, or claim native Windows/macOS verification on this Linux machine.
No automatic push, main sync, tags, or release is part of this work.
