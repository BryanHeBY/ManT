# Roff structure audit ledger

This checkout retains only reproducible fixture records. The complete historical
ledger and review narrative are preserved in [mant-roff-corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/main/audits/mant-tracked/tests/fixtures/roff/STRUCTURE_AUDIT.md).
See [corpus ownership and audit paths](README.md) before running a distribution scan.

`STRUCTURE_AUDIT.csv` is the incremental, human-reviewed ledger for the
development-only AST-to-IR structure audit. It complements, rather than
replaces, the content differential ledger.

The audit parses one local roff page through two ManT-owned paths: the owned
libmandoc AST supplies source-level structural obligations, and the normal
source-aware native-manual path supplies `mant-ir`. It reports candidates when
lowering appears to lose no-fill line boundaries, paragraph/list/definition
containers, table rows/cells/spans, relative indentation, or typed links. It
never uses terminal line wrapping or an installed `man(1)` program as the
oracle.

| Field | Meaning |
| --- | --- |
| `corpus`, `path`, `section`, `source_sha256` | Same exact decompressed-source identity used by `FIDELITY_AUDIT.csv` |
| `profile_schema` | Structural checks that produced the row. A stale version triggers a new scan for the same source identity. |
| `scan_status` | Latest automatic result: `clean`, `review`, or `hard-failure` |
| `review_status` | Human disposition: `not-required`, `pending`, `false-positive`, `confirmed-open`, or `confirmed-fixed` |
| `note` | Reason for the human conclusion and any focused regression |

`clean` means the current source-addressable obligations were satisfied. It
does not certify terminal geometry, typography, or every possible semantic
mapping. A `review` row is useful precisely because it may identify an
unanticipated topology loss; inspect source, AST/IR JSON, and both ManT text
renderers before deciding whether to add a Rust regression.

## What v5 verifies

The profiler retains compact topology signatures in the JSON report. An mdoc
`Bl` is matched to the IR container at the same source line and must retain its
generic-versus-definition kind and direct item count. Native tbl(7) rows and
mdoc `Bl -column` rows are compared in document order; they must retain every
parser-reported cell and its column/row spans. The source-aware lowering path
may recover additional cells when pinned libmandoc retains fewer cells than
the tbl format declares, so enrichment is accepted while cell loss is not.
tbl vertical continuations must lower to an empty continuation cell. Table
wrappers may be transparently merged by the lowering path, so the audit
deliberately checks row topology rather than requiring a particular
intermediate parent block. These checks catch a container being flattened or
given the wrong semantic kind even when its global count happens to match.

The corresponding man(7) `.TP`/`.IP` totals remain telemetry rather than a
strict equality test. A run of hanging tags can legally be folded into one
definition with several aliases, and libmandoc does not preserve enough shared
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
the profiler reports a candidate only if a non-empty `RS` scope produces neither an
indented block nor a semantic list/definition container whose renderer owns
the indentation. Paragraph-boundary and raw `.br` counts remain report
telemetry rather than failure conditions: empty or stateful requests can
legitimately have no independent output block.

Native link obligations come from the structural node that owns the
operands: one for each `Xr` or `MR`, the first address of `UR`, `MT`, `Lk`, or
`%U`, and one for each address operand of `Mt`. A `%R` field with an authored
RFC number also contributes its generated URL. The audit visits the link
block or element once and preserves multiplicity when macro expansion shares
a source coordinate or repeats a destination. Marker-list and plain-list
`It` heads skipped by CVS remain parser diagnostics and source origins;
their child links have no execution obligation. Visible definition terms,
column cells, and list bodies retain theirs.

The audit checks the link class and counts each proven destination separately.
A decoded single-mailbox `mailto:` address from `UR`, `Lk`, or `%U` counts as
an email. A proven empty address does not require a link. A valid address
with an empty label still requires one. Where recovery or a decoding limit
prevents an exact address observation, an unclassified `UR`, `Lk`, or `%U`
still contributes to the combined external and email count. An explicit
email macro continues to require an email link. Manual counts cover usable
typed addresses. An `Xr` with an empty or whitespace-only name and no section
has no address obligation. If the name still produces a visible label such as
`(3)`, the audit requires the retained invalid target and its structured
`ir.invalid-manual-reference` diagnostic. Space and escape recovery affect
the known or unclassified status of the name and optional section.

