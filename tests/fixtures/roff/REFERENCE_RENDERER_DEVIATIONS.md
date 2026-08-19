# Reference-renderer deviation ledger

`REFERENCE_RENDERER_DEVIATIONS.csv` records the narrow cases in which a
human review concluded that ManT preserves source semantics more usefully than
the terminal output used as the differential-audit reference.

This is deliberately **not** a list of general “groff bugs”. The audit invokes
the host `man(1)` command, whose formatter, device, macro package, terminal
width, and even implementation vary by corpus. Linux entries commonly involve
GNU man/groff; BSD entries may instead involve mandoc. The ledger therefore
names the observed reference layer and bounds every conclusion to an exact
source hash and review scope.

## Admission rule

Add a row only after a reviewer has inspected all of the following:

1. The original roff source and its macro-level intent.
2. ManT's structured IR plus a human-oriented rendering.
3. The reference terminal rendering, including the formatter/device context.
4. A reason the difference harms source semantics, such as merging a
   definition term into its body, exposing a device artifact, or collapsing an
   equation operator.

Wrapping, typography, headers, alternative historical standard names, and a
mere preference for ManT link decoration do **not** qualify. Those remain
ordinary `false-positive` review dispositions in
[`FIDELITY_AUDIT.csv`](FIDELITY_AUDIT.csv).

## CSV fields

| Field | Meaning |
| --- | --- |
| `id` | Stable category plus representative source identifier |
| `category` | Narrow affected semantic property, not a renderer-wide verdict |
| `review_state` | `historical-reviewed` for a migrated, manually checked legacy result; `reproduced` once the exact toolchain and commands have been captured |
| `corpus`, `path`, `section`, `source_sha256` | Reproducible source identity inherited from the fidelity ledger |
| `reference_renderer` | The observed rendering layer; never assume it is groff on every platform |
| `mant_advantage`, `reference_limitation` | The competing behavior stated in comparable semantic terms |
| `scope` | Which user-visible contract the conclusion covers |
| `note` | Evidence limits and source-specific qualification |

The initial batch migrates ten reviewed rows already present in the content
fidelity ledger. It is intentionally conservative: historical reviews did not
retain third-party raw terminal output or a formatter version in the repository,
so they remain `historical-reviewed`, not permanent universal regressions.
When an exact source and renderer are replayed, add the command/tool version to
`note`, promote the row to `reproduced`, and add a focused Rust regression if
the ManT-side invariant belongs in CI.

## Relationship to the other audit records

| Record | Answers |
| --- | --- |
| [`FIDELITY_AUDIT.csv`](FIDELITY_AUDIT.csv) | Did a local source/reference comparison produce a candidate, and what was its human disposition? |
| `STRUCTURE_AUDIT.csv` | Does AST-to-IR lowering preserve structure that token comparison intentionally ignores? |
| `REFERENCE_RENDERER_DEVIATIONS.csv` | Which reviewed source-specific cases establish that the reference terminal rendering is the less semantic presentation? |

No row here suppresses a future candidate automatically. An upgraded source,
different formatter, or changed terminal device must be reviewed again.
