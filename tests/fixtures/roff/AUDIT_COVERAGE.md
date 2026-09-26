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
- every checked-in real fixture must appear under the current structure,
  projection, target-conservation, and semantic-entry precision schemas. These
  bounded fixture runs are the reproducible CI
  baseline; local distribution corpora remain development and release-time
  evidence;
- the historical `MANDOC_FIDELITY_AUDIT.csv` package cohort must retain every
  historical fidelity identity under `mandoc-1.14.6-1`, with
  `MANDOC_LAYOUT_AUDIT.csv` covering that cohort's comparable content rows;
- the separate pinned-CVS `MANDOC_CVS_FIDELITY_AUDIT.csv` and
  `MANDOC_CVS_LAYOUT_AUDIT.csv` supplement may cover checked-in fixtures not in
  the historical sample, using only the registered
  `cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1` identity. Their source
  identities must be disjoint from the package cohort, and their layout must
  cover the supplement's own comparable content rows; and
- the union of both mandoc cohorts' fidelity and layout identities must cover
  every checked-in fixture. A header-only supplement does not count as a run.

The original structure, projection, and groff-layout ledgers may contain a
deliberate superset. Source-pattern sweeps, host-only equation probes, complete
release scans, and renderer-specific layout studies do not need to be copied
into unrelated ledgers merely to make row counts equal. The aligned mandoc
ledgers are narrower by design: package content is the historical fidelity
sample; pinned-CVS supplement content is checked-in fixtures absent from that
sample. Layout is checked against comparable content **within each cohort**,
not against another renderer's result. A source may not be copied into both
cohorts to make a missing count disappear.
`TARGET_AUDIT.csv` is also independent: its distribution rows record complete
or targeted zero-width destination sweeps rather than replaying the historical
visible-fidelity sample. Only its checked-in fixture coverage, schema, source
identity, and review queue are part of the cross-ledger CI contract.
`SEMANTIC_AUDIT.csv` follows the same independent model for final semantic
entries and retained definition structure. Its broad sweeps need not mirror
the visible-fidelity sample, while every checked-in fixture must have a current
schema row and no unresolved review result.

`ENTRY_QUERY_GOLD.json` adds a different, source-bound contract: selected
explanation queries must return the reviewed independent owners, forms, names,
kinds and body witnesses, and no extra direct owners. Its `checkout` subset runs
without host manuals/network in the normal Unix gate. The wider fixed panel is
local-only; missing/drifted sources and unreviewed queries are unresolved. A
source-lexical signal or a valid binding alone never proves declaration
eligibility. Explicit TP bullet eligibility, styled parameter separators,
prose false-head exclusions and private native-witness lifecycle remain covered
by the synthetic native-declaration regressions; Term counts are not failures.

`REFERENCE_RENDERER_DEVIATIONS.csv` is a curated conclusion index, not a
coverage route. Its nine historical mandoc conclusions remain bound to the
`mandoc-1.14.6-1` package cohort. The checker validates each against that
cohort's matching source hash, section, renderer command, and completed human
disposition; the new CVS rows cannot be used as substitute evidence. A future
CVS deviation needs an independent reviewed route. The accepted package
source conclusions are either a reviewed `false-positive` comparison or
`confirmed-fixed` evidence with a focused regression; unresolved and merely
clean/unreviewed rows are rejected.

The CVS supplement was introduced as two header-only CSVs. Its six
`archlinux/` fixtures (`btrfs-subvolume`, `gzip`, `tmux`, `yay`, `zip`,
`zsh`) now have individually reviewed structure, projection, target,
semantic, CVS content, and CVS layout rows. The six CVS content and six CVS
layout rows are `clean`; the historical package cohorts are unchanged.
Future additions must first run the exact checked-in source identities
through their route-specific profilers and inspect every candidate. For the
mandoc pair, use the registered CVS oracle attestation and exact archive,
select only the new pages for fidelity via `--pages-file`, then replay the
completed CVS fidelity rows into layout with `--replay-fidelity-records` and
the scratch fidelity database. Both runs must pass the same `--reference-id`,
absolute `--reference` binary, `--oracle-attestation`, and `--oracle-archive`;
do not append those results to the historical package ledgers or claim the
CVS oracle is package `mandoc-1.14.6-1`. The registered attestation is
`crates/libmandoc-rs/upstream/oracle/attestations/cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1.json`.

Check the set relationship without invoking a renderer or reparsing the local
distribution corpora:

```sh
python3 scripts/check-roff-audit-coverage.py
```

For an already recorded distribution corpus, replay its historical fidelity
identities from the same roots and corpus name:

```sh
python3 scripts/audit-roff-structure.py --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
python3 scripts/audit-roff-projection.py --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
python3 scripts/audit-roff-layout.py --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
```

That replay does **not** select the six new Arch fixtures: they are absent
from the historical fidelity ledger. For these checked-in pages, run the
structure, projection, target, and semantic profilers with `--fixtures` so
each route finds its own missing identities; inspect candidates before
updating the checked-in ledgers. For the separate mandoc cohort, put the six
absolute fixture paths in `target/entry-en06/six-pages.txt`, then run content
and layout into separate scratch CSVs before reviewing and copying completed
rows to the CVS supplement:

```sh
python3 scripts/audit-roff-fidelity.py \
  --pages-file target/entry-en06/six-pages.txt --corpus fixtures \
  --mant "$PWD/target/debug/mant" \
  --reference "$PWD/target/mandoc-migration/reference/mandoc" \
  --reference-kind mandoc \
  --reference-id cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1 \
  --oracle-attestation crates/libmandoc-rs/upstream/oracle/attestations/cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1.json \
  --oracle-archive target/mandoc-migration/freeze-cvs-20260920T122115Z/upstream-cvs-20260920T122115Z.tar.gz \
  --audit-db target/entry-en06/cvs-fidelity.csv --findings-only
python3 scripts/audit-roff-layout.py --fixtures --corpus fixtures \
  --replay-fidelity-records \
  --fidelity-db target/entry-en06/cvs-fidelity.csv \
  --mant "$PWD/target/debug/mant" \
  --reference "$PWD/target/mandoc-migration/reference/mandoc" \
  --reference-kind mandoc \
  --reference-id cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1 \
  --oracle-attestation crates/libmandoc-rs/upstream/oracle/attestations/cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1.json \
  --oracle-archive target/mandoc-migration/freeze-cvs-20260920T122115Z/upstream-cvs-20260920T122115Z.tar.gz \
  --audit-db target/entry-en06/cvs-layout.csv --findings-only
```

These roff audit scripts call the default ManT renderer, not
`--annotated-preview`. Clean rows establish that route's comparison only;
annotated surface fidelity, source/IR ownership, and semantic query gold
require their own review.

The coverage check is cheap enough for daily CI: it validates CSV headers,
status/schema values, duplicate and exact source identities, current schema
coverage (including target and semantic fixtures), each mandoc cohort's
renderer identity and content/layout relationship, cross-cohort disjointness,
pending-review totals, and the small checked-in fixture inventory.
It also validates the schema and unique IDs of the curated deviation ledger and
reports how many rows reproduce the current mandoc renderer.
It does not run groff, scan host manuals, or turn local third-party corpora
into a CI dependency. Zero missing counts establish execution-range alignment;
zero pending counts establish completion of the recorded review queues. Both
are required before calling the audit coverage complete. They do not certify that a reference renderer is
semantically authoritative; the per-row disposition still records that human
judgment.
