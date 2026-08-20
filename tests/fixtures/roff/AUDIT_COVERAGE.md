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
  evidence.

Each ledger may contain a deliberate superset. Source-pattern sweeps, host-only
equation probes, complete release scans, and renderer-specific layout studies
do not need to be copied into unrelated ledgers merely to make row counts
equal. `REFERENCE_RENDERER_DEVIATIONS.csv` is a curated conclusion index, not a
coverage route.

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
coverage, pending-review totals, and the small checked-in fixture inventory.
It does not run groff, scan host manuals, or turn local third-party corpora
into a CI dependency. A zero missing count certifies execution-range alignment,
not that every newly surfaced `review` candidate has received a human
disposition; that separate queue is printed explicitly and retained in its
route ledger.
