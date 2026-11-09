# Native word projection and separator ownership

This matrix has 2,470 distinct roff sources and 12,350 pristine process
receipts. Each source stores its exact bytes and hash, native TEXT and operand
visits, UTF-8 rows and status/stdout/stderr hashes for ASCII, UTF-8, tree, HTML
and lint. The terminal profiles run at 160 columns. The 201 raw TAB inputs
return lint status 2. Four control-only suffixes ending in a breakpoint return
status 1 for the trailing delimiter style rule. The other 2,265 return 0.

| Cohort | Sources | Coverage |
| --- | ---: | --- |
| Core | 1,638 | Four prior glyph/arm states × twelve controls × seven source and word seams × five retirement paths, deduplicated by exact source |
| Reported prefixes | 576 | Three prior states × optional intermediate empty word × sixteen prefixes × No/Em/Lk × optional trailing empty word |
| Scopes and fields | 63 | No/Em/Li/Lk, same-word and same-macro literal, separate literal rows, KEEP, Sm, tag/hang/inset/diag/column BODY and actual extended tag/hang HEAD |
| Separators | 168 | Marker ordering, authored SP/TAB/NBSP, controls, terminal EOS, accepted and rejected successors |
| Tight empty words | 18 | Breakpoint, zero-width graph or word-end marker × one, two or three empty words × filled or literal, followed by Ns |
| Consumer fixtures | 7 | Three minimal row projections and four visible link ranges |

Different AST parents, flags and source coordinates remain part of the
witness, even when the text is identical. The actual HEAD probes use an
extended Xo scope. `-diag` is tested at BODY because its label parses Xo as
literal text. Rejection is exercised in the separator suffix cohort. The
ordinary retirement cross has no valid substitute for a field rejection.

All 2,248 exact inputs compare complete rows after the same terminal
overstrike and furniture decoding as the existing word-receipt matrix. The
201 device-tab inputs allow only the tab-stop interval immediately before the
successor to vary. The 21 native-field inputs exclude absolute column origins
and the generated HEAD/BODY field gap. They retain the gap's presence and
compare the authored word sequence, separators, hard rows and blank rows.
The source witnesses retain every raw tab and field operand.

The seven consumer fixtures run through the CLI in plain and ANSI, JSON link
label/target inspection where applicable, and two default and two addressable
Markdown exports with actual readback. The same sources run through the TUI
after JSON round-trip and after both Markdown readbacks at 32, 40 and 78
columns. The click and copy probes cover all authored label cells, including
leading blanks, wide characters and combining marks. Adjacent generated blanks
and the generated colon lie outside the hit range.

Replay the frozen pristine receipts and optionally save full process output.

```sh
python -m scripts.roff.fixtures.record_word_projection \
  --evidence target/audits/repair-word-projection/consumers/oracle.jsonl
cargo test --locked -p mant-engine --all-features --test roff_lowering word_receipts
```
