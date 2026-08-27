# Roff structure audit ledger

`STRUCTURE_AUDIT.csv` is the incremental, human-reviewed ledger for the
development-only AST-to-IR structure audit. It complements, rather than
replaces, the content differential ledger.

The audit parses one local roff page through two ManT-owned paths: the owned
`mantdoc` AST supplies source-level structural obligations, and the normal
source-aware native-manual path supplies `mant-ir`. It reports candidates when
lowering appears to lose no-fill line boundaries, paragraph/list/definition
containers, table rows/cells/spans, relative indentation, or typed links. It
never uses terminal line wrapping or an installed `man(1)` program as the
oracle.

| Field | Meaning |
| --- | --- |
| `corpus`, `path`, `section`, `source_sha256` | Same exact decompressed-source identity used by `FIDELITY_AUDIT.csv` |
| `profile_schema` | Structural checks that produced the row; changing it requires a deliberate recheck |
| `scan_status` | Latest automatic result: `clean`, `review`, or `hard-failure` |
| `review_status` | Human disposition: `not-required`, `pending`, `false-positive`, `confirmed-open`, or `confirmed-fixed` |
| `note` | Reason for the human conclusion and any focused regression |

`clean` means the current source-addressable obligations were satisfied. It
does not certify terminal geometry, typography, or every possible semantic
mapping. A `review` row is useful precisely because it may identify an
unanticipated topology loss; inspect source, AST/IR JSON, and both ManT text
renderers before deciding whether to add a Rust regression.

## What v4 verifies

The profiler retains compact topology signatures in the JSON report. An mdoc
`Bl` is matched to the IR container at the same source line and must retain its
generic-versus-definition kind and direct item count. Native tbl(7) rows and
mdoc `Bl -column` rows are compared in document order; they must retain every
parser-reported cell and its column/row spans. The source-aware lowering path
may recover additional cells when the native AST retains fewer cells than
the tbl format declares, so enrichment is accepted while cell loss is not.
tbl vertical continuations must lower to an empty continuation cell. Table
wrappers may be transparently merged by the lowering path, so the audit
deliberately checks row topology rather than requiring a particular
intermediate parent block. These checks catch a container being flattened or
given the wrong semantic kind even when its global count happens to match.

The corresponding man(7) `.TP`/`.IP` totals remain telemetry rather than a
strict equality test. A run of hanging tags can legally be folded into one
definition with several aliases, and the parser does not preserve enough shared
source identity to distinguish that normalisation from a container boundary in
the generic IR. Real mdoc definition lists *are* source-addressable and remain
strictly checked.

No-fill source text that begins an input line is a hard-line-boundary
obligation; bounded runs of POD-style zero-width `\&` rows count as one blank
row, while trailing guards and standalone sequences of roff font switches do
not claim printable content. Literal display wrappers may be transparently
coalesced, so their minimum-presence check is made against retained
preformatted rows rather than the number of IR containers. Relative `RS`
nesting is deliberately weaker: roff distances are not terminal columns, so
v4 reports a candidate only if a non-empty `RS` scope produces neither an
indented block nor a semantic list/definition container whose renderer owns
the indentation. Paragraph-boundary and raw `.br` counts remain report
telemetry rather than failure conditions: empty or stateful requests can
legitimately have no
independent output block. Typed manual, external, email, and section links are
checked independently by unique printable source occurrence, so duplicated
AST views and empty closing macros cannot manufacture an obligation while a
surviving link still cannot mask degradation to a different target class. An
unresolvable `.Sx` is allowed to degrade to visible text only when the normal
lowering path also emits its structured
`unresolved-section-reference` diagnostic; the audit therefore detects silent
loss without pretending that a cross-document or absent section is a valid
same-document target.

JSON reports retain the exact source coordinates for each link macro family
under `sourceLinkOrigins`; this makes a count candidate manually auditable
without turning local paths or the verbose topology into a product protocol.

