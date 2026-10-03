# Native text cell contracts

The 51 complete sources in `cases.json` were run with the locked pristine
CVS reference before writing the assertions. Reference identity:
`cvs-20260927T130954Z-linux-x86_64-gcc-16.2.1-reproducible-20260930`,
binary SHA-256
`482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6`.

Expectations come from UTF-8 output with terminal font overstrikes resolved,
the five-column common body margin removed, and exactly one section separator
before `ENDTEST` excluded. Authored empty rows, padding and word boundaries
are retained. Sources use a fixed date; OS furniture is outside that region.
The 50 macro cases also ran ASCII, HTML and lint; all selected sources passed
lint. The complete printable ASCII alphabet separately ran UTF-8.

Literal, HANG and TAG cases exercise ordinary and emphasized words, Unicode
(including wide and combining scalars), font escapes, internal and pending
`\p`, `\&`, bare `\z`, and overstruck glyphs. List cases assert that `Xo`
really belongs to `It HEAD`. Tests load the source, perform a real JSON string
roundtrip, and compare the rendered rows without collapsing whitespace.

The relevant native paths are `term.c::term_word`, `encode`, `encode1` and
`term_fill`; printable ASCII width follows `term_ascii.c::utf8_getwidth`.
Literal tabs and the pending-break HANG/TAG padding variants are excluded
from this exact-geometry fixture: their responsive projection is governed
by the existing tab and field layout contracts. Pending breaks remain covered
in literal text, and internal markers/zero-width graphs in all three contexts.
