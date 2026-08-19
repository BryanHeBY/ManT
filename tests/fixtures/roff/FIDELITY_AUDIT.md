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

The ledger began with the host Arch Linux manual tree and independently
installed Miniconda manual trees. It also records pinned package corpora so a
distribution comparison can be repeated without confusing package drift with
a parser regression:

| Corpus | Package inputs | Exact source coverage | Human disposition |
| --- | --- | ---: | --- |
| `debian-sid-2026-07-21-amd64` | Debian packages `cpio 2.15+dfsg-2.1`, `dash 0.5.12-12`, and `groff 1.24.1-1` | 52 pages, 49 distinct decompressed sources | 45 clean; 7 reviewed renderer/device-condition differences |
| `fedora44-2026-07-20-x86_64` | Fedora packages `bash 5.3.9-3.fc44`, `clang 22.1.1-2.fc44`, `gcc 16.0.1-0.10.fc44`, `git-core-doc 2.53.0-1.fc44`, and `tar 1.35-8.fc44` | 258 pages discovered; 257 rendered; 1 exact source reused from an earlier completed corpus | 202 clean; 55 reviewed aliases whose embedded `.so` chains are deliberately not expanded by ManT |
| `alpine-3.24.1-x86_64` | Official Alpine 3.24.1 APKs: `man-pages 6.18-r0`, `busybox-doc 1.37.0-r31`, `openssl-doc 3.5.7-r0`, and `util-linux-doc 2.42.1-r0` | 425 incremental ledger rows; 394 syntax-prioritized pages rechecked after the table fixes | 412 no-candidate scans; 8 reviewed link or formatter token-boundary differences; 5 confirmed table findings fixed |
| `openbsd-7.9-amd64` | Official OpenBSD 7.9 `man79.tgz` for amd64 | 339 syntax-prioritized and source-directed pages from 2,902 distinct sources | 332 latest clean scans; 7 reviewed candidates; 8 real lowering defects fixed and 22 source-checked false positives |
| `netbsd-10.1-amd64` | Official NetBSD 10.1 amd64 `man.tar.xz` set | 541 syntax-prioritized and source-directed pages from 2,544 distinct sources | 523 latest clean scans; 18 reviewed candidates; 2 real lowering defects fixed and 19 source-checked false positives |
| `freebsd-15.1-amd64` | Official FreeBSD 15.1-RELEASE amd64 `base.txz` | 417 stable, syntax-prioritized, and source-directed pages from 11,183 paths / 4,085 distinct sources | 405 latest clean scans; 12 reviewed candidates; 5 real lowering defects fixed and 10 source-checked false positives |
| `illumos-gate-e8f5c080` | Official illumos-gate read-only mirror at commit `e8f5c080` | 918 stable, syntax-prioritized, section-balanced, and source-directed pages from 4,627 distinct sources | 914 clean automated scans; 4 reviewed reference-renderer differences; 1 mdoc link-lowering defect fixed |
| `apple-oss-manpages-pinned` | Apple `bsdmanpages-56` and `man-171` source tags | 36 syntax-prioritized pages | 34 clean; 2 aliases whose targets are absent from the individual official source repositories |
| `apple-oss-command-repos-pinned` | Apple `shell_cmds-329`, `file_cmds-479`, and `system_cmds-1042.120.1` source tags | 70 syntax-prioritized pages from 185 sources | 66 clean; 1 real request-lowering defect fixed; 3 source-layout or formatter differences reviewed |
| `generator-ronn-scdoc-pinned` | ronn-ng `v0.10.1` committed output and scdoc `1.9.7` locally generated output | All 4 generated pages | 4 clean; no candidate findings |
| `generator-clap-mangen-0.3.0` | Official `clap_mangen-v0.3.0` roff snapshots | All 21 generated pages | 21 clean; no candidate findings |

The Debian review found two real, general `tbl` gaps before the final counts:
tables nested in an unfilled mdoc display were discarded, and a legal
`T}\tT{` boundary failed to open the next multiline cell. Both now have
minimal Rust regressions; the ledger records the corrected page bytes rather
than retaining stale failures. The remaining review rows state the precise
false-positive reason instead of acting as a generic allowlist.

The earlier BSD and Apple expansion found four general compatibility gaps. Stateful
mdoc `Sm` spacing now crosses list-item and structural boundaries correctly;
`Rs` bibliography entries retain formatter-generated terminal punctuation;
the device-only `ti` argument is no longer exposed as document text; and the
pinned parser recognizes the current `-isoC-2023` and `-p1003.1-2024`
standard names. Each change has a minimal Rust regression. Remaining BSD
findings were checked against the source and both visible outputs before being
classified. In particular, differences in historical `St` wording, Pod
term/body joining, URL punctuation, and terminal table layout do not represent
discarded source content. A later complete BSD source-directed pass over `.Lk`,
`.Mt`, and `.Bx` occurrences also verified that `Bx`'s lifecycle aliases and
version/release forms arrive from libmandoc as compact generated tokens.
Lowering now uses the macro's authored AST arguments to render the mdoc-defined
portable lifecycle text and canonical `versionBSD release` form. The remaining
`.St` signals are formatter-owned alternate titles for the same standards,
rather than missing source text.

