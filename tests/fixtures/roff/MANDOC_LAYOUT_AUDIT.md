# Mandoc renderer-layout audit

`MANDOC_LAYOUT_AUDIT.csv` applies the same source-gated layout contract as
`LAYOUT_AUDIT.csv` to the exact comparable rows in the mandoc content ledger.
It is independent evidence: it never rewrites content, structure, projection,
or groff-layout conclusions.

Rows add `reference_kind=mandoc` and the same stable `reference_id` used by
`MANDOC_FIDELITY_AUDIT.csv`. Only content rows whose mandoc comparison
completed as `clean` or `review` are eligible; the nine renderer skips have no
two-output layout baseline.

Run a full aligned replay with:

```sh
python3 scripts/audit-roff-layout.py \
  --manpath /path/to/exact/manual-root \
  --corpus corpus-name --replay-fidelity-records \
  --fidelity-db tests/fixtures/roff/MANDOC_FIDELITY_AUDIT.csv \
  --reference-kind mandoc --reference mandoc \
  --reference-id mandoc-1.14.6-1 \
  --audit-db tests/fixtures/roff/MANDOC_LAYOUT_AUDIT.csv \
  --findings-only
```

Use `--recheck-review-recorded` to revisit only current renderer-layout
candidates. The 2026-08-24 replay covers all 17,802 comparable identities:
17,729 clean and 73 reviewed, with no hard failure or pending disposition.

All 73 candidates were inspected against their source. One checked-in rclone
fixture contains an ambiguous sentence repeated in filled and no-fill
contexts. The other 72 are one class: mandoc preserves or adds two to four
vertical blank rows at a source-gated no-fill boundary, while ManT deliberately
normalizes the run to one semantic separator. Every visible line and relative
indent remains present. This is established ManT layout policy, covered by
`collapses_a_no_fill_blank_line_run_to_one_visual_separator`, not an AST or
content regression.

The new rows comprise two NetBSD aliases of one `ipfilter` source and
DragonFly `screen(4)`. The temporary OpenBSD current sample found the same
class once in `isakmpd.conf(5)`; all 36 self-contained ports manuals were
clean. Rolling results remain local evidence rather than permanent ledger
rows.

Keep this route local and release-time. Daily CI validates its ledger schema,
source-identity coverage, review completion, and dependency-free self-check;
it does not install mandoc or reconstruct third-party corpora.
