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

Not yet completed: S2–S6. Current successful baseline verification is recorded
in `remaining-review-verification.md`; it is not evidence for later changes.

## Local verification boundary

Compile only in repository targets. Run the allowed checks individually;
`scripts/check.sh` includes packaged-crate compilation in a temporary extraction
tree and is therefore not run locally. Do not weaken that CI gate, override
TMPDIR, or claim native Windows/macOS verification on this Linux machine.
No automatic push, main sync, tags, or release is part of this work.
