# Roff fidelity audit ledger

`FIDELITY_AUDIT.csv` is the incremental, human-reviewed ledger for local
differential scans. It prevents unchanged host manuals from being investigated
again while keeping host-specific output out of ordinary CI.

Each row identifies one exact decompressed source:

| Field | Meaning |
| --- | --- |
| `corpus` | Stable name for the installed manual collection |
| `path` | Source path relative to the selected root, or to the common parent of multiple roots |
| `section` | Exact manual suffix such as `1`, `3bsd`, or `7ssl` |
| `source_sha256` | SHA-256 of the decompressed roff bytes |
| `scan_status` | Latest automated result: `clean`, `review`, `hard-failure`, or `skipped` |
| `review_status` | Human disposition: `not-required`, `pending`, `false-positive`, `confirmed-open`, or `confirmed-fixed` |
| `note` | Reason for the human disposition |

The content hash is part of the identity. An upgraded manual is scanned as a
new row instead of inheriting the conclusion for older bytes. Automated
rechecks update `scan_status` but preserve a human disposition; a clean
recheck therefore does not erase the evidence that a page once exposed a real
regression.

`false-positive` is not a generic allowlist. It means the roff source,
reference output, and ManT output were compared and the signal was attributed
to presentation differences such as wrapping, table traversal, generated
headers, or reference-renderer token concatenation. `confirmed-fixed` is
reserved for a real ManT defect with a focused Rust regression.

The initial ledger covers the host Arch Linux manual tree and the independently
installed Miniconda manual trees. A `skipped` row is historical, incomplete
coverage rather than an accepted disposition. Normal incremental runs select
such a row again even when its content hash is unchanged.

Pages containing `.so` requests are rendered through the indexed-manual query
path with an exact derived `MANT_MANPATH`. Localized hierarchies remain isolated
from their default-language neighbours, redirect targets stay under the same
approved root, and aliases exercise the product resolver rather than a second
implementation in the audit script. A renderer failure or an output without
comparable visible tokens is a `hard-failure`, never a silent skip.

Migrate only historical skips after upgrading the audit logic with:

```sh
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man \
  --retry-skipped \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv \
  --corpus archlinux-host
```

After improving either renderer or the comparison heuristic, re-run only rows
still awaiting disposition with `--pending-only`. A clean recheck clears an
automated `pending` state; human `false-positive`, `confirmed-open`, and
`confirmed-fixed` decisions remain durable.

Re-run all recorded content, including historical skips, with:

```sh
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man \
  --recorded-only \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv \
  --corpus archlinux-host
```

Omit `--recorded-only` to add new or changed pages. Use a new `corpus` name
when the provenance or root collection changes. The checked-in compressed
fixtures and their package/license records remain the reproducible CI oracle;
this host ledger is discovery evidence.

For syntax-directed expansion, first build the batch profiler and combine the
ledger with a compressed local profile cache:

```sh
cargo build --package libmandoc-rs --example roff_ast_profile
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man \
  --max-pages-per-section 25 --syntax-priority \
  --syntax-cache /tmp/mant-roff-syntax.json.gz \
  --syntax-report /tmp/mant-roff-syntax-report.json \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv \
  --corpus archlinux-host --findings-only
```

The profiler reads the real libmandoc AST in batches. Its report distinguishes
features already represented in completed ledger rows, features added by the
current selection, and structures still absent after selection. The cache is
keyed by corpus, relative path, and decompressed-source hash, so a package
upgrade is profiled again while unchanged pages are reused. AST coverage guides
which pages deserve comparison; only a human-reviewed finding plus a focused
fixture and Rust assertion becomes a regression contract.
