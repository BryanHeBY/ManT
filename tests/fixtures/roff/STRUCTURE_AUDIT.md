# Roff structure audit ledger

`STRUCTURE_AUDIT.csv` is the incremental, human-reviewed ledger for the
development-only AST-to-IR structure audit. It complements, rather than
replaces, the content differential ledger.

The audit parses one local roff page through two ManT-owned paths: the owned
libmandoc AST supplies source-level structural obligations, and the normal
source-aware native-manual path supplies `mant-ir`. It reports candidates when
lowering appears to lose no-fill line boundaries, list/definition items, table
rows or spans, relative indentation, explicit breaks, or typed links. It never
uses terminal line wrapping or an installed `man(1)` program as the oracle.

| Field | Meaning |
| --- | --- |
| `corpus`, `path`, `section`, `source_sha256` | Same exact decompressed-source identity used by `FIDELITY_AUDIT.csv` |
| `profile_schema` | Structural checks that produced the row; changing it requires a deliberate recheck |
| `scan_status` | Latest automatic result: `clean`, `review`, or `hard-failure` |
| `review_status` | Human disposition: `not-required`, `pending`, `false-positive`, `confirmed-open`, or `confirmed-fixed` |
| `note` | Reason for the human conclusion and any focused regression |

`clean` means only that the current coarse AST obligations were satisfied. It
does not certify terminal geometry, typography, or every possible semantic
mapping. A `review` row is useful precisely because it may identify an
unanticipated topology loss; inspect source, AST/IR JSON, and both ManT text
renderers before deciding whether to add a Rust regression.

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
ordinary CI.
