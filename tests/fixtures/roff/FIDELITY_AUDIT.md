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
current selection, and structures still absent after selection. In addition
to atomic macros and node attributes, it records the combinations of flags,
fonts, list/display modes, table properties, and their node or parent context.
The sampler gives these interaction shapes extra weight so a corpus cannot look
complete merely because each constituent feature appeared somewhere. The cache is
keyed by corpus, relative path, and decompressed-source hash, and records the
profiler feature-schema identity. A package upgrade is profiled again while
unchanged pages are reused; changing the profiler's observed AST shapes
invalidates and rebuilds the old cache. AST coverage guides
which pages deserve comparison; only a human-reviewed finding plus a focused
fixture and Rust assertion becomes a regression contract.

When adding another distribution or release tree, avoid spending the review
budget on byte-identical copies already represented by a completed corpus:

```sh
python3 scripts/audit-roff-fidelity.py --manpath /tmp/other/share/man \
  --max-pages 200 --syntax-priority --dedupe-across-corpora \
  --syntax-cache /tmp/other-roff-syntax.json.gz \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv \
  --corpus other-release-amd64 --findings-only
```

Reuse requires the same decompressed SHA-256, topic, and exact section. Pages
that mention `.so` or `.mso` remain corpus-local because their meaning can
depend on neighbouring files. The syntax report records each reused page and
its prior corpus; the CSV retains the original evidence instead of claiming a
second renderer run that did not occur.

When a syntax family is rare or a known lowering policy must be rechecked over
every occurrence, select it from the decompressed source before sampling:

```sh
python3 scripts/audit-roff-fidelity.py --manpath /usr/share/man \
  --source-pattern '^[.]Dd' --recheck-recorded \
  --audit-db tests/fixtures/roff/FIDELITY_AUDIT.csv \
  --corpus archlinux-host --findings-only
```

Repeated `--source-pattern` expressions are ANDed and use multiline regular
expression semantics. Unreadable paths are reported explicitly instead of
being counted as selected coverage. Source selection complements AST coverage:
AST sampling finds rare shapes, while a complete source-directed sweep can
catch a wrong lowering branch in a common, already-covered macro.

The automated comparison has multiple detector families. Token presence and
token continuity remain tolerant of renderer layout; source-conditioned
probes additionally compare formatter-owned whitespace or punctuation where
those characters are semantic, such as the `.Nd` separator and synopsis
function terminators. A general punctuation skeleton is intentionally not a
contract because tables, wrapping, and renderer typography would make it a
moving allowlist.
