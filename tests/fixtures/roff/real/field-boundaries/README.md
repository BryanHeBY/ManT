# Complete manuals for native execution boundaries

These four unchanged complete manuals were selected from the November 3,
2026, 1,000-page audit. The original retained release archives were independently
hashed and their exact members re-extracted on November 6. This reused retained
archives; it was not a new network retrieval. No source was trimmed, repaired,
line-normalized or recompressed.

| Fixture | Fixed source | Selected contract |
| --- | --- | --- |
| `debian-chem.1.gz` | Debian sid 2026-07-21 amd64, `groff_1.24.1-1_amd64.deb`, original `data.tar.xz` member | `.SY` exit restores ordinary filled prose width before later requests and structures. |
| `netbsd-scmdctl.1` | NetBSD 11.0 amd64 `man.tar.xz`, revision 1.2 | The motor table's 16 physical spans, including 10 data and six rules, retain one owner and their cell order. |
| `openbsd-mandoc.1` | OpenBSD 7.9 amd64 `man79.tgz`, revision 1.199 | A display inside an enum nested under a definition BODY keeps the executed parent origin and its local six-column offset. |
| `openbsd-ksh.1` | OpenBSD 7.9 amd64 `man79.tgz`, revision 1.223 | A literal nested in a bullet list keeps its local six-column offset and restores the following BODY origin. |

[`manifest.json`](manifest.json) binds source identities to the original audit,
release URL/member, archive hash, compressed transport and decompressed source
hashes, byte counts, complete embedded notices, and all five captured pristine
profiles. `chem` retains its exact original gzip member; the other three retain
their exact complete uncompressed release members.

## Licenses

- `chem` is GPL-3.0-or-later, as its original FSF 2006–2020 notice states.
  The complete shared terms are in [`GPL-3.0-or-later.txt`](../LICENSES/GPL-3.0-or-later.txt).
  Its license is not the GFDL used by other groff manuals.
- `scmdctl` retains Brad Spencer's complete 2021 ISC notice in the source and
  [`SCMDCTL-MANUAL-ISC.txt`](../LICENSES/SCMDCTL-MANUAL-ISC.txt).
- `mandoc` retains Ingo Schwarze and Kristaps Dzonsons's complete ISC notice in
  the source and [`MANDOC-MANUAL-ISC.txt`](../LICENSES/MANDOC-MANUAL-ISC.txt).
- `ksh` expressly declares Public Domain. Its declaration and revision remain
  in the source and [`KSH-MANUAL-PUBLIC-DOMAIN.txt`](../LICENSES/KSH-MANUAL-PUBLIC-DOMAIN.txt);
  BSD or ISC terms are not substituted for that declaration.

These notices govern the third-party manuals, not ManT's own code.

## Verification boundaries

Before behavioral assertions, each complete source ran the pinned pristine
reference in ASCII, UTF-8, HTML, tree and lint profiles. The first four exited
zero. Lint exited **4 for chem**, **2 for scmdctl**, **0 for mandoc**, and
**1 for ksh**; the manifest retains the real diagnostics. Successful rendering
does not mean lint-clean or whole-page equivalence.

All four also ran GNU groff 1.24.2 in ASCII/UTF-8 with `-t -mandoc`,
`LL=78n`, `LT=78n`, C.UTF-8 and UTC. Their selected source-event and relative
origin witnesses agree; justification, hyphenation and table junction glyphs
can differ. Groff remains an auxiliary comparison and is not needed by normal
CI. No test consults the host manual database or downloads a source.

Real-source regression tests check selected topology, glyph origin, prose,
JSON and repeated Markdown contracts. The source identity test checks exact
bytes and full notices. Six audit ledgers separately record their actual
observations and remaining Review axes; catalogue inclusion does not declare
every font, native device geometry or consumer behavior clean.

Existing tmux, printf, bpf and bhyve controls are reused from
[`source-adjacency`](../source-adjacency/README.md) and
[`scoped-layout`](../scoped-layout/README.md), without duplicate copies.
