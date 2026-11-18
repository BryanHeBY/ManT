# Roff semantic-entry precision audit

This checkout retains only reproducible fixture records. The complete historical
ledger and review narrative are preserved in [mant-roff-corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/main/audits/mant-tracked/tests/fixtures/roff/SEMANTIC_AUDIT.md).
See [corpus ownership and audit paths](README.md) before running a distribution scan.

`SEMANTIC_AUDIT.csv` records an independent audit of semantic entries created
after native roff lowering. Target conservation proves that zero-width
destinations survive; it cannot prove that numbered prose, placement-only
definitions, or invalid value-domain children were not promoted into the
agent-visible semantic index.

The current semantic profile is `mant.roff-semantic-profile/v6`. An incremental
scan revisits matching corpus, path and source hash records from earlier profiles.
It preserves human review dispositions and notes. Use `--recorded-only` to replay
every unchanged recorded source. Verification requires a current profile row.
An ordinal-shaped list with an unproved BODY effect is recorded with its
source coordinates and reason. The audit still checks that its owner is
present.

## Oracle and scope

The separate [query gold](ENTRY_QUERY_GOLD.json) retains the 17 checkout queries
from the original 199-query panel without changing their expectations. The full
panel is archived in mant-roff-corpus. The checkout subset
records raw source coordinates, complete returned forms, exact names/kinds,
bounded opening-body witnesses and all expected direct owners. Extra direct
owners fail too; supplementary mention evidence is allowed. Empty independent
declarations must stay empty, not borrow the next description. `expectedSupports`
separately checks exact member sources/forms, the final description's head,
middle and tail witnesses, required table/code/link structure and absence of
invented alias relationships. A `declaration-member` response is resolved back
to its physical owner before applying the unchanged original body assertions. These are
selected witnesses, not whole-page golden renderings or a precision/recall
estimate for all entries. Per-query notes distinguish exact common declarations
from accepted conservative classifications and local macro/format syntax.
Source-group, member path, SHA and original sampling reasons remain in the
manifest. Opening-body witnesses are normalized eight-word hashes, not copied
manual prose; inspect the original source and local result when adjudicating a
failure. These hashes guard the reviewed witness, not the entire body. Raw
third-party pages are not redistributed by the manifest.

`audit-roff-semantics.py --query-gold ENTRY_QUERY_GOLD.json` uses the existing
collector via optional profiler probes. The `--fixtures` subset selects only
the checked-in source root and is included in `scripts/check.sh`. For the archived panel, supply its explicit `--query-gold` path and relocate
its declared roots with `--query-root ROOT=/path/to/root`. Native AST matches
are coordinate candidates for a reviewer to inspect, not another classifier or
proof of owner equality. The comparator combines source coordinates with exact
forms, names, kinds and body witnesses; it refuses incomplete/omitted evidence.
Unreviewed or missing-input probes remain unresolved rather than silently clean.
