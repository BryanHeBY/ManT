# Mandoc reference fidelity audit

`MANDOC_FIDELITY_AUDIT.csv` is the independent content ledger for comparing
ManT with a native mandoc renderer. It deliberately reuses the same immutable
`(corpus, path, decompressed-source SHA-256)` targets as
`FIDELITY_AUDIT.csv`, so differences between reference renderers are not
confused with a different sample.

This is a supplementary oracle, not a replacement for groff. ManT and native
mandoc share a parser family, so agreement cannot prove parser correctness;
the route is valuable for formatter-generated mdoc punctuation, source-device
conditionals, UTF-8 rendering, platform behavior, and differences that the
independent groff comparison can then contextualize.

The first two columns bind every row to one renderer installation:

| Field | Meaning |
| --- | --- |
| `reference_kind` | Always `mandoc` for this ledger |
| `reference_id` | Stable package/renderer identity; currently `mandoc-1.14.6-1` |
| remaining fields | Same exact identity, scan, review, and note contract as `FIDELITY_AUDIT.csv` |

The current identity names the installed Arch `mandoc-noconflict 1.14.6-1`
package; changing renderer bytes or packaging requires a new identity and a
fresh independent ledger rather than silently inheriting these conclusions.

Mandoc receives decompressed source bytes on stdin and renders UTF-8 at width
200. That avoids filesystem compression support becoming part of the result.
A redirect-only `.so` page is resolved within its selected manual hierarchy;
an embedded `.so`/`.mso` request is recorded as `skipped` because expanding it
would require a second, renderer-specific include implementation. Standard
output alone is compared, and the renderer identity is mandatory whenever an
audit database is written.

## Exact replay

Use the historical groff ledger as a read-only source index:

```sh
cargo build -p mant
python3 scripts/audit-roff-fidelity.py \
  --manpath /path/to/exact/manual-root \
  --corpus corpus-name --replay-source-records \
  --source-ledger tests/fixtures/roff/FIDELITY_AUDIT.csv \
  --reference-kind mandoc --reference mandoc \
  --reference-id mandoc-1.14.6-1 \
  --audit-db tests/fixtures/roff/MANDOC_FIDELITY_AUDIT.csv \
  --findings-only
```

Replay refuses a missing path or decompressed hash mismatch. Multiple package
roots must be supplied in the same order-independent set used by the original
corpus so their relative labels remain stable. Normal incremental, pending,
and recorded rechecks retain the same semantics as the groff ledger.

The 2026-08-23/24 replay reconstructed all 17,776 historical release-corpus
identities plus all 35 checked-in fixtures. Distribution archives, package
versions, and hashes are recorded in
[`FIDELITY_AUDIT.md`](FIDELITY_AUDIT.md). In addition:

- the OpenBSD, NetBSD, FreeBSD, Alpine, Debian, Fedora, illumos, Apple, and
  generator inputs were restored from the exact official artifacts or pinned
  commits already named there;
- 4,991 Arch identities still matched the installed tree; the remaining 204
  were mapped through the official 2026-08-19 Arch Linux Archive
  `core.files`, `extra.files`, and `multilib.files` databases to 58 exact
  package archives, then verified against the ledger's decompressed hashes;
- local review bundles containing source and rendered third-party text stayed
  under `/tmp` and are not repository artifacts.

## Recorded result

The aligned ledger contains 17,811 rows: 17,726 `clean`, 76 `review`, and 9
documented `skipped`, with no hard failure and no pending human review. NetBSD
11.0 contributes 2,344 clean and 13 reviewed rows; DragonFly BSD 6.4.2
contributes 6,355 clean and 8 reviewed rows after fixes. The earlier corpus
retains 55 reviewed rows. Every candidate has a source-specific disposition.

The same run found two general mdoc punctuation defects. Multi-operand `Fa`
requests now retain formatter-owned commas both in ordinary declarations
(`3bcad69`) and in out-of-SYNOPSIS `Fo` callback declarations (`ad5a2d7`). The
table recovery comparison also exposed and fixed a shifted multiline tbl cell
(`98db676`). Each fix has an exact licensed real-page fixture and focused Rust
assertions.

Human review compared the source, mandoc output, ManT text, and structured
semantics. Remaining differences are explained per row: compact equation
spellings, literal documented escapes, table traversal order, source-requested
line continuation, labeled link destinations that remain present in ManT's
link object, mandoc-only token concatenation, and the established no-fill
normalisation policy. Do not replace those notes with a source-hash allowlist.

Nine high-confidence cases where that review establishes a source-semantic
advantage for ManT are indexed separately in
[`REFERENCE_RENDERER_DEVIATIONS.csv`](REFERENCE_RENDERER_DEVIATIONS.csv).
They cover representative table topology, token continuation, an equation
operator, Unicode and named-character fidelity, an `.St` citation, `.Ns`
spacing, and continued `.TP` aliases. A current renderer deviation may be
backed either by a reviewed `false-positive` comparison or by a
`confirmed-fixed` source conclusion with an exact regression; the coverage
checker validates both. The deviation ledger does not promote ordinary false
positives or the layout route's intentional blank-run normalization.

The audit script checkpoints large databases atomically. It does not make
concurrent writers to one CSV safe; replay corpora serially. Run
`python3 scripts/check-roff-audit-coverage.py` after every expansion to prove
that the historical and fixture baselines remain aligned.
