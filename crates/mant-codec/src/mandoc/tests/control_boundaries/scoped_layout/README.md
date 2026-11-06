# Scoped list, column and display layout

This finite source-bound matrix contains sixteen exact inputs and 80 pristine
profiles. Every source ran ASCII, UTF8, HTML, tree and lint before the behavioral
assertions were written; all profiles exit 0. The original minimal source bytes
are retained, with independent source and raw stdout/stderr hashes. Expected rows
come only from the locked pristine reference, never from a candidate renderer.

The sources were reduced from the retained real printf, bpf, bhyve, tmux, ntpq
and mdoc pages. The full original pages have their own source diagnostics; these
minimal reductions are lint-clean and must not be confused with an assertion
that all of the original page is native-clean.

## Rules and controls

The pinned primary paths are `mdoc_term.c::print_mdoc_node` (source-line entry,
then save/restore offset and rmargin), `termp_it_pre/post` (new HEAD flags and
offset + width; BODY offset; column flush rather than new line),
`termp_bd_pre/post` (one display offset and literal tab policy),
`termp_bl_pre/post`, `termp_pp_pre`, and `term.c::term_newln/term_vspace`.
Native geometry, the semantic output owner's coordinate base, and completed
physical rows have separate lifetimes.

| Axis | Sources and independent opposite controls |
|---|---|
| Sibling HEAD origin | Two nested tag items, with and without a paragraph inside the first BODY; top-level sibling tag control |
| Display origin | Literal offset-indent inside a tag BODY and a following sibling; top-level display control; negative local offset already represented by block layout |
| Column closure | Three-column header inside a tag BODY, with styled and plain header operands; top-level header control |
| Completed row | Nested enum/item followed by Pp; top-level enum/Pp control |
| List offset | One complete source with outer offset-3n, inner offset-indent, repeated inner siblings and a later outer sibling |
| Display post | Ragged display with an internal explicit break, Ed and Pp inside an ohang BODY |
| Column return | A following literal's origin after a completed column list and ordinary BODY words |
| Temporary origin | A real ti request before an authored leading blank, followed by a row without ti |

Exact internal gaps and empty rows are intentional assertions. In particular,
the second nested tag starts at the same reading origin as the first, all three
column header cells share a row, display indentation is added once, and Pp after
a completed nested list contributes one empty row. Narrow viewport wrapping may
adapt geometry, but cannot justify a sentinel-sized producer hint or a false
physical closure. JSON, Markdown, engine and UI consumers have separate tests.

The negative-display control was separately run in all five pristine profiles
before its assertion: native CODE begins at absolute column 9, including the
common five. Its reading origin 4 is the outer BODY origin 6 plus the authored
display offset -2. The literal owner's layout already carries -2, so the local
row hint remains zero. A negative private relative origin must not saturate to
65535, and this source must not be clamped back to the BODY origin either.

The final two sources were added after a bounded real-page replay found further
owner-coordinate mismatches. Each ran all five pristine profiles first, with
clean lint. A list's own offset applies at the native It pre before its width;
rebasing the HEAD owner cannot erase that offset. The combined list-offset
source keeps OUTER/LATER at reading column 3, inner FIRST/SECOND at 15, and their
bodies at 21. In the ragged display both .Ft and .Fc begin at reading column 6;
the actual Ed post closes its row and the following Pp adds one empty row.

Two final bounded reductions cover separately confirmed real-page details.
After a column list returns, CODE keeps reading origin 14; the column's old
separator cannot add a fifteenth cell. In the temporary-origin source, the
BODY origin 8 plus ti 4 plus one authored space places ipv4 at 13. AFTER returns
to 8, proving that the temporary register is consumed once. The source uses
an explicit width 6n (the same effective eight-column field as the original
default tag width) to keep the reduction lint-clean. The original ntpq source
and its diagnostics remain untouched. These two reductions also ran all five
pristine profiles before their assertions; no full-page replay was added.

## Wrapper and recording policy

These sixteen sources intentionally preserve their original minimal wrapper:
`Dd November 6, 2026`, `Dt T 1`, `Os`, NAME, then DESCRIPTION. There is no
synthetic NEXT section and no authored terminal control or requested trailing
vertical space. The recorder decodes native overstrike decoration, locates the
DESCRIPTION body, and validates the native OS/date/title footer at page origin.
Pinned `print_mdoc_foot()` first calls `term_vspace()`, so exactly its one empty
separator is removed. Any additional empty row remains in the recorded body;
the recorder never strips all trailing rows. The footer can occupy one or two
device rows without becoming body text.

The fixed five-column native section origin is removed exactly once. It is not
inferred from the shortest line. Author/field spaces, internal hard rows and
remaining empty rows are retained without folding. The helper's unit tests
protect these distinctions and reject a body that violates the common origin
or lacks the expected footer separator.

Re-record or check with the active identity preflight:

```sh
python3 -m scripts.roff.fixtures.record_scoped_layout_boundaries \
  --evidence target/audits/scoped-layout --check
```

The evidence directory contains complete source bytes and five raw profile
outputs. No product binary participates in recording the expected fixture.
