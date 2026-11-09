# Accepted word receipts and deferred hard rows

The fixture has 758 members and 754 unique source byte strings. Each member
binds its source hash, pristine identity, UTF-8 rows, text-node witnesses,
and status and stream hashes for ASCII, UTF-8, tree, HTML and lint. The
terminal profiles run at 160 columns. All profiles set the footer OS to
`ManT` for a reproducible invocation. The 185 sources containing raw TAB
in quoted operands return lint status 2. The other sources return status 0.

| Group | Members | Coverage |
| --- | ---: | --- |
| W1 | 630 | Full cross of seven prior states, zero to two empty words, six successors and five retirement paths |
| W1 control | 20 | Zero-width and breakpoint controls, font changes, word breaks and multiple source blanks before the first graph |
| W2 | 23 | Restarted words with an older held prefix, optional empty or font-only operands and optional final empty operand, including five field bodies |
| W3 | 45 | Plain and delayed glyphs, trailing SP/TAB, zero width, repeated markers and rejected suffixes through EOF, generated brackets and real line flushes |
| W4 | 19 | Em, Li, Lk, BR, BI, UR, MT, In, KEEP, Sm and literal mode, with tag, hang, inset, diag and column bodies |
| W5 | 5 | Same-line and same-coordinate word visits, the raw source-space control, and link ownership with and without rearming zero advance |
| W6 | 13 | EOF with or without a late glyph, rejected pass tails and zero through three authored initial or final literal rows |
| Consumer | 3 | Ordinary quoted-space and deferred-row minima and a wide/combining-character link |

Tree witnesses preserve each TEXT node's actual parent macro and node kind,
line, column, spelling and `NODE_LINE` and `NODE_NOFILL` flags. A raw-space
input line and a quoted macro operand can produce identical body text but
have different witnesses. Same-coordinate macro expansions still have
separate nodes.

The row oracle decodes terminal overstrikes after removing exactly the known
five-column generated page margin. A backspace targeting that generated
margin does not remove a source column. One footer or section separator is
page furniture. EOF checks keep preceding authored empty rows. The single
initial literal-row omission is explicitly identified and counted.

The filled TAB cases compare exact glyphs, hard rows, and all spacing outside
the source's tab-stop interval immediately before its successor. The
original TAB is separately verified in the tree. The field witnesses allow
generated run-in spacing after HEAD to appear as regular SP rather than
NBSP. They still assert its width and preserve every authored SP/NBSP inside
the word range. These are comparisons of accepted content and row ownership,
with TAB stops, terminal font overstrike bytes, and absolute page margins
outside the projection contract.

The ordinary group exercises the complete three-stage interaction. Wrapper
and field groups use representative distinct dispatches. Repeating every
plain cross under each wrapper would not add new native word-entry states.
The three consumer sources run through the actual CLI text/ANSI path and
two Markdown exports and readbacks. The addressable checks cover the
DESCRIPTION body. Earlier raw HTML anchors follow the existing visible-source
fallback contract. At 40 and 78 reader columns the same sources verify
physical rows, link activation, search and copy with terminal cell widths.

Replay the frozen receipts and retain exact process output when needed.

```sh
python -m scripts.roff.fixtures.record_word_receipts \
  --evidence target/audits/word-receipt-repair/oracle.jsonl
```
