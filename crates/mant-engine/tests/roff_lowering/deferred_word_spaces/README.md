# Deferred native word spaces

The 1,705 configurations contain 1,692 distinct sources. The fixture records exact
source bytes and hashes, the pristine reference identity, and status and stream
hashes for ASCII, UTF-8, tree, HTML and lint. Terminal probes use 160 columns.
Expected text uses the UTF-8 output after terminal overstrikes, removal of the
five-column page margin and exclusion of page furniture. The nine inset and
diag cases retain both the original rows and the corresponding projection with
generated separator cells represented by regular spaces.

| Cohort | Cases | Execution facts exercised |
| --- | ---: | --- |
| N1 | 720 | Separate No and Em TEXT visits and direct Lk operands, empty and nonempty targets, filled and literal flow, 0–2 preceding and following empties, five retirement boundaries and two label spellings |
| N1 same line | 12 | Direct operands and sibling TEXT nodes without intervening source lines |
| N1b field | 24 | TAG, HANG, inset, diag and column fields, including HEAD where the syntax supplies one |
| N1b link | 12 | Man UR and MT bodies with direct alternating-font operands and paragraph or line closes |
| N1b registers | 15 | Spacing off, KEEP, sentence spacing, continuation, zero advance and word-end breaks |
| N1b word boundaries | 756 | Zero-advance and continuation prefixes, pending word breaks, seven word forms, trailing empty operands and exact source-space followers |
| N1b KEEP | 162 | Same-line and separate TEXT operands around PREKEEP, completed zero-advance glyphs and backspace over KEEP padding |
| N1b partial | 4 | Accepted prefixes and rejected field tails |

The row assertions compare the full visible content after JSON transport.
Horizontal whitespace, interior empty rows, and each glyph are significant.
Tree receipts capture the AST independently of the projection. For example,
diag and column items do not use the extended HEAD arrangement in this
matrix. Separate consumer tests cover links, search and the reader buffer.
Raw terminal font controls and soft wrapping are outside these assertions.

Pinned CVS `term.c::term_word()` writes the automatic separator before
decoding its word. `encode1()` may retreat over a blank, and
`term_flushln()` consumes the accepted interval before restarting a pass.
A later owner cannot claim a deferred separator from an earlier word.
