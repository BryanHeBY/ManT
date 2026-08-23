# Roff audit coverage contract

ManT's roff audits answer different questions, but their execution ranges have
one explicit relationship. `FIDELITY_AUDIT.csv` is the historical breadth
index keyed by immutable `(corpus, path, decompressed-source SHA-256)` identity:

- `STRUCTURE_AUDIT.csv` must cover every fidelity identity with the current
  AST-to-IR profile schema, including pages whose external renderer comparison
  could not complete;
- `PROJECTION_AUDIT.csv` must cover the same complete fidelity set with the
  current IR-to-CommonMark profile schema;
- `LAYOUT_AUDIT.csv` must cover fidelity rows whose comparison completed as
  `clean` or `review`, because `skipped` and `hard-failure` rows have no valid
  two-renderer layout baseline; and
- every checked-in real fixture must appear under the current structure and
  projection schemas. These bounded fixture runs are the reproducible CI
  baseline; local distribution corpora remain development and release-time
  evidence;
- `MANDOC_FIDELITY_AUDIT.csv` must contain every historical fidelity identity
  plus every checked-in fixture under one explicit mandoc renderer identity;
  and
- `MANDOC_LAYOUT_AUDIT.csv` must cover every comparable mandoc-fidelity
  identity and every checked-in fixture under the same renderer identity.

The original structure, projection, and groff-layout ledgers may contain a
deliberate superset. Source-pattern sweeps, host-only equation probes, complete
release scans, and renderer-specific layout studies do not need to be copied
into unrelated ledgers merely to make row counts equal. The aligned mandoc
ledgers are narrower by design: content is exactly historical fidelity plus
fixtures, and layout is exactly the comparable mandoc content set.
`REFERENCE_RENDERER_DEVIATIONS.csv` is a curated conclusion index, not a
coverage route. Even so, rows naming the current mandoc renderer are validated
against the matching mandoc-fidelity source hash, section, renderer command,
and completed human disposition so the curated index cannot silently drift
from its detailed evidence. The accepted source conclusions are either a
reviewed `false-positive` comparison or `confirmed-fixed` evidence with a
focused regression; unresolved and merely clean/unreviewed rows are rejected.

Check the set relationship without invoking a renderer or reparsing the local
distribution corpora:

```sh
python3 scripts/check-roff-audit-coverage.py
```

When the check reports a missing corpus, replay that corpus from the same roots
and corpus name used for fidelity:

```sh
python3 scripts/audit-roff-structure.py --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
python3 scripts/audit-roff-projection.py --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
python3 scripts/audit-roff-layout.py --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
```

The coverage check is cheap enough for daily CI: it validates CSV headers,
status/schema values, duplicate and exact source identities, current schema
coverage, matching mandoc renderer identities, pending-review totals, and the
small checked-in fixture inventory. It also validates the schema and unique IDs
of the curated deviation ledger and reports how many rows reproduce the current
mandoc renderer.
It does not run groff, scan host manuals, or turn local third-party corpora
into a CI dependency. A zero missing count certifies execution-range alignment,
and a zero pending count certify execution-range alignment and completion of
the recorded review queue. They do not certify that a reference renderer is
semantically authoritative; the per-row disposition still records that human
judgment.