The audit records every executed `Sx` occurrence. An empty or control-only
label still executes, but contributes no link or unresolved diagnostic
obligation. Authored fixed spaces contribute label glyphs. When a label has
only recovered glyphs and the parser reported an error on that operand, the
profiler records it under `sectionLabelsForReview`. The audit reports the
source coordinates and reason in `source_review` and marks the ledger row
`review`. The separate `source_review_occurrences` count excludes definite
structural violations. Independently visible labels on the same source line
keep their own obligations. A followed alias leaves this review with the
actual target's source identity.

The section lookup check also considers surviving authored destinations on
paragraphs, terms and inline content. A duplicate spelling must stay
unresolved. For ordinal-shaped lists, an unproved BODY effect is reported at
the source list occurrence in `source_review`. The list owner and its item
count still have to survive.
Section references use recursively collected authored words and the validated native heading
target. The audit pairs native headings with emitted section owners at matching
source coordinates, retaining occurrence order through macro expansion. With
complete heading-owner evidence the observed resolved links must contain the
expected canonical targets in order, and a distinct
`unresolved-section-reference` diagnostic must name each unresolvable label.
Formatting can split a link into more than one piece. Incomplete heading-owner
evidence limits the audit to the aggregate visible-label obligation count for
that page. The consumer regression separately checks the fixed CVS HTML href
and heading ID pairs so the shared authored spelling cannot hide a wrong target.

`sourceLinkOrigins` keeps the existing coordinate sets for `Xr`, `UR`/`Lk`,
`MT`/`Mt`, and `Sx`. The obligation count also covers direct `MR` and reference
fields. A fallback macro that expands `MR` into styled `IR` operands, as in
`addftinfo(1)`, can yield extra manual links through the product's independent
manual-reference recognition. The native count follows the expanded tree.

The `mant.roff-structure-profile/v16` successful page response requires
`ordinalCasesForReview`, `literalDisplaysForReview`, and
`sourceLinkOrigins.sectionLabelsForReview`. Their
arrays keep all source occurrences, and an empty array records conclusive
evidence. Incomplete observations fail validation. A pure alias retains its
own redirect identity.

List, equation and table topology records are validated on both sides. Native
coordinates, item counts, row indexes and spans require nonboolean integers.
Known enum values and table-cell flags retain the distinct AST and IR shapes.
The source ordinal observer retains overwrite evidence across node boundaries,
including styled pieces and source continuations, and records an uncertain
owner with its original coordinates instead of inventing a definite shape.
Glyphless display rows and nested lists or columns can establish useful BODY
structure. A stroke that consumes zero-advance state preserves the potential
ordinal spelling in the source audit, with unresolved placement recorded for
Review.
Short and long displays use the same source payload check. A proved glyph
prefix still requires a preformatted owner when a later edit leaves the
exact spelling uncertain. A rewind can revoke that proof, and an empty
initial pass can reject later glyphs. A glyphless completed row can use
vertical space. A separate display equation supplies its own visible owner.
Short display witnesses include delimiter words executed beside the native
BODY, using the same spacing rules as its content. A known prefix of a
table cell keeps its exact row obligations when a later source range needs
review.
Display source evidence follows the active native field across formatter
words and their actual separators. An unproved edit keeps the owning
display's source coordinates for Review. A nested column's cells or a
definition's terms supply their own content. Literal text in an item's
description still supplies an obligation.
Each display inherits the spacing mode at its source position. A tbl span
clears that mode before later words. Each display inherits only the font
evidence available when it enters. A local or inherited font can leave
whole-prefix replacement for Review. A later font request in a separate
owner does not affect a finished display.
Generated terminal words enter the same source field as authored words.
The forced bold dash of `Fl` has three known device cells, while a manual
reference can generate parentheses for two glyphless arguments.

Each proven display needs a sourced literal block in its own section or
item, with the table, row and field also identified for an mdoc column list.
Native child positions identify possible block origins. For source rows of
known spelling, the audit consumes the matching glyph range in execution
order. A column can merge several displays into one block, so separate
ranges can prove separate occurrences. The field witness spans adjacent inline macros, Unicode text, generated
delimiters and link words. Within each cell, ordinary rows on both sides
reserve their own positions even when they spell the display identically.
Only preformatted rows can discharge a display obligation. A cell whose
source ranges cannot be reconstructed is recorded for source Review. An
empty observed cell can still prove a display missing. Outside cells, a
distinct sourced block can independently witness the native display.

Repeated zero-advance effects and normalized custom enclosures retain their
source execution order. BODY evidence follows the native selected equation
rows and skips alignment counts. A populated glyphless equation is an authored
object with its own source and empty reading value, independently of a
configuration-only equation root and of ordinal source uncertainty.

