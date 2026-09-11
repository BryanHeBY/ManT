# Full-dimensional audit and bounded triage

`AUDIT_TRIAGE.json` records a new replay after `RENDERING_AUDIT.json`, without
rewriting that historical evidence or promoting its reviews to acceptance.
The scope is 45,036 logical inputs at 37,286 physical paths, the eight existing
roff dimensions, the separate content/source-geometry census, and the complete
112-case reduced CLI/reader matrix at widths 20, 40, 80 and 120.

This is **completed scanning with pending review**, not whole-corpus acceptance.
The ordinary coverage gate still checks the existing accepted ledgers; its
zero pending-ledger count does not close this new census's review queue.
The full passes use explicitly frozen binaries before the selected product
fixes below. The corrected product has separate real-page, upstream comparison,
matrix and full local-gate evidence; those checks must not be represented as
another 45,036-page run of the final product commit.

## Distinguishing evidence from defects

The audit now preserves raw comparisons alongside source-bound explanations.
An explanation must identify the literal source declaration and exact generated
occurrence. For example, fixed CVS `termp_nd_pre` emits an en dash for `Nd`,
whereas ManT's NAME presentation uses an em dash. Only this proven occurrence
can be explained; arbitrary punctuation or missing content cannot be waived.

Other corrections prevent the oracle from confusing presentation or query
coordinates with source semantics:

- Native nonbreaking indentation occupies terminal columns; ASCII-space-only
  measurements previously invented indentation mismatches.
- Table/equation interiors are not prose line-origin anchors. Unchecked geometry
  stays partial or uncovered, while following prose and real gap changes remain
  checked.
- Native named-bullet evidence and unsigned numeric definition boundaries are
  distinct from arbitrary literal keys or malformed semantic names.
- Invisible headings retain their original IR excerpt coordinates. Heading
  inline links must also count toward observed reference conservation.
- A literal nonpositive relative offset is not evidence of positive indentation;
  unsupported expressions retain conservative obligations, not an exemption.

Mutation tests continue to detect missing, duplicated and reordered content,
operand leakage, changed gaps/origins and relocated targets. Artifacts are
bounded, risk-ranked representatives across categories and corpora, not merely
the first pages encountered. Raw findings and retention omissions remain visible.

## Whole-corpus results

Every logical input is recorded in each of structure, projection, targets,
semantics, mandoc fidelity/layout, and groff fidelity/layout. The JSON retains
their separate execution, status and coverage counts. The 66 logical target
records initially affected by the output-byte cap in a 64-page profiler batch
were replayed one physical
page per batch using the same frozen binaries. All 66 target profiles completed;
their source-context limitations remain, and the original budget rows are kept.

| Legacy dimension | Raw clean | Review | Failure / unavailable / budget |
| --- | ---: | ---: | ---: |
| Structure | 45,017 | 16 | 3 |
| Projection | 45,031 | 2 | 3 |
| Targets, original batch | 44,970 | 0 | 66 |
| Semantics | 44,180 | 28 | 828 |
| Fidelity, CVS mandoc | 44,708 | 241 | 87 |
| Layout, CVS mandoc | 44,946 | 3 | 87 |
| Fidelity, groff | 43,451 | 779 | 806 |
| Layout, groff | 44,219 | 11 | 806 |

Each row sums to 45,036. Raw clean can still have partial source-context
coverage; the JSON retains that independent axis. All 828 semantic execution
errors are the explicit standalone-alias rejection requiring manual discovery.
Three indexed loads cannot resolve missing Apple corpus `.so` targets. The
722 groff fidelity hard failures lack comparable reference tokens on sources
with possible external context. One additional groff error is the intentional
`macro-recursion.7` fixture reaching its input-stack limit. These are not
automatically accepted inputs. The two projection reviews and five structure
reviews have separate, successful corrected-oracle rechecks; original counts
remain unchanged.

The additional census gives these independent content assessments:

| Content assessment | Logical inputs |
| --- | ---: |
| Covered without a content difference | 14,608 |
| Difference explained by strict source evidence | 10,134 |
| Remaining content review | 18,313 |
| Content coverage unavailable | 1,981 |

Raw overall statuses remain 54 clean, 14,350 partial, 28,651 review and 1,981
uncovered. Explained content does not automatically establish complete geometry.
The geometry dimension separately has 119 covered, 25,228 partial, 957 review
and 18,732 uncovered inputs. These dimension counts overlap and must not be
added together as independent defects.

Thus both infrastructure limitations and actual lowering defects existed.
Review volume alone cannot decide which one a page contains. External includes,
non-UTF8 source, decoder budgets, reference recursion limits and failed standalone
loading remain explicit coverage/execution evidence, not automatic product bugs
or automatic waivers. A source with a possible include is not thereby proved
harmless. Historical hashes are checked against the actual decoded inputs;
rolling-host changes do not inherit earlier corpus acceptance.

## Selected confirmed product fixes

| Finding | Upstream execution evidence and correction | Regression |
| --- | --- | --- |
| `vimtutor(1)` loses three example gaps after a control-only paragraph body | CVS `pre_PP`/`print_bvspace` and groff `an*break-paragraph` execute distance before the body. Preserve that executed space, even if only formatter state was lowered, with the paragraph's source span. | `empty_paragraph_spacing.rs`: PP/P/LP, PD 0/1/2, nf/fi/font, skipped/initial paragraphs and nested RS |
| `tipc-media(8)` incorrectly fits `window` and its body on one row | CVS man rendering retains a pending head line when the body executes `br` or another line boundary. Preserve this fact in definition placement; do not add global empty breaks or let TQ merging reopen a closed line. | `definition_body_breaks.rs`: 168 initial-request combinations, delayed-break negatives, fitting, fonts and target retention |

Independent upstream replay checks 27 paragraph and 36 definition combinations
against both the fixed, unpatched CVS build and groff 1.24.1. Real-page diffs are
limited to the three missing gaps and the lost label/body boundary respectively.
The post-fix 112-case reader matrix retains the same raw findings as before:
all 51 generated assertions and 112 basic viewport checks pass, and all nine
mutation classes are detected. Four-width residual content and exact-viewport
candidates are preserved rather than declared equivalent.

The sole highest-priority possible-operand candidate, illumos `eqn(1)`, was
causally checked: deleting or replacing all 32 `.ne 2` requests leaves each
renderer's output byte-identical. Extra standalone `2` tokens come from
linearized equations, not leaked request operands. This does **not** close the
page's equation, grouping or diacritic review.

## Remaining work and reproducibility

Bulk manual review stops here by request. Remaining content, table/equation,
tab-stop, framing, declaration-run and layout candidates are not accepted or
silently suppressed. See the JSON for exact producer commits, binary/rule hashes,
manifest and actual-source inventory hashes, raw result hashes, supplemental
replays and validation logs. Large artifacts remain under the recorded local
`target` paths; hashes establish identity, not remote availability or build
attestation.

Use the full-manifest commands in
[`docs/development.md`](../../../docs/development.md#pinned-reference-content-and-geometry-audit).
The whole-corpus replay uses a 64-page profiler batch and 16 workers; the target
budget supplement uses batch size 1. The content/geometry census uses four
workers. Both reference renderers use deterministic UTF-8 and a 200-column
reference width; the reduced reader matrix independently exercises narrow views.

Real terminal-cell checks cover the reduced matrix, not all corpus pages or
Windows/macOS native interaction. This run does not repeat the historical
old/new canonical-IR migration comparison or native sanitizer stress. Per-child
limits are not an aggregate parent-process wall-time or memory guarantee.
