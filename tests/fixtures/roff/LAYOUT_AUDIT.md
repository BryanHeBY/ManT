# Roff renderer-layout audit ledger

`LAYOUT_AUDIT.csv` is an independent, opt-in ledger for local comparisons
between a `man(1)`-compatible reference renderer (normally GNU groff on Linux)
and ManT's semantic `--format man` text. It does not replace the content
fidelity or AST-to-IR structure ledgers, and it never invalidates or requires a
re-run of their completed rows.

The audit uses the shared `audit-roff-fidelity.py` rendering path, so aliases,
manual-root resolution, terminal cleanup, and the controlled `MANWIDTH=200`
reference environment have one implementation. It then derives a deliberately
narrow layout signal:

- relative indentation collapse at a unique whole-line anchor, after removing
  each renderer's own page-wide body margin;
- blank-line spacing changes between two adjacent source lines inside a
  recognised no-fill or literal-display region;
- two short no-fill reference lines that ManT renders as one line.

It records blank-run counts, indentation levels, and aligned anchor counts in
its JSON report for human review. It does **not** compare ordinary paragraph
wrapping, absolute terminal columns, headers, page furniture, or general table
geometry: those vary with the formatter, device, macro package, and ManT's
intentional copy-friendly text presentation.

| Field | Meaning |
| --- | --- |
| `corpus`, `path`, `section`, `source_sha256` | Exact decompressed manual identity, independent of the other ledgers |
| `layout_schema` | Renderer-layout probe version that produced the row |
| `scan_status` | Latest automatic result: `clean`, `review`, or `hard-failure` |
| `review_status` | Human disposition: `not-required`, `pending`, `false-positive`, `confirmed-open`, or `confirmed-fixed` |
| `note` | Human conclusion, reference environment, and any focused regression |

Run it only for a newly added corpus or an explicitly chosen sweep; ordinary CI
does not call it. A normal run is incremental against this ledger alone:

```sh
cargo build -p mant
python3 scripts/audit-roff-layout.py --manpath /tmp/new-release/share/man \
  --corpus new-release-amd64 --max-pages-per-section 20 \
  --json /tmp/mant-layout.json --findings-only
```

Use `--recheck-recorded` only when changing this layout probe or deliberately
revisiting a renderer result. Keep reviewed third-party output in a local
scratch directory; commit the compact ledger conclusion and a focused licensed
fixture only after confirming a real ManT defect.
