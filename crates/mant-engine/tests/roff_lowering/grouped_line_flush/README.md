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

## Alignment and hanging paragraphs

`alignment.json.gz` binds 1,504 distinct sources to five pristine CVS
profiles. Its 1,344-case man product crosses center and right alignment,
default and explicit 0/1/2 counts, filled and no-fill input, plain and hanging
contexts (with and without `RS`), widths 0/4/16, ordinary/empty/zero-advance/
word-break payloads and ordinary text/`br`/`PP` tails. It adds 112 embedded
`nf`, `fi`, `ti` and `br` cases and 48 mdoc owner/link contrasts. The
captured tree records when a source blank is a grouped request or when a
zero-count alignment request owns no TEXT at all. The test checks actual
parsed operand counts and fill mode against that witness.

The ASCII and UTF-8 receipts retain the native 160-column positions. Body
comparisons exclude only generated leading columns and a single section
separator. Portable Markdown keeps the hard rows on two real import/export
cycles in both profiles, with the exact heading-ID marker separated from the
addressable body comparison. Markdown retains list and fenced-table framing.
The two focused fixtures check public hanging geometry in one paragraph
after JSON and at three TUI widths, including soft continuation at 20 columns.

The existing short filled HP field behavior is a layout difference: if the
native first row remains open, ManT keeps the authored break at alignment
entry. Eight wide-field, zero-count, bare-zero-advance cases followed by
`br` or `PP` also differ in one glyph on the 8cd01383 baseline. Those glyphs
remain open for review and have no accepted portable replacement in the
matrix. Seventy filled cases with an empty aligned tail and `PP` compare
Markdown's single paragraph gap separately from native physical rows.

Re-record or verify with `python3 -m scripts.roff.fixtures.record_hanging_alignment`
and its `--check` option. The recorder verifies the pinned reference identity
before generating receipts and never reads product output.
