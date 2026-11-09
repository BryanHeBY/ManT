# Complete manual for filled source-line boundaries

`xcb_composite_unredirect_window.3.gz` is the original gzip member
`usr/share/man/man3/xcb_composite_unredirect_window.3.gz` from Arch Linux's
`libxcb-1.17.0-1-x86_64.pkg.tar.zst`. The archive came from the retained
local package cache and its gzip member matched the installed copy exactly
on November 9, 2026. The archived member is complete and unchanged.
[`manifest.json`](manifest.json) binds the package, gzip and decompressed
source hashes to the original source and archive locations.

The complete X11 notice in
[`LIBXCB-COPYING.txt`](../LICENSES/LIBXCB-COPYING.txt) is the unmodified
`usr/share/licenses/libxcb/COPYING` member from the same package. It
applies to the fixture.

The exact source ran through the pinned pristine CVS ASCII, UTF-8, HTML,
tree and lint profiles before writing the regression. The first four
profiles exited 0. Lint exited 2 and reported three filled-TAB warnings,
a filled blank-line warning, the unparsed date and STYLE diagnostics.

Tree output places the raw line-initial TAB at lines 18 and 21 in the
`window` and `update` IP bodies and at line 24 in DESCRIPTION's ordinary
paragraph. All three leaves carry NODE_LINE and are filled. In
`man_term.c::print_man_node()`, the leading-space newline checks for ASCII
space. The 160-column native UTF-8 profile keeps each phrase with its
predecessor on one row, while native HTML keeps it in the original DD or P
with no intervening BR. The regression checks raw TEXT ownership,
the transported IR, plain rendering and two Markdown readbacks. It leaves
terminal-specific tab stops and viewport soft wrapping to the consumer.

The structure, projection, target, semantic and pinned-CVS content/layout
ledgers cover this exact source identity. The layout audit checks its
source-gated signals, while the focused test checks the three filled seams.