Equation topology is source-addressed by line and context. Native
`Node::equation_inline` mode determines inline/display ownership independently
of physical input-line position. Older snapshots use their original line-start
fallback. An inline equation stays in its complete paragraph or literal owner,
and display equations retain their independent blocks. Normalized values and
placement are checked together. Empty configuration equations contribute only
to the parser-state count.

Only executed native equation nodes establish mathematical obligations. CVS
`roff_parseln` keeps tbl words opaque while the table parser owns their input.
Those cell words keep their literal delimiters through the normal text path.
The profiler compares bounded executed snapshots directly. Its reading-value
normalization handles native special characters and fonts in one sequential
pass, preserving escaped backslashes and consuming nonprinting breakpoints.

This distinction is intentional. The ledger should surface credible lowering
loss, not prescribe byte-for-byte formatter emulation or turn normal
libmandoc normalisation into a review queue.

A valid pure `.so` alias is recorded as covered but does not inherit the
target page's AST obligations. Its source identity owns only the redirect;
the resolved target has its own fidelity/structure identity. Missing targets
remain hard failures, so this rule does not hide incomplete manual trees.

The literal display check reserves ordinary rows within the same
mdoc column cell using the spacing mode at each field's first source
node. Leading source spaces close filled rows. Explicit spacing, the
automatic gap before a noncompact display and buffered invisible cells
on no-fill lines contribute physical row slots. For bounded changes, the audit
considers every alignment with the fewest inserted or deleted rows. A
replacement can describe a missing and an inserted row, including when its
neighbors also moved. A display row requires a literal observation and
ordinary rows compete for identical observations. The audit reports the
possible ranges of loss for the cell and for each display. A single deletion
has a linear check that covers long cells. Beyond the alignment budget,
unresolved display candidates remain in review. The audit compares literal
row multiplicity with verified display rows to bound each owner's loss
and the whole cell's loss independently. The same bounds apply to a
partial cell whose known prefix diverges. An unclassified tail stays in
source review when its verified prefix matches. Bounded edits with a
complete cell still report exact alignment ranges. Each
`literalDisplaysForReview` record names its source coordinates and its
one-based occurrence at those coordinates. A display inside a column cell
also reports the enclosing column table's coordinates and page-wide
occurrence with the zero-based row and column. Occurrences count tables
across parent scopes. Review records may skip occurrences and may be in
any order. A repeated display coordinate and occurrence is invalid.

Build the profiler and scan the reproducible real fixtures:

```sh
cargo build -p mant-engine --example roff_structure_profile
python3 -m scripts.roff.audit.audit_roff_structure --fixtures --json /tmp/mant-structure.json
```

The ManT-authored `real/mant-audit/equation-contexts.7` fixture makes every
equation placement class observable on this reproducible run. For a complete
host sweep, select every `.EQ` page; combine `.TS` with exact eqn operator
tokens for the table-focused intersection:

```sh
python3 -m scripts.roff.audit.audit_roff_structure --manpath /usr/share/man \
  --corpus current-host-eqn --source-pattern '^\.EQ(?:\s|$)' --findings-only
python3 -m scripts.roff.audit.audit_roff_structure --manpath /usr/share/man \
  --corpus current-host-table-eqn --source-pattern '^\.TS(?:\s|$)' \
  --source-pattern '(^|[^A-Za-z])(sub|sup|over|ldots)([^A-Za-z]|$)' \
  --findings-only
```

For a local corpus whose previous content-audit rows should be replayed under
the new structural checks, use the same corpus identity and manual root:

```sh
python3 -m scripts.roff.audit.audit_roff_structure --manpath /usr/share/man \
  --corpus archlinux-host --replay-fidelity-records --findings-only
```

`--recorded-only` rechecks unchanged structure-ledger rows; omit both replay
options to scan new or changed pages. A newer profiler schema also makes an
otherwise unchanged row eligible for a normal recheck, so expanded checks never
silently inherit old coverage. The scan state is replaced for that source
identity, while authored review dispositions and notes are carried forward.
`--source-pattern` is the preferred way
to run complete high-risk sweeps for `.nf`/`.fi`, `.EX`/`.EE`, `.TP`/`.IP`,
`.Bl`, `.TS`/`.TE`, or mdoc display families. The profiler is a local batch
tool, not a public CLI or MCP interface, and its host-only results never gate
ordinary CI. Daily CI only runs the script's dependency-free `--self-check`
plus focused Rust regressions for findings that were manually confirmed.
