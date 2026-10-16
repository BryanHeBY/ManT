# Literal leading-row regression inputs

These 23 original Apache-2.0 inputs separate native execution facts from the
approved reading projection for complete roff `Preformatted` owners. They are
not terminal screenshots or a claim that mandoc discards the first blank row.

Every complete source ran the locked pristine CVS `20261013T025419Z` reference
in ASCII, UTF-8, tree and lint modes before its expectations were recorded.
All sources are lint-clean. The recipe fixes the locale, 80-column terminal
profiles, input basename and `-Ios=ManT` default so mdoc footer and tree hashes
do not depend on the host operating system. Source `.Os` remains argument-free;
an explicit source argument would itself introduce a style diagnostic.

`cases.json` records exact source hashes, raw profile hashes, native body rows
and empty no-fill TEXT node source lines. Body rows are the complete rows
strictly between `BEFORE` and `AFTER`, with terminal overstrikes resolved and
only their common structural left prefix removed. Empty rows, author spaces
and NBSP are not trimmed or collapsed. The full raw receipts are retained in
the task audit, rather than copied into a second permanent source corpus.

## Native rules

- `man_term.c::print_man_node` and `mdoc_term.c::print_mdoc_node` process
  `NODE_NOFILL`/`NODE_LINE` before dispatch. An empty TEXT executes
  `term_vspace`, except for the explicitly continued-line branch.
- `term.c::term_newln` closes an occupied row; `term_vspace` additionally
  asserts an empty row, or consumes a previously requested negative spacing.
- `man_term.c::pre_literal` closes the preceding line at EX/EE boundaries.
- `mdoc_term.c::termp_bd_pre/post` governs display entry and closing. The
  fixtures use `Bd -literal -compact` so block spacing is not confused with an
  empty row inside the literal body.
- `roff_term.c::roff_term_pre_sp` and the shared source-line entry rules keep
  control-only nodes separate from physical empty TEXT rows. Font controls
  between rows must not change which completed owner is normalized.

## Approved reading exception

After roff execution and complete IR owner assembly, before semantic
identification, omit exactly one empty initial logical row when the same
`Preformatted` owner has later body content. Preserve zero or two-plus initial
empty rows, an entirely empty owner, and a first row containing authored
whitespace. Internal and trailing rows remain intact. The normalization must
not run on intermediate drains, other block types or Markdown input.

This intentionally differs from the native rows in `cases.json` for qualifying
owners; the `readingException` object records that decision separately.
The policy is based on resolved IR content, not trimmed native screen text:
the native terminal may omit trailing author spaces or Tab positioning cells,
while the corresponding source TEXT is nonempty and must remain protected.

| Inputs | Boundary |
| --- | --- |
| nf/ex/bd-leading-0/1/2 | Three entry mechanisms and exact initial empty-row cardinality |
| nf-empty-zero/one, ex-empty-two, bd-empty-one | No later body; no normalization |
| nf-authored-spaces, ex-authored-tab, bd-authored-nbsp | Nonempty authored whitespace cannot be mistaken for an empty row |
| nf-internal-and-trailing-rows | One removable initial row, an internal row and two trailing rows |
| nf-controls-between-leading-rows, nf-control-after-leading-row | Intermediate formatter drains must not normalize a partial owner |
| bd-style-scope-after-leading-row | Font scope pre/post inside one complete display |
| ex-link-after-leading-row, bd-anchor-after-leading-row | Link, anchor and styled content retain their identity after rebasing |
| nf-independent-literal-owners | A filled paragraph separates two complete owners |

`python3 -m unittest scripts.roff.tests.test_literal_leading_rows` checks the
source identities, profile recipe and independent matrix census without
requiring a host mandoc executable. Rust producer and consumer tests own
normalization, mappings, semantic names, links and actual reader assertions.