The FreeBSD and illumos expansion then moved beyond section-balanced samples
to inverse-frequency AST selection. It found five additional general FreeBSD
gaps: command heads lost in extended `Nm` synopsis blocks; mdoc requests
flattened inside `tbl` text cells; nested `Sm` state lost between structural
siblings; `.Pp`-separated alternatives concatenated inside extended
definition heads; and punctuation following implicit mdoc enclosures silently
discarded. Each defect was checked in the real page and reduced to a focused
Rust regression. The initial illumos sample added SysV-derived section families
and traditional man/eqn inputs; its equation finding is terminal fraction-bar
geometry rather than a missing operand or operator. A later balanced 746-page
pass across all section suffixes found one general mdoc defect: `.Lk URL .`
treated sentence punctuation as its visible label, hiding the URL in text
renderers. Link lowering now keeps an unlabeled target visible and appends the
punctuation outside the link. The remaining new `csh(1)` candidate is a
reference tokenisation artefact: its definition term and following body remain
separate and intact in ManT.

The Alpine 3.24.1 pass adds a musl-based Linux release plus BusyBox, OpenSSL's
Pod-derived pages, and util-linux generated manuals. It manually inspected all
11 differential candidates and source-to-IR behavior for the two confirmed
findings. A commented-out `T{` marker had let source-assisted `tbl` recovery
claim later ordinary rows, and a `\\^` vertical-span control marker was rendered
as visible text. Both are now reduced to focused Rust regressions. The remaining
candidates retain their source text: they arise from deliberately visible link
destinations, reference token concatenation across definition terms, or a
different but structurally faithful traversal of a vertically spanned table.

The pinned inputs can be independently identified as follows:

- OpenBSD `man79.tgz` SHA-256: `7a5e66facf678b41b6b4722b073c357d1eea27facaf4610701ffbec1c80751af`.
- NetBSD `man.tar.xz` SHA-512: `79f523d692c734a3a18921a4ec9cc7e0c2d1ae567bf4911a04f85dc78281c15f0b0fd68d201e9050daba498068df2c59660d3633e3abdbbc3d651ea8a26dd5a3`.
- FreeBSD 15.1-RELEASE amd64 `base.txz` SHA-256: `3768988b151c20f965679062b065c63a977d6bbb9f47fd83695ec2c40790c18f`.
- Alpine 3.24.1 x86_64 APK SHA-256 values: `man-pages-6.18-r0.apk` `2ba1a7a9f579495cdb084fab1bc3efc72266402b172f94279549affb30506916`; `busybox-doc-1.37.0-r31.apk` `508b598dc2f6f91a2078418870ebc1ac00138ef2c7fd059d9a640e0a65ec5ff4`; `openssl-doc-3.5.7-r0.apk` `4553b42473e80dbfff28d287c2f5b8ed4c1f838c7d48782e40635654cd4769d8`; `util-linux-doc-2.42.1-r0.apk` `acf57e3589d3c0ee5b95aea6e9dd9df62ac71829da815dd4fc38e174deb07d2b`.
- The illumos-gate mirror was pinned at commit `e8f5c080cc0b7997410d860afd787df30ba1cf2d`; its source files retain their upstream CDDL headers.
- Apple tags resolve to commits `c62819a460dcc7906465c4c213a7fc0211148960`, `d798e66636621e416604cdeacc01ca45d964ad2d`, `298787009e5432c5e4c378a077f98267077e3495`, `659a8a301e2acf0343f8b8673a154a2ca4d07084`, and `15832a892bdd86cf3e3f2fde9265142f714437c8` in the table's repository order.
- ronn-ng `v0.10.1`, scdoc `1.9.7`, and `clap_mangen-v0.3.0` resolve to commits `bc667fe55b2df9fe54bd34d8f589ab58a3e81371`, `5c782cda95427e7170bab7a9d7eef19c7c2d12d0`, and `f0d30d961d26f8fb636b33242256fca73a717f77` respectively.

The generator sources are MIT or Apache-2.0/MIT licensed. Their outputs, the
FreeBSD release tree, and the illumos checkout were used as local discovery
inputs and were not copied into the repository, so no new third-party fixture
or redistribution notice is required.

A `skipped` row is historical, incomplete coverage rather than an accepted
disposition. Normal incremental runs select such a row again even when its
content hash is unchanged.

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

For deliberate visual and structural inspection, pass `--review-dir /tmp/mant-roff-review` to the audit command. It produces a local manifest with one path-safe directory per selected page containing the decompressed source, reference text, ManT text, and finding metadata. This is review evidence only: do not commit the bundle or treat its renderer output as a snapshot fixture. Record the conclusion in this ledger and reduce a real defect to the existing licensed fixture system instead.

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
