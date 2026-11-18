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
v4 reports a candidate only if a non-empty `RS` scope produces neither an
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

Each executed `Sx` shares the parser's authored title spelling. The audit
pairs native headings with emitted section owners at matching source
coordinates, retaining occurrence order for sections from the same macro
expansion. With complete heading-owner evidence, the observed resolved links
must contain the expected canonical targets in order, and a distinct
`unresolved-section-reference` diagnostic must name each unresolvable
authored reference. Additional observations can occur where formatting splits
a link into more than one piece. Incomplete heading-owner evidence limits
the audit to the aggregate executed-reference count for that page.

`sourceLinkOrigins` keeps the existing coordinate sets for `Xr`, `UR`/`Lk`,
`MT`/`Mt`, and `Sx`. The obligation count also covers direct `MR` and reference
fields. A fallback macro that expands `MR` into styled `IR` operands, as in
`addftinfo(1)`, can yield extra manual links through the product's independent
manual-reference recognition. The native count follows the expanded tree.

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
silently inherit old coverage. `--source-pattern` is the preferred way
to run complete high-risk sweeps for `.nf`/`.fi`, `.EX`/`.EE`, `.TP`/`.IP`,
`.Bl`, `.TS`/`.TE`, or mdoc display families. The profiler is a local batch
tool, not a public CLI or MCP interface, and its host-only results never gate
ordinary CI. Daily CI only runs the script's dependency-free `--self-check`
plus focused Rust regressions for findings that were manually confirmed.
