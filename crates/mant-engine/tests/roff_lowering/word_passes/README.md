# Automatic separators across ordered marker passes

The fixture retains the exact 768-source successor cross and 324-source
wrapper cross from the 218edc22 review, plus 31 focused witnesses for
native reading and actual Markdown readback. The crosses cover repeated markers, a rearmed
zero-advance glyph, spacing changes, empty words, EOF, and typed links.
The terminal comparison preserves the complete rows and link-target
punctuation. It removes only the known document furniture and page
margin, then resolves the ASCII overstrikes in these sources.

All 1,123 sources have native tree word visits and five profile receipts
from pinned CVS. The 192 sources containing the `ALPHA\z.` operand carry
the native trailing-delimiter style diagnostic. They still render and are
included in the exact row assertions. The other 931 sources are lint-clean.

`term_word()` writes the automatic separator once. `term_field()` delays
its advance until an output character, so a committed pass can retire it
without printing it. Another marker or newly cached glyph cannot replay
that separator. The named sources also cover leading source spaces after
an already projected pass, a surviving automatic separator, and rejected
tails that complete a hard row before a positive gap or EOF.
Deferred source-space controls check plain, styled and linked output,
including an advance overwritten by a native backspace.

```sh
python -m scripts.roff.fixtures.record_word_passes
cargo test --locked -p mant-engine --all-features --test roff_lowering word_receipts::
cargo test --locked -p mant --test process marker_passes_keep_only_printed_separators
```
