# Roff regression fixtures and audit contracts

ManT keeps the exact licensed manual pages, reduced inputs, expected values and
source-bound ledger rows needed by its offline regression tests. Broad source
collections, distribution sweeps and historical run narratives live in
[mant-roff-corpus](https://github.com/BryanHeBY/mant-roff-corpus).

The full pre-migration audit records are preserved under
[`audits/mant-tracked/tests/fixtures/roff`](https://github.com/BryanHeBY/mant-roff-corpus/tree/main/audits/mant-tracked/tests/fixtures/roff),
with the source ManT commit and file hashes in the migration snapshot. The
retained CSV rows are the unchanged `corpus=fixtures` subset. The 17 checkout
query-gold cases retain their exact expectations; the complete 199-query panel
is archived alongside the other historical records. This relocation is not a
new correctness run and does not turn old or uncovered evidence into a pass.

## What stays here

- `real/`: selected original manuals with provenance, licenses and focused tests.
  Follow the [source catalogue requirements](real/README.md) when adding a page.
- Small synthetic roff fixtures and permanent Rust/consumer regression matrices.
- The fixture-only audit CSVs, current CVS fixture supplements and coverage rules.
- `ENTRY_QUERY_GOLD.json`: network-free checkout query expectations.
- `LAYOUT_ACCEPTANCE.json` and `LAYOUT_FOLLOWUP_ACCEPTANCE.json`: source-bound
  reduced cases executed by the behavior-matrix checker, not disposable logs.

Do not copy a complete downloaded manual tree, distribution ledger, large
renderer output bundle or stage conversation into this repository. Confirmed
findings should contribute a minimal **complete** licensed real page where
practical, its source metadata, and a permanent assertion. Preserve necessary
include members and ownership context.

## Running a broader audit

The ManT audit tools remain here because they test the current product and its
profile schemas. Configure a corpus checkout for external scans:

```sh
export MANT_ROFF_CORPUS="$HOME/dev/mant-roff-corpus"
python3 -m scripts.roff.audit.audit_roff_structure \
  --manpath /path/to/exact/manual/root --corpus snapshot-id \
  --json "$MANT_ROFF_CORPUS/audits/mant/current/structure.json"
```

For structure, projection, targets, semantics and layout, default ledger paths
follow the selected input:

| Input | Default ledger directory |
| --- | --- |
| `--fixtures`, or no manual root | `tests/fixtures/roff` in this checkout |
| `--manpath`, with `MANT_ROFF_CORPUS` | `$MANT_ROFF_CORPUS/audits/mant/current` |
| `--manpath`, without that variable | `target/roff-audits` (untracked) |

Explicit `--audit-db`, `--fidelity-db` and `--source-ledger` paths take
precedence. Fidelity retains its optional unrecorded scan when no external
corpus checkout is configured; with the variable set, external fidelity scans
also default to that external current-run directory. Fidelity `--no-record`
explicitly disables result-ledger access and is mutually exclusive with `--audit-db`.
The layout collector always uses it for its fidelity subprocess, so a layout
scan cannot change fidelity history through inherited configuration. `--verify` remains
read-only. Environment configuration never redirects fixture CI or rewrites
immutable imported history.

Use explicit archived paths when replaying historical source records; an empty
current-run ledger is not the historical baseline. To replay the complete
archived query panel, supply `--query-gold` and all necessary `--query-root`
overrides. Its old relative paths are evidence of the original run, not a
portable path-resolution contract.

Review every unexplained candidate and representative clean pages. Record
source hashes, input context, producer/reference identities and uncovered
axes. See [coverage](AUDIT_COVERAGE.md), [fidelity](FIDELITY_AUDIT.md),
[structure](STRUCTURE_AUDIT.md), [projection](PROJECTION_AUDIT.md),
[layout](LAYOUT_AUDIT.md), [targets](TARGET_AUDIT.md),
[semantics](SEMANTIC_AUDIT.md) and
[renderer deviations](REFERENCE_RENDERER_DEVIATIONS.md) for each admission rule.
