# Complete sources for scoped layout regressions

These three complete manuals are unchanged sources from the November 3, 2026
1,000-page audit. They exercise ordinary nested definition bodies, scoped
literal origins and the physical rows of column-list cells. They are not
excerpts or rewritten reproductions. The existing exact tmux manual in
[`source-adjacency/`](../source-adjacency/README.md) supplies the larger nested
list control without duplicating its bytes here.

| Fixture | Immutable source | Selected coverage |
| --- | --- | --- |
| `netbsd-printf.1` | NetBSD 11.0 amd64 `binary/sets/man.tar.xz`, `./usr/share/man/man1/printf.1`; embedded revision 1.41 | Nested tag-list bodies must retain finite row origins after complete ordinary `It` execution. |
| `openbsd-bpf.4` | OpenBSD 7.9 amd64 `man79.tgz`, `./usr/share/man/man4/bpf.4`; embedded revision 1.51 | Complete ioctl definitions followed by literal C structures, scoped indentation and authored TABs. |
| `illumos-bhyve.8` | illumos-gate commit `e8f5c080cc0b7997410d860afd787df30ba1cf2d`, `usr/src/man/man8/bhyve.8` | The `Backend`, `Path` and `Description` header cells share one authored source row; nested options also exercise scoped origins. |

[`manifest.json`](manifest.json) binds each fixture to the original audit
manifest, source identity, complete source and fixture SHA-256, release or
commit, archive member, license notice, and the five captured pristine CVS
profiles. Fixture promotion on November 6, 2026 reused the retained source;
this date is not a claim that the release archives were downloaded that day.

The NetBSD and OpenBSD release archives were hashed and their exact members
compared with the audited sources before copying. The illumos GitHub contents
API was queried at the full immutable commit; its complete decoded file and
reported Git blob SHA-1 `d65d1337d20cce160bdf6771adb054682a7b1341` matched
the retained source. No source transformation or recompression was applied.
The original notices, source comments and line endings remain intact.

## Fixture licenses

- NetBSD `printf.1` retains its Regents of the University of California notice,
  IEEE contribution acknowledgement and complete BSD 3-clause terms. Shared
  terms are in [`BSD-3-Clause.txt`](../LICENSES/BSD-3-Clause.txt).
- OpenBSD `bpf.4` has a **custom Berkeley/Lawrence Berkeley Laboratory notice
  with an advertising acknowledgement**, not ordinary BSD 3-clause terms.
  Its complete authored notice, including the advertising condition,
  non-endorsement condition, disclaimer and `enet.4` derivation, is copied to
  [`BPF-MANUAL-LICENSE.txt`](../LICENSES/BPF-MANUAL-LICENSE.txt).
- illumos `bhyve.8` retains Peter Grehan, Hans Rosenfeld, OmniOSce and Oxide
  notices and its manual-specific BSD 2-clause terms. The complete notice is
  in [`BHYVE-MANUAL-BSD-2-Clause.txt`](../LICENSES/BHYVE-MANUAL-BSD-2-Clause.txt);
  the differently authored libarchive license is not substituted for it.

These terms apply only to these third-party manuals, not to ManT itself.

## Verification boundary

The complete source files were run through the pinned pristine CVS reference
in ASCII, UTF-8, HTML, tree and lint profiles before behavioral assertions
were written. The first four profiles exited 0 for all three pages. Lint
exited **2 for printf**, **1 for bpf**, and **1 for bhyve**; these existing
diagnostics are retained, not called clean lint. The manifest records exact
profile output hashes and exit statuses.

The source-identity test checks unchanged bytes and complete license binding
without a network connection or a host manual database. Consumer regression
tests separately exercise the selected rows, origins, owners, links, JSON
and Markdown readbacks. Neither the catalogue nor a successful parse asserts
whole-page equivalence, every font, arbitrary column geometry or all roff
behavior.

The structure, Markdown projection, target, semantic and active-CVS fidelity
ledgers include all three exact identities. The layout observer retains a
reviewed partial result for bhyve: its lexical keys leave the backslash-e escape undecoded in
five literal owners. Independent comparison of all 30 decoded payload rows
preserves their authored spaces and physical rows after only the common page
origin is removed. That limitation is not relabeled as full-page clean.
