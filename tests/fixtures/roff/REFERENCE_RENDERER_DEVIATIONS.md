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

The initial batch migrates ten reviewed rows already present in the content
fidelity ledger. It is intentionally conservative: historical reviews did not
retain third-party raw terminal output or a formatter version in the repository,
so they remain `historical-reviewed`, not permanent universal regressions.
When an exact source and renderer are replayed, add the command/tool version to
`note`, promote the row to `reproduced`, and add a focused Rust regression if
the ManT-side invariant belongs in CI.

The first fully reproduced entries come from the above-parity survey attached
to issue #26 and the equation findings in issue #27:

- For the exact decompressed `XDrawArc.3` source hash recorded in the CSV, GNU
  groff 1.24.1 with `-Tutf8 -mandoc -t -e` renders several table-cell fraction
  operands as bare `_` or `__` bars. ManT retains semantic intervals such as
  `[0, π / 2]` and `[π / 2, 3 π / 2]`.
- For the recorded `mm2gv.1` source, the same groff pipeline flattens the matrix
  expression `M sup T` to `MT`. ManT renders `M ^ T`, retaining the exponent
  relation.

These rows assert preservation of operands and mathematical relations, not
equivalence of terminal equation geometry.

Two project-authored issue-#36 fixtures make additional above-parity behavior
fully replayable with GNU groff 1.24.1:

- `.TP` followed by two `.TQ` requests produces three stacked terms in the
  terminal reference. ManT retains the same visible spellings and additionally
  exposes them as aliases of one semantic option identity, so any spelling can
  select the shared description without guessing ownership.
- A directly recursive user macro makes groff stop at its input-stack limit
  with exit status 4 after emitting more than one million blank lines and no
  following section. ManT bounds expansion, emits a diagnostic, and retains the
  finite tail of the document.

These are deliberately different claims: the first is extra semantic structure
over the same visible content; the second is bounded recovery from hostile or
broken input. Neither is generalized beyond the recorded fixture and toolchain.

## Above-parity survey reconciliation

Issue #26 also carried a complete survey of the ways ManT can preserve more
source semantics than groff's fixed-width terminal presentation. The table
below records the whole survey while keeping the CSV's source-identity rule
intact.

| Survey claim | Durable audit treatment |
| --- | --- |
| No line-wrap hyphenation | Source-specific token-continuity evidence remains in rows such as `url-continuity-zipgrep`; a generic width-dependent formatter behavior is not promoted to a universal CSV row. |
| URL and link targets remain complete | Exact historical examples are recorded by `url-continuity-zipgrep` and related fidelity-ledger dispositions. The issue's `lirc(4)`, `seccomp(2)`, and `column(1)` observations were not given new rows because their exact source/toolchain identities were not retained. |
| Equation content survives terminal limitations | `equation-table-fraction-xdrawarc` and `equation-table-superscript-mm2gv` are exact-hash `reproduced` rows. |
| No terminal page furniture or width-truncated cells | This is a deliberate output-boundary property rather than a source-specific formatter deviation, so it belongs to the renderer contract and focused tests rather than this CSV. |
| Outline nodes, stable IDs, semantic entries, and typed links survive | These are ManT product capabilities absent from a flat terminal rendering, not competing presentations of one source construct; architecture and protocol tests own them. |

This distinction prevents a true observation such as “ManT keeps URLs whole”
from becoming an unbounded claim that every groff version, width, and device
corrupts every URL. A survey observation becomes a CSV statistic only when its
source and reference layer can be replayed.

## Systematic survey snapshot

The earlier systematic survey in issue #26 covered more than terminal
rendering. On its 11,505-page corpus it reported the following independent
surfaces; these figures are retained as a dated review snapshot, not silently
merged into the source-hash CSV:

| Surface | Survey result | Durable owner now |
| --- | --- | --- |
| Published outline paths and stable IDs | 800 sampled pages; no failed path, ID, or non-empty-content round trip | Protocol/addressing regressions |
| Semantic entry names | 1,058 resolutions with no failure; 64 ambiguous aliases all refused explicitly | Entry-selection regressions |
| Malformed-input robustness | 93 adversarial cases; no panic, signal, or crash | Parser limits, fuzz targets, and focused hostile-input tests |
| Underlying roff requests | Content parity on 19 of 20 probed behaviors; `.ll 20` kept a long token whole where groff inserted a line-break hyphen | Fidelity audit plus token-continuity deviation rows |
| File inclusion | ManT refused more ambient filesystem authority than groff safe mode | `IncludePolicy` contract and containment tests |
| IR to CommonMark | The survey found one missing and three forged headings | The CommonMark topology oracle added after issues #26 and #28 |

The original IR-to-CommonMark findings are historical defects, not current
advantages. They remain in the snapshot because they explain why reference
rendering, structural self-checks, and parser robustness are separate oracle
families rather than one aggregate “parity” score.

### File-inclusion safety comparison

This comparison is operational security evidence, not a terminal-fidelity CSV
entry. The absolute `.so` row was reproduced on 2026-08-20 using a harmless
temporary target with GNU groff 1.24.1 and current ManT. The other rows retain
the groff 1.23.0 survey result until replayed independently.

| Request | groff `-mandoc` safe mode | ManT | Evidence state |
| --- | --- | --- | --- |
| `.so /absolute/path` | Includes the target | Refuses it, preserves a visible “See the file …” notice, and reports two diagnostics | Reproduced |
| `.nx /absolute/path` | Includes the target | Refuses it with an insecure-request diagnostic | Historical reviewed |
| `.cf /absolute/path`, `.trf /absolute/path` | Refuses | Refuses with an insecure-request diagnostic | Historical reviewed |
| `.sy`, `.pi`, `.pso` | Refuses without unsafe mode | Refuses | Historical reviewed |
| `.open`, `.write` | Refuses | Refuses | Historical reviewed |

The safety conclusion is deliberately narrow: for these observed requests,
ManT grants no more ambient file or process authority than the reference and is
strictly narrower for absolute `.so` and `.nx`. The parser's documented
`IncludePolicy::Root` remains the normative containment contract.

Operational robustness is a separate comparison surface. For example, the
bounded `.while` expansion introduced after issue #29 terminates hostile input
with a diagnostic where the observed groff invocation keeps running. That
advantage is locked by the vendored-parser regression and documented in the
`libmandoc-rs` patch inventory; it is not counted in this renderer-deviation
ledger because no competing rendered document exists.

## Relationship to the other audit records

| Record | Answers |
| --- | --- |
| [`FIDELITY_AUDIT.csv`](FIDELITY_AUDIT.csv) | Did a local source/reference comparison produce a candidate, and what was its human disposition? |
| `STRUCTURE_AUDIT.csv` | Does AST-to-IR lowering preserve structure that token comparison intentionally ignores? |
| `REFERENCE_RENDERER_DEVIATIONS.csv` | Which reviewed source-specific cases establish that the reference terminal rendering is the less semantic presentation? |

No row here suppresses a future candidate automatically. An upgraded source,
different formatter, or changed terminal device must be reviewed again.