Equation topology is source-addressed by line and context. A non-empty
line-start `.EQ` node must remain one display equation block; an inline
delimited equation must remain an inline code-valued expression without
splitting its paragraph; and an equation carried by a tbl cell must remain in
that cell. The normalized value must match as well as the placement. An empty
configuration-only `.EQ delim … .EN` is counted as parser state and must not
invent a display block. For table-cell delimiter expressions that the native parser
keeps as opaque text, the profiler reads the bounded source, finds expressions
using the active delimiter pair, and normalizes each fragment through the same
pinned eqn parser. It never implements a second equation grammar merely to
guess their meaning. Missing optional host decompressors only omit this extra
table check for `.xz`/`.bz2` inputs; plain, gzip, and zstd inputs are
self-contained.

This distinction is intentional. The ledger should surface credible lowering
loss, not prescribe byte-for-byte formatter emulation or turn normal
parser normalisation into a review queue.

A valid pure `.so` alias is recorded as covered but does not inherit the
target page's AST obligations. Its source identity owns only the redirect;
the resolved target has its own fidelity/structure identity. Missing targets
remain hard failures, so this rule does not hide incomplete manual trees.

Build the profiler and scan the reproducible real fixtures:

```sh
cargo build -p mant-engine --example roff_structure_profile
python3 scripts/audit-roff-structure.py --fixtures --json /tmp/mant-structure.json
```

The ManT-authored `real/mant-audit/equation-contexts.7` fixture makes every
equation placement class observable on this reproducible run. For a complete
host sweep, select every `.EQ` page; combine `.TS` with exact eqn operator
tokens for the table-focused intersection:

```sh
python3 scripts/audit-roff-structure.py --manpath /usr/share/man \
  --corpus current-host-eqn --source-pattern '^\.EQ(?:\s|$)' --findings-only
python3 scripts/audit-roff-structure.py --manpath /usr/share/man \
  --corpus current-host-table-eqn --source-pattern '^\.TS(?:\s|$)' \
  --source-pattern '(^|[^A-Za-z])(sub|sup|over|ldots)([^A-Za-z]|$)' \
  --findings-only
```

For a local corpus whose previous content-audit rows should be replayed under
the new structural checks, use the same corpus identity and manual root:

```sh
python3 scripts/audit-roff-structure.py --manpath /usr/share/man \
  --corpus archlinux-host --replay-fidelity-records --findings-only
```

`--recorded-only` rechecks unchanged structure-ledger rows; omit both replay
options to scan new or changed pages. A newer profiler schema also makes an
otherwise unchanged row eligible for a normal recheck, so expanded checks never
silently inherit old coverage. `--source-pattern` is the preferred way
to run complete high-risk sweeps for `.nf`/`.fi`, `.EX`/`.EE`, `.TP`/`.IP`,
`.Bl`, `.TS`/`.TE`, or mdoc display families. The profiler is a local batch
tool, not a public CLI or MCP interface, and its host-only results never gate
ordinary CI. Daily CI only runs the script's dependency-free `--self-check`
plus focused Rust regressions for findings that were manually confirmed.

## 2026-08-24 BSD closure result

The exact NetBSD 11.0 replay was 2,357/2,357 clean. The DragonFly BSD 6.4.2
replay was 6,359 clean and four reviewed out of 6,363 pages. Three candidates
were mdoc subsection titles containing an `Xr` macro: the title is one string
identity in the renderer-neutral IR while links in document content remain
typed. The fourth was `atc(6)`, whose literal display intentionally contains
the characters `.Bl` and `.It` rather than a semantic list. All four are
source-confirmed false positives, with no hard failure or pending review.

The independent rolling checks described in `FIDELITY_AUDIT.md` added 200/200
clean OpenBSD current pages and 36/36 clean self-contained ports manuals to
the temporary evidence. Released NetBSD and DragonFly identities are retained
in this ledger; rolling identities are promoted only when a stable release or
focused fixture makes them durable.
