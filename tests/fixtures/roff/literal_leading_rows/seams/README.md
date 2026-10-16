# Literal owner handoff inputs

These six original Apache-2.0 inputs supplement the parent matrix without
changing its 23-case census. They test ownership seams rather than adding
another rendering policy. Every complete source ran pristine CVS
`20261013T025419Z` in ASCII, UTF-8, tree and lint modes before these facts were
written. All 24 runs exited successfully, with no lint diagnostics.

`oracle.json` preserves the exact arguments, raw outputs and hashes. The recipe
uses `LC_ALL=C.UTF-8`, `-Ios=ManT`, and `-Owidth=80` for terminal profiles. Input
is a byte-identical file under its recorded basename in a temporary directory.
`cases.json` separates observed native rows and AST facts from notes about the
approved complete-owner reading exception. It does not derive an expectation
from a ManT renderer.

| Source | Native evidence and handoff to check |
| --- | --- |
| hp-literal-headless-ip | Adjacent HP and IP blocks have empty HEADs. HP BODY contains empty TEXT then bold `--one`; IP BODY contains `Description.`. A removed literal prefix must not invalidate the actual semantic pair witness. |
| inset-shared-literal | An inset It has HEAD `HEAD` and BODY `BODY`, both no-fill. The BODY source-line event ends the HEAD row; its leading break must remain when that BODY is represented as a shared literal. |
| nf-zero-width-row | Nonempty TEXT `\&` occupies a native empty row. It is not the empty-TEXT vertical-space branch. |
| nf-word-break-row | Nonempty TEXT `\p` leaves a buffered break consumed by the next no-fill source-line event. It is not an empty source TEXT or a helper terminator. |
| nf-independent-row-origins | PP margin precedes an independent literal with one initial empty row. Later `.ti +3n` shifts only `SHIFTED`; `CODE` and `PLAIN` retain the same origin. |
| tbl-literal-cell | T{ contains `.nf`, an empty line and `CODE`. Pristine tbl renders `CODE` as one row and exposes a tbl cell plus a request sibling, not a nested literal AST. Source-backed codec recovery must establish its own actual recovered owner before normalization. |

## Upstream execution boundaries

- `man_term.c::print_man_node` and `mdoc_term.c::print_mdoc_node` execute
  `NODE_NOFILL`/`NODE_LINE` before dispatch. Empty TEXT, `\&` and `\p` take
  distinct paths, even when the final screen row is empty.
- `term.c::term_word` buffers `ASCII_NBRZW` for `ESCAPE_IGNORE` (`\&`) and a
  newline for `ESCAPE_BREAK` (`\p`). `term_fill` treats the former as a graph
  cell and the latter as a word-end break request.
- `man_term.c::pre_HP` explicitly skips its HEAD; the declaration is in BODY.
  `pre_IP`/`post_IP` handle its actual HEAD/BODY independently. Native block
  spacing must not be counted as an initial row owned by the literal HP.
- `mdoc_term.c::termp_it_pre` writes an inset BODY's initial fixed space when
  HEAD has children. The later BODY source-line event provides the physical
  separation; output ownership must not erase it.
- `roff_term.c::roff_term_pre_ti` first closes the prior line, then adjusts the
  temporary origin. Removing a prior empty owner row must rebase row indices
  without removing this later resolved origin.
- `roff.c::roff_parseln` and `roff_req_or_macro` decide which T{} input reaches
  tbl and which request becomes a sibling. `tbl_data.c::tbl_cdata` retains
  continued cell text; `tbl_term.c::tbl_literal`/`tbl_word` execute that cell in
  the native table context. Do not infer a nested literal node from the source
  spelling `.nf` alone.

Rows in `nativeBodyRows` resolve terminal overstrikes, then remove only the
common structural prefix measured from `BEFORE`. Empty rows, extra spaces and
NBSP remain exact. In particular, the HP and PP examples include independent
native paragraph margins in addition to their literal's initial row.

The approved reading exception remains the parent README's policy. It runs on
complete roff owners, preserves shared HEAD/BODY boundaries, and leaves Markdown
input unchanged. The seam notes neither broaden it to source AST nodes nor
authorize stripping every visually empty native row.
