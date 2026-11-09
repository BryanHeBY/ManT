# Grouped formatter rows

The 1,230 configurations cover 1,184 distinct inputs. They record the
DESCRIPTION rows of exact inputs run through the pinned pristine CVS
reference. All sources ran ASCII and UTF-8 at 160 columns,
HTML, tree and lint. The row comparison removes native page furniture, terminal
overstrikes and horizontal whitespace at row edges. It retains every physical
row, full word, and interior space. The focused roff fixtures additionally
check AST operand ownership and exact authored TAB through JSON and Markdown.

| Cohort | Cases | Coverage |
| --- | ---: | --- |
| Grouped | 400 | man and mdoc, filled and no-fill, ce and rj, count 1/2, zero advance and blank operands, leading SP/TAB, conditional and inline-scope contrasts |
| Ordinary | 70 | Native no-fill TEXT and control boundaries without grouped dispatch |
| Extensions | 640 | Empty and mixed TAB/SP, empty conditional flush, embedded br/sp 0/fi/nf, 0–3 groups at start/middle/end |
| Additional controls | 120 | Embedded sp 1/sp -1/ti after blank, zero-width and zero-advance operands |

The two annotated mdoc cases apply the existing public reading policy that
omits a single initial blank before literal content. The native rows remain
recorded alongside the exact source. Every interior or trailing blank is
compared without that allowance.
