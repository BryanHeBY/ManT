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

- authored relative indentation collapse at a unique whole-line anchor inside
  a no-fill or literal-display region; a renderer's implicit display gutter is
  explicitly out of scope;
- blank-line spacing changes between two adjacent source lines when the source
  itself contains a blank line or `.sp` request inside a recognised no-fill or
  literal-display region;
- two short no-fill reference lines that ManT renders as one line, unless the
  same text is also attributable to a flowed or single-line source occurrence;
  this avoids treating formatter wrapping or repeated text as a lost boundary.

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

To add layout evidence for exactly the source bytes already comparable in the
content-fidelity ledger, select that corpus explicitly. The content ledger is
only read as an immutable identity index; its historical `skipped` and
`hard-failure` rows are not comparable renderer baselines and are excluded. It
is neither re-rendered nor rewritten:

```sh
python3 scripts/audit-roff-layout.py --manpath /tmp/old-release/share/man \
  --corpus old-release-amd64 --replay-fidelity-records \
  --json /tmp/mant-layout-old-release.json --findings-only
```

Use `--recheck-recorded` only when changing this layout probe or deliberately
revisiting a renderer result. Keep reviewed third-party output in a local
scratch directory; commit the compact ledger conclusion and a focused licensed
fixture only after confirming a real ManT defect.

After a narrowly scoped lowering fix that can only remove a known candidate,
`--recheck-review-recorded` replays just unchanged rows whose latest layout
status is `review`. This keeps a confirmation pass proportional to the actual
finding set rather than rescanning an entire local corpus.

## 2026-08-24 BSD closure result

All 2,357 NetBSD 11.0 pages and all 6,363 DragonFly BSD 6.4.2 pages were clean
under the groff-backed source-gated layout oracle. The temporary OpenBSD
current sample was also 200/200 clean, and all 36 self-contained ports manuals
were clean. These results concern authored no-fill boundaries, relative
indentation, and source-requested spacing only; they do not redefine terminal
wrapping or formatter margins as ManT contracts.
