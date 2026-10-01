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
- `MANDOC_FIDELITY_AUDIT.csv` must contain every historical fidelity identity
  under one explicit historical mandoc renderer identity;
- `MANDOC_LAYOUT_AUDIT.csv` must cover every comparable mandoc-fidelity
  identity under that same historical renderer identity; and
- every checked-in fixture must have paired mandoc content and layout
  evidence, either in those historical ledgers or in the separate
  `MANDOC_CVS_FIXTURE_FIDELITY_AUDIT.csv` and
  `MANDOC_CVS_FIXTURE_LAYOUT_AUDIT.csv` supplements.

The original structure, projection, and groff-layout ledgers may contain a
deliberate superset. Source-pattern sweeps, host-only equation probes, complete
release scans, and renderer-specific layout studies do not need to be copied
into unrelated ledgers merely to make row counts equal. The aligned mandoc
ledgers are narrower by design: content is exactly historical fidelity plus
fixtures already compared with that renderer, and layout is exactly the
comparable historical mandoc content set. Adding a fixture does not authorize
claiming that an unavailable historical renderer ran on its bytes, changing
the old renderer identity, or discarding the fixture from the inventory.
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

## Current CVS fixture supplements

The paired CVS supplements record actual comparisons for newly admitted
checkout fixtures without rewriting distribution evidence from a different
mandoc release. They use the existing mandoc CSV headers, including explicit
`reference_kind` and `reference_id`; the layout supplement uses the current
layout schema. Each supplement must contain exactly the same source identities
and sections. Every source must match the decompressed SHA-256 and section of
an actual checked-in fixture under corpus `fixtures`. Only completed `clean`
or `review` comparisons qualify, and unresolved `pending` reviews still fail
the gate. A failed or skipped comparison cannot supply missing fixture coverage.

The renderer must be the single active pristine CVS registration in
`crates/libmandoc-rs/upstream/oracle/registry.json`. Coverage verifies the
registered attestation hash, UTF-8 authorization, and its current tracked
SOURCE, FILES, CVS inventory and build-recipe hashes, including the pristine
archive identity recorded in SOURCE. This inexpensive static
check does not require a local reference executable or claim another render.
The actual fidelity/layout audit commands must still pass complete oracle
preflight, including the binary and pristine archive, before writing rows.

Supplement identities are merged only for the two checked-in-fixture coverage
checks. They cannot fill holes in the historical distribution baseline. A
source copied from a distribution remains inadmissible until it is a licensed,
identified checkout fixture; renaming its corpus without matching actual bytes
does not grant coverage. Existing historical rows and their renderer identities
retain their original meaning. When the active oracle changes, replay the
bounded supplement against the new registered renderer before replacing its
identity; historical distribution ledgers are not relabeled as new evidence.

The coverage CLI accepts `--cvs-fixture-fidelity-db` and
`--cvs-fixture-layout-db` for explicit ledger paths. Their defaults are the two
tracked supplements above. `--self-check` tests missing pairs, stale source
hashes, renderer mismatches, distribution rows, section/schema errors,
incomplete comparisons, pending reviews, and mutation of oracle trust records.

`REFERENCE_RENDERER_DEVIATIONS.csv` is a curated conclusion index, not a
coverage route. Even so, rows naming either the historical ledger's mandoc
renderer or the active CVS supplement renderer are validated against their
respective mandoc-fidelity source hash, section, renderer command,
and completed human disposition so the curated index cannot silently drift
from its detailed evidence. The accepted source conclusions are either a
reviewed `false-positive` comparison or `confirmed-fixed` evidence with a
focused regression; unresolved and merely clean/unreviewed rows are rejected.

Check the set relationship without invoking a renderer or reparsing the local
distribution corpora:

```sh
python3 -m scripts.roff.audit.check_roff_audit_coverage
```

When the check reports a missing corpus, replay that corpus from the same roots
and corpus name used for fidelity:

```sh
python3 -m scripts.roff.audit.audit_roff_structure --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
python3 -m scripts.roff.audit.audit_roff_projection --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
python3 -m scripts.roff.audit.audit_roff_layout --manpath /path/to/man-root \
  --corpus corpus-name --replay-fidelity-records --findings-only
```

The coverage check is cheap enough for daily CI: it validates CSV headers,
status/schema values, duplicate and exact source identities, current schema
coverage (including target and semantic fixtures), matching mandoc renderer
identities, pending-review totals, and the small checked-in fixture inventory.
It also validates the schema and unique IDs of the curated deviation ledger and
reports how many rows reproduce the current mandoc renderer.
It does not run groff, scan host manuals, or turn local third-party corpora
into a CI dependency. A zero missing count certifies execution-range alignment,
and a zero pending count certify execution-range alignment and completion of
the recorded review queue. They do not certify that a reference renderer is
semantically authoritative; the per-row disposition still records that human
judgment.
