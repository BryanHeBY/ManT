# Native enclosure word operands

This repository-authored matrix covers native word identity before projection.
It preserves the global word-register and field contracts established by the
existing `source_sequences`, `word_spacing`, `spacing_execution` and synopsis
matrices; it does not replace their fixtures or rewrite their expectations.

| Family | Configurations | Axes |
| --- | ---: | --- |
| Cursor regression core | 64 | Empty Eo / Es opening, closing or both NBRZW × off→on/toggle × prose/KEEP/filled display/tag BODY × Text/Link |
| Direct operands and delimiter parsing | 54 | Eight raw Empty/NBRZW/font/zero/cached glyph/break/continuation/ordinary-punctuation values × opening/closing/both × filled/no-fill, plus six delimiter-punctuation parsing controls |
| Structural risks | 50 | Absent vs present-empty parts, immediate end marker, inline En, font/link and ordinary empty-word controls across five owners |
| Opposite controls | 24 | Empty no-fill, bare/cached zero, previous glyph/continuation/font, on→off and repeated off in both source modes |

The 192 configurations contain 185 unique full sources and 960 actual native
profile invocations. Duplicate sources from intentionally overlapping risk
axes are counted separately from unique inputs. Every exact source first ran
the locked pristine ASCII, UTF-8, HTML, tree and lint profiles. The recorder
never calls a ManT candidate. It retains source hashes, raw output hashes,
statuses and diagnostics independently. Reproduce them with:

```sh
python3 -m scripts.roff.fixtures.record_enclosure_operands \
  --evidence target/audits/enclosure-operands
python3 -m unittest scripts.roff.tests.test_enclosure_operands
```

Only `--write` changes this new fixture. Ordinary Eo sources are clean. Es/En
are supported obsolete syntax with lint 2; crossed end-body risk inputs have
their own nesting warnings. Neither warning class is presented as clean.

## Native execution rules

`mdoc_term.c::termp_eo_pre/post` uses actual child presence and end markers.
The fully absent form writes `term_word("\\&")`, including the immediate
end-body pre branch; this is an invisible native input cell, not an empty
operand. `termp_quote_pre/post` executes the normalized Es operands with their
escapes at the current En entry. The Es definition's source node is not
replayed. Missing closing and present-empty closing remain distinct.

`term.c::term_word` consumes its incoming boundary and changes NOSPACE even
for an empty operand, while its automatic separator is a separate cell.
NBRZW is zero width but participates in the native graph/buffer rules. Bare
zero, cached glyph, font changes, word-end break and continuation retain the
same shared text executor as ordinary source words. `term_newln` only closes
an occupied native buffer; a zero-cell word cannot manufacture a literal row.

## Consumer assertions

Rows remove the single native device origin proven by ORIGIN. No per-row trim
or whitespace folding erases the target leading space. Font-bit receipts are
recorded per scalar for real terminal-cell checks. JSON strings, IR validation,
CLI exact rows and two real Markdown reads use independent native expectations.
The shared typed fixture helper also supplies actual 40/78-column UI checks.

Portable Markdown has two explicit structural layout reductions. A compact
filled display is a separate Paragraph owner: its export uses ordinary
CommonMark paragraph separators before/after it. A definition exports as an
ordinary bullet list: its six-column native field origin becomes structural
list indentation, and its following outer paragraph requires a block-closing
separator. These **ExplainedDifference** axes do not relax operand SP, hard-row,
empty-row, font or link assertions. The first native/JSON/CLI observation uses
every exact reference row; portable expectations separately add the known
paragraph/list gaps and remove only the fixed six-column field origin. The
BODY's independently executed leading word cell remains. A clone is used to
observe list content without its marker; the actual read IR, including its
list, is kept intact for the second export. Native widths and viewport soft
wrapping are not confused with hard source rows. Owner/source, links, search and copy are
checked on their original accepted content, rather than on generated Markdown
syntax. Finite matrix coverage does not claim all roff programs are correct.

The six `operand-punctuation-*` sources deliberately retain the original
`[` spelling. Pinned `mdoc_macro.c::in_line_argn` extracts it as an opening
delimiter sibling rather than an Es child. Their AST assertions prove the
remaining absent/present-empty normalized operand identities and one/two
original delimiter nodes. They do not claim that `[` entered the raw Es
executor. All original CLI/JSON/Markdown/UI text and row checks still run for
those sources; no rendering failure is hidden by this parser classification.

Six additional `operand-ordinary-punctuation-*` sources use `@`, which the
five independently recorded tree profiles prove remains an actual normalized
Es operand on opening, closing or both sides. These supplement, rather than
replace, the original `[` parser-delimiter controls. Their exact native
filled/no-fill rows, raw identity, JSON and consumer assertions all apply.
