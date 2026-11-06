# Deferred output ownership

`cases.json` contains 219 complete, distinct sources: the original 180-case
domain (69 known bad memberships and 111 controls) and 39 causal extensions
and controls. Each exact source ran pinned pristine ASCII, UTF-8, HTML, tree
and lint before its assertions were written. Source hashes, native profile
hashes and original operand byte/line/column witnesses are retained.

Native `term.c::term_word()` represents unknown SPECIAL/invalid NUMBERED
characters with `ASCII_NBRZW`, leaving BACKAFTER/BACKBEFORE unchanged.
`encode1()` decides the old glyph's survival. ManT independently preserves
supported recovery spellings in source order. HTML macro handlers identify
which source operands form a label; terminal formatting does not make
preceding recovered spelling part of it.

Expected labels, style and ordered recovery members come from those source
operands and native execution, not a candidate IR snapshot. `ordinal` counts
accepted occurrences of the same spelling; `source_ordinal` distinguishes
source members even when coordinates repeat during macro expansion. Equal
text or target URI does not merge owners. `reading_rows` is the selected
enhanced reading projection, retaining recovery beyond native output;
`native` records the independent raw device projection.

The D2 controls also assert 43 original native members, nine sibling/tail
members, explicit hardline witnesses and structured native diagnostics.
Ten controls without unknown recovery have complete independently derived
reading rows. A zero-length recovery inventory therefore cannot hide a lost
native glyph. Six additional bullet/dash/enum sources protect the real
pre-marker newline boundary rather than inventing an unreachable pending
recovery at marker entry.

`consumers.json` freezes 42 of those sources. Permanent tests exercise JSON
strings, both Markdown readers, both export options, two export/read rounds,
artifact byte maps, visible/raw search, response-local explain ranges, real
terminal buffers, App input and CLI output. Six selected inputs also run in
real Unix PTYs. Shared table-budget tests use four additional complete
sources with private small limits; normal CLI runs never prove exhaustion.

Portable literal/table fences flatten inner links and style; default export
may omit local-section wrappers. Those policies do not permit moving recovery
into an adjacent retained link. Original IR owners/styles are checked first.
Operand witnesses establish gold provenance, diagnostics retain native source
coordinates, and artifact/query positions refer to their own generated
Markdown. Each Markdown read has its own source. No new per-inline provenance
field or public formatter instruction is introduced.

Explain retains its finite name-boundary and complete-definition-form policy;
it is not substring search over definition terms. Tests distinguish legitimate
no-evidence labels from real literal payload mentions, resolve returned support
ranges and forbid invented direct names. Visual TUI copying keeps its existing
soft-row newlines and structural continuation padding; wide views also copy
complete source words. These policies do not relax scalar/target membership.
