# ManT-authored audit fixtures

These small roff inputs are authored by the ManT project and distributed under
the repository's Apache-2.0 license. Unlike the adjacent attributed upstream
manuals, they exist to exercise combinations that a structural oracle must see
on every local fixture run. They are not copied from an operating system or
third-party manual.

`equation-contexts.7` combines a configuration-only `.EQ`, inline delimited
equations, a display equation, and delimiter-driven tbl cells. Focused Rust
tests remain the behavioral gate; the fixture proves that the corpus profiler
itself observes each placement class.

`projection-escapes.7` locks source entity spellings, trailing brace text in a
heading, and dollar-prefixed variable text across the native-IR-to-CommonMark
boundary.

`tq-aliases.7` and `macro-recursion.7` retain reproducible evidence for two
source-specific cases where ManT preserves more programmatic semantics than
GNU groff's terminal result: alias ownership and the complete finite document
around a recursively defined macro.

## Formatter consumer regressions

The complete `lowering-*.1` inputs are consumed by the source-tree integration
test `crates/mant-engine/tests/roff_consumers.rs`, not packaged unit tests.
They are original Apache-2.0 probes. Fixed behavior follows mandoc renderer
`mdoc_term.c` 1.388 / `term.c` 1.295, except ordinary filled text under `Sm off`
retains source word boundaries (the mdoc manual and groff 1.24.1 policy).
The product parser remains the pinned libmandoc; Cargo tests need no host oracle.

| Files (`lowering-` prefix, `.1` suffix) | Contract | Integration assertion |
| --- | --- | --- |
| bk-table, enclosure-table | R01/B06 | nested_wrapper_payloads: every cell and table topology |
| enclosure-font | R02/B02 | enclosure_font_scope: effective inner/outer styles |
| display-spacing | R03/B05 | display_final_spacing: post-list state |
| lk-label-state, lk-uri-state, lk-order, mt-state, in-state | R04/B03 | link_and_include_font_state: current/previous effects |
| fa-delimiter, fa-prose | R05/B04 | function_logical_adjacency: exact punctuation |
| sm-plain-lines | R06/B05 | plain_text_lines_keep_word_boundaries |
| literal-function, literal-enclosure | R07/B07 | literal_macro_descendants_keep_source_lines: exact lines and style |

The R06 result deliberately differs from mandoc terminal output; R07 differs
from groff's collapsed function layout. Do not accept either oracle at random
or flatten whitespace before checking these contracts.
