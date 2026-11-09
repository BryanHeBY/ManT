# Shared source-line execution witnesses

`cases.json.gz` contains 4,189 matrix members with 4,177 distinct byte inputs.
Each member identifies its axes, complete source, SHA-256, five profile
receipts and ordered native UTF-8 marker rows. The reference was the
registered pristine CVS 20261013T025419Z at width 160. ASCII, UTF-8 and
tree succeeded on all members. Lint statuses are retained independently.
The sixteen failed pristine HTML receipts are from the independently
repaired held-tag path. Native tree and terminal supply their evidence.

| Group | Frozen members and scope |
| --- | --- |
| M0 | 768 man/mdoc × filled/no-fill × eight input spellings × six incoming word states × four followers |
| M1 | 360 scope cases and 24 nested Bo/Xo/Bf with Po cases |
| M2 | 1,320 body, link and HEAD cases, eight immediate HEAD/BODY controls and sixteen nested list/link-label cases |
| M3 | 126 hidden, empty and control cases |
| M4 | 432 expansion cases, 512 previous general controls, the previous 336 and 24 followups, and 32 LF/CRLF macro-definition and append controls |
| M5 | Two displays, two heading cases and three tbl controls |
| M6 | Eight formula/source-line seams, supplementing the existing 720 basic, 118 family and 54 quote witnesses |
| M7 | 216 direct and grouped operands, including the ordinary child-visiting In control |

The generated M1–M7 extensions select distinct execution paths in each
family. They omit Cartesian repeats where the relevant input never gains
NODE_LINE or the same field handler is already covered by the parent matrix.

The Rust test checks relative physical rows, including intervening empty rows,
and marker sequence and multiplicity after an actual JSON round trip. M0
also checks accepted characters between the first and last markers, removing
portable horizontal spacing cells for that character comparison only. The
adjacent consumer and real-manual tests cover exact SP and TAB spelling, the
three-space tag HEAD gap, source flags, term and body ownership, links,
plain and ANSI text, two rounds of Markdown, and TUI cells, clicks and copy.

Two T{} controls store both native and projected rows. The CVS tree exposes a
single flattened cell string and `tbl_term.c::tbl_word()` executes that
operand directly. The fixed device wraps it at its measured column width.
The shared IR keeps the cell word intact for viewport layout. Both output
expectations retain the same marker sequence. No other fixture rows have a
projection override.

The previous 336-member subset retains original observations for its 42
failures. The classifier recognizes 36 extra hard rows and six missing hard
rows, rather than grouping both under a TAB predicate.

Replay the frozen exact sources with the registered binary to verify the
receipts and save raw outputs:

```sh
ulimit -c 0
python3 -m scripts.roff.fixtures.record_shared_source_lines \
  --evidence target/audits/shared-source-line-oracle.jsonl
```
