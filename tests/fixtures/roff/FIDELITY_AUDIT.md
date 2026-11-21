# Roff fidelity audit ledger

This checkout retains only reproducible fixture records. The complete historical
ledger and review narrative are preserved in [mant-roff-corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/main/audits/mant-tracked/tests/fixtures/roff/FIDELITY_AUDIT.md).
See [corpus ownership and audit paths](README.md) before running a distribution scan.

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

Before a recorded observation is reused, the audit validates the currently
stored manual with the file loader's compression contract. A failed read
appears in the current report and leaves the recorded observation untouched.
Restoring the valid member permits reuse of the original decoded identity.

The advanced `--pages-file FILE` input accepts a newline-delimited set of
absolute manual paths below the selected `--fixtures` or `--manpath` roots. It
bypasses sampling and ledger selection while retaining normal hierarchy and
alias handling. It is primarily the narrow interface used by the separate
renderer-layout audit; it does not by itself change the ledger unless an
explicit `--audit-db` is also supplied.

`--layout-signals` adds reference-layout evidence to JSON without changing a
page's content-fidelity `scan_status`. It aligns only unique whole-line anchors
and records each renderer's blank-line runs and indentation levels. Candidates
are limited to an indentation collapse relative to the local body baseline,
spacing changes between adjacent source no-fill lines, and exact merges of two
short source no-fill lines. This keeps ordinary device-width wrapping and
formatter-owned margins out of the review queue.

`false-positive` is not a generic allowlist. It means the roff source,
reference output, and ManT output were compared and the signal was attributed
to presentation differences such as wrapping, table traversal, generated
headers, or reference-renderer token concatenation. `confirmed-fixed` is
reserved for a real ManT defect with a focused Rust regression.

The small, stricter subset where review establishes that ManT preserves the
source semantics more usefully than the observed terminal reference is recorded
separately in [`REFERENCE_RENDERER_DEVIATIONS.csv`](REFERENCE_RENDERER_DEVIATIONS.csv)
and its [admission guide](REFERENCE_RENDERER_DEVIATIONS.md). Do not promote an
ordinary presentation difference to that ledger without source, IR, and
renderer evidence.
