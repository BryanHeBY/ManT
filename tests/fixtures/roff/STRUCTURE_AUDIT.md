# Roff structure audit ledger

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

## What v2 verifies

The profiler retains compact topology signatures in the JSON report. An mdoc
`Bl` is matched to the IR container at the same source line and must retain its
generic-versus-definition kind and direct item count. Native tbl(7) rows and
mdoc `Bl -column` rows are compared in document order; they must retain cells
and column/row spans. tbl vertical continuations must lower to an empty
continuation cell. Table wrappers may be transparently merged by the lowering
path, so the audit deliberately checks row topology rather than requiring a
particular intermediate parent block. These checks catch a container being
flattened or given the wrong semantic kind even when its global count happens
to match.

The corresponding man(7) `.TP`/`.IP` totals remain telemetry rather than a
strict equality test. A run of hanging tags can legally be folded into one
definition with several aliases, and libmandoc does not preserve enough shared
source identity to distinguish that normalisation from a container boundary in
the generic IR. Real mdoc definition lists *are* source-addressable and remain
strictly checked.

No-fill source text that begins an input line is a hard-line-boundary
obligation; standalone roff font-switch lines are intentionally excluded
because they have no printable glyph. Relative `RS` nesting is deliberately
weaker: roff distances are not terminal columns, so v2 reports a candidate
only if a document with an `RS` scope produces no indented IR block at all.
Paragraph-boundary and raw
`.br` counts remain report telemetry rather than failure conditions: empty or
stateful requests can legitimately have no independent output block. Typed
manual, external, email, and section links are checked independently, so a
surviving link cannot mask degradation to a different target class.

This distinction is intentional. The ledger should surface credible lowering
loss, not prescribe byte-for-byte formatter emulation or turn normal
libmandoc normalisation into a review queue.

Build the profiler and scan the reproducible real fixtures:

```sh
cargo build -p mant-engine --example roff_structure_profile
python3 scripts/audit-roff-structure.py --fixtures --json /tmp/mant-structure.json
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
