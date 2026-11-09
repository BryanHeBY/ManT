# Complete mathematical manual sources

These eight complete manuals were selected from the November 9, 2026
mathematical Markdown audit. They retain their original content, comments,
notices and line endings. The X11 file is the original gzip package member.

| Fixture | Immutable source | Formulas | Display owners | Selected coverage |
| --- | --- | --- | --- | --- |
| `posix-float.h.0p` | `manpages-posix_2017a.orig.tar.xz`, `man-pages-posix-2017/man0p/float.h.0p` | 19 | 6 | Complete inline descriptions, sums, fractions, floors and ceilings |
| `posix-stdint.h.0p` | Same archive, `man-pages-posix-2017/man0p/stdint.h.0p` | 15 | 0 | Nested integer-limit definitions and quoted literal carets |
| `posix-drand48.3p` | Same archive, `man-pages-posix-2017/man3p/drand48.3p` | 27 | 0 | Source-line-initial inline formulas, scripts and complete sentences |
| `openbsd-erf.3` | OpenBSD `src` commit `3c34c3840589cf2f3a60107b434f42c3253fb3e2`, `lib/libm/man/erf.3` | 1 | 1 | Fraction, radical, integral limits and nested powers |
| `openbsd-lgamma.3` | Same commit, `lib/libm/man/lgamma.3` | 2 | 2 | Gamma integral and reflection formula |
| `freebsd-stdc_bit_width.3` | FreeBSD `src` commit `769c49de261fbbd1293adab9188e5099516675cf`, `lib/libc/stdbit/stdc_bit_width.3` | 1 | 1 | Logarithm subscript and floor brackets |
| `illumos-eqn.1` | illumos-gate commit `b1c424a261b16b9c81280c0eb6de8b2f9f4ffacb`, `usr/src/man/man1/eqn.1` | 11 | 0 | Matrix axes, vertical piles, scripts, limits, fences and accents |
| `xorg-XDrawArc.3.gz` | Arch `libx11-1.8.14-1-x86_64.pkg.tar.zst`, `usr/share/man/man3/XDrawArc.3.gz` | 10 | 0 | Nine active inline formulas and one readable formula in a complete literal owner |

[`manifest.json`](manifest.json) records the original URLs, archive or commit,
complete decompressed source and stored-file hashes, byte counts, license
bindings and five pristine CVS run receipts. The X11 installed file was
compared byte-for-byte with the downloaded official archive member before
promotion. Source and transport hashes are checked without network access.

## Licenses

- The POSIX pages retain the IEEE/The Open Group 2018 reprint notice. The
  complete accompanying permission statement is in
  [`POSIX-COPYRIGHT.txt`](../LICENSES/POSIX-COPYRIGHT.txt).
- OpenBSD `erf.3` and `lgamma.3` retain their complete University of California
  notices and BSD 3-clause terms. Shared terms are in
  [`BSD-3-Clause.txt`](../LICENSES/BSD-3-Clause.txt).
- FreeBSD `stdc_bit_width.3` retains Robert Clausecker's 2025 copyright and
  `SPDX-License-Identifier: BSD-2-Clause`. The complete SPDX standard terms,
  carrying that source's copyright line, are in
  [`FREEBSD-STDBIT-BSD-2-Clause.txt`](../LICENSES/FREEBSD-STDBIT-BSD-2-Clause.txt).
- illumos `eqn.1` retains its Sun Microsystems 2002 notice and CDDL header.
  [`CDDL-1.0.txt`](../LICENSES/CDDL-1.0.txt) is the complete
  `usr/src/OPENSOLARIS.LICENSE` from the same immutable commit.
- X11 `XDrawArc.3` retains the X Consortium and Digital Equipment
  Corporation/Tektronix notices and their distinct documentation permissions.
  [`XORG-XDRAWARC-MANUAL.txt`](../LICENSES/XORG-XDRAWARC-MANUAL.txt) copies the
  entire notice through its final warranty sentence, removing only roff comment
  prefixes. Its exact source-line range is also hash-bound in the manifest.

These licenses apply to their third-party manuals. ManT uses its repository
license.

## Regression boundary

All eight exact inputs ran through the pinned pristine CVS ASCII, UTF-8,
HTML, tree and lint profiles before assertions were written. ASCII, UTF-8,
HTML and tree exited 0. Lint exited 3 for `float.h`, 2 for `stdint.h`,
`drand48`, illumos `eqn` and X11 `XDrawArc`, 1 for FreeBSD `stdc_bit_width`,
and 0 for both OpenBSD pages. The receipts preserve those diagnostics.

`mant-engine/tests/real_mathematical_export.rs` checks all 86 mathematical
owners after an actual query JSON round trip, their readable caches,
MathML/LaTeX encoding, 85 active Markdown math events and the one reported
literal-context fallback. Focused assertions check complete POSIX sentences,
literal caret roles, matrix column/row order, mathematical search and the
single X11 code owner. Native source-line position and executed delimiter
origin are checked independently by the structure profiler's mutation test.

The structure, Markdown projection, target and semantic ledgers cover all
eight identities. The active CVS content/layout supplements retain their
reviewed findings. Four content signals concern attached terminal script or
fraction tokens whose shared reading representation uses spaces. Two layout
observations remain partial around an unclassified `P`, a macro definition
and physical eqn input lines. Their ledger notes preserve the exact scope.

## Wider audit scope

The expanded audit exercised 1,824 distinct decompressed source hashes across
POSIX 2017a, mathematical library directories from Apple, NetBSD, OpenBSD,
FreeBSD and illumos, retained Linux/BSD release samples, X11 and the FreeBSD
`opengl-man` 657.2 distfile. Pure redirects and duplicate source hashes were
excluded from this total. Of these, 92 pages contained 860 executed formulas.

The native HTML mathematical roots and IR owners matched in count. Comparing
visible mathematical atoms included `mfenced` attributes. GNU `ldots`
normalization followed the declared compatibility extension. Default and
explicit LaTeX exports matched for every page, and readable exports contained
zero active math events. All 849 active formulas, covering 358 distinct LaTeX
bodies, rendered with MathJax 3.2.2 and KaTeX 0.19.1. Eleven formulas used the
declared readable literal-context fallback. Sixteen browser comparison pages
were rendered. Nine received focused visual inspection of scripts, limits,
fractions, matrix rows and piecewise layouts.

Source syntax still determines the mathematical structure. Quoted `"^"` and
ordinary underscores retain their literal spelling. The archived OpenGL
`gluProject.3` also places an `.EN` after equation text on one source line.
CVS `roff_parseln` passes that line and following input to `eqn_read` until
an actual ending request, so the resulting atom text includes those tokens.
The audit records that source behavior alongside target-format approximation
reports. The fixed CI sources cover the selected contracts described above.
