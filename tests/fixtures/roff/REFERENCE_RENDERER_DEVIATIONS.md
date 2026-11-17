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

Third-party pages inherit their exact source identity from the fidelity ledger.
Small project-authored `real/mant-audit` fixtures may instead establish that
identity directly when the raw source, SHA-256, reference command, and focused
ManT regression all live in this repository. This exception keeps a narrowly
reproducible renderer limitation from requiring a synthetic fidelity-corpus
row.

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

## Mandoc-specific reproduced evidence

The aligned 2026-08-24 replay also records nine source-hash-specific cases for
`mandoc-1.14.6-1 -T utf8 -O width=200`. Each row inherits an exact identity
from `MANDOC_FIDELITY_AUDIT.csv`, where the source, native mandoc output, ManT
text, and structured result received a durable human conclusion:

- a wide `tbl` in `svcerr_systemerr(3)` retains source-ordered logical cells
  in ManT while the terminal reference flattens adjacent cells into continuity
  phrases;
- a source-requested continuation in `bash(1)` remains one complete `PATH`
  value in ManT but is split into terminal tokens by the recorded renderer and
  width;
- the `mm2gv(1)` matrix keeps the transpose relation as `M ^ T`, whereas
  mandoc exposes the eqn source spelling `$M sup T$`;
- the `groff_char(7)` `u006E_0303` example produces the documented `ñ` in
  ManT while mandoc leaves the demonstrated output empty;
- the authored `.St -p1003.1-2024` citation in FreeBSD `ps(1)` remains between
  “described by” and “and what” instead of disappearing from the sentence; and
- an explicit `.Ns` in `bhyve(8)` retains the executable spelling `pptN`
  instead of changing it to `ppt N`;
- NetBSD `drm(4)` retains the source-authored name `Jaromír Doleček` while the
  renderer drops the unsupported `\\[vc]` caron;
- NetBSD `ite(4)` honors an explicit `.Ns` and renders the authored plural
  `ites` rather than `ite s`; and
- DragonFly `gdb(1)` retains continued long tags such as `--symbols=file` and
  `--exec=file`, groups each with its short alias, and makes every spelling
  addressable; mandoc 1.14.6 drops the long tags from terminal output.

The coverage checker validates every row for the current mandoc renderer
against the exact content-ledger identity, section, reviewed result, and
renderer command. Most rows are source-confirmed `false-positive` comparisons.
The `gdb(1)` row is instead backed by a `confirmed-fixed` conclusion: the
one-way missing-content detector is clean after the parser fix, while the
manual source, reference output, exact fixture, and focused alias assertion
prove the above-parity behavior. Older immutable renderer observations may
remain when the current mandoc identity advances.

The same NetBSD source identities also have independent GNU groff 1.24.1
evidence in the CSV. Direct `-Tutf8 -mandoc -t -e` replay drops the `\\[vc]`
caron and inserts the same unwanted `.Ns` space. Separate rows are retained
because agreement between two reference renderers is evidence from two
toolchains, not one universal formatter claim.

The other mandoc review dispositions were intentionally not promoted. Most
are equivalent wrapping or detector accounting, repeated examples of one
already represented class, or the deliberate no-fill policy in which ManT
normalizes a blank run to one semantic separator. Those remain per-row audit
notes rather than evidence that one renderer is categorically more accurate.

## Corpus ownership

This checkout retains only conclusions about checked-in fixtures. The full
historical survey and original ledger are archived in
[mant-roff-corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/main/audits/mant-tracked/tests/fixtures/roff/REFERENCE_RENDERER_DEVIATIONS.md).
Distribution conclusions and render artifacts belong there; apply the same
source-bound admission rules before recording them. See [the fixture guide](README.md).
