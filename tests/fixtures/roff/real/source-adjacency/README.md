# Complete sources for shared execution regressions

These four complete manuals come from the retained November 3, 2026
1,000-page audit. They preserve the exact upstream source, including its
copyright and license notices. No excerpt, synthetic reproducer, macro edit,
or generated replacement is presented as an upstream manual.

| Fixture | Source and exact member | Coverage |
| --- | --- | --- |
| `arch-tmux.1.gz` | Arch Linux `tmux-3.8-1-x86_64.pkg.tar.zst`, `usr/share/man/man1/tmux.1.gz`; embedded OpenBSD revision 1.1173 | Previously confirmed missing `(` in `Given twice (-CC) disables echo.`; the complete `-C` definition and its `CONTROL MODE` link must retain their owner and query binding. |
| `openbsd-date.1` | OpenBSD 7.9 amd64 `man79.tgz`, `./usr/share/man/man1/date.1`; source revision 1.72 | A small affected control for `.Sm off`/`.Sm on`, `.Ns`, and nested optional synopsis arguments: `[[[[[[cc]yy]mm]dd]HH]MM[.SS]]`. The original audit did not report this page as a confirmed defect. |
| `dragonfly-regex.3.gz` | DragonFly BSD 6.4.2 `dfly-x86_64-6.4.2_REL.iso.bz2`, `usr/share/man/man3/regex.3.gz` | Complete real `Fo`/`Fa` synopsis declarations with return types, consecutive argument lists, generated commas and semicolons; also retains the previously observed enclosure example `('?',`. |
| `illumos-ksh93.1` | `illumos/illumos-gate` commit `e8f5c080cc0b7997410d860afd787df30ba1cf2d`, `usr/src/man/man1/ksh93.1` | Previously confirmed Markdown delimiter leakage in `$((arithmetic_expression))`, plus a real inline `Fo` declaration and local section links. |

[`manifest.json`](manifest.json) records origin URLs, release or commit,
archive and member names, archive/source/fixture SHA-256 values, license
mapping, and source preservation. The two gzip files are the original
compressed members. The two uncompressed files are exact complete upstream
bytes. The original audit manifest SHA-256 is recorded to identify the
selection independently of the temporary evidence directory.

The source archives were already retained locally; fixture promotion and
archive/member verification took place on November 4, 2026. This is not a
claim that the archives were downloaded on that date. The tmux package member
was independently extracted from the retained package and matched the audited
decompressed hash. OpenBSD date was matched against its tar member; the
DragonFly member and illumos file were matched against the retained source
and audited hashes.

The tmux manual uses the complete ISC notice preserved in
[`TMUX-MANUAL-ISC.txt`](../LICENSES/TMUX-MANUAL-ISC.txt), including its authored
`LOSS OF MIND` disclaimer. The OpenBSD and DragonFly manuals retain their
individual copyright notices and complete BSD 3-clause terms; the shared
terms are also in [`BSD-3-Clause.txt`](../LICENSES/BSD-3-Clause.txt).
The illumos file explicitly selects Eclipse Public License 1.0 in its header;
its AT&T, Sun, and OmniOS notices remain intact. Complete terms are in
[`EPL-1.0.txt`](../LICENSES/EPL-1.0.txt), copied unchanged from the installed
SPDX license text (SHA-256
`f3d8dc494be2e7ea9cd31ac2fb902e35dce7ff8181b9b30d05861b47c339cb25`),
corresponding to the manual's <http://www.eclipse.org/org/documents/epl-v10.html>
license reference. These licenses cover only these test sources.

Before writing the regression assertions, all four exact sources were freshly
run with the pinned pristine mandoc CVS reference in ASCII, UTF-8, HTML,
tree and lint profiles. The first four profiles exited 0 for each file.
**All four lint profiles exited 1**, reporting missing referenced manuals in
the local manual database at STYLE level; these are retained diagnostics,
not clean lint results. The permanent tests assert the selected content,
source owner, link/query relationships, JSON transport, and two actual
Markdown readbacks. They do not claim whole-page layout, every font, or
arbitrary roff/Markdown correctness.

The shared structure, projection, target and semantic ledgers include all four
sources. The pinned-CVS supplement records exact source identities separately
from the historical distribution baseline. Its layout observer cannot uniquely
match 48 tmux row pairs and one ksh93 pair because its source keys retain roff
escape spellings. Independent comparison of every affected decoded literal
owner found the same internal content and physical rows after a constant owner
origin; an authored tmux TAB retains its own identity instead of native space
expansion. These are reviewed observer limitations, not evidence of complete
column or whole-page layout equivalence.
