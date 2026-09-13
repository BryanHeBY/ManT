use super::*;

#[test]
fn formatter_boundaries_precede_pending_zero_advance_glyphs() {
    // CVS `term_word()` buffers each word boundary before decoding `\zX`;
    // BACKBEFORE then lets X overwrite the following word's boundary.  The
    // ordering is the same for an explicit empty word, `\&`, and the fixed
    // blank glyph `\0`, with or without `.mc` settling the previous field.
    for predecessor in [r#""""#, r"\&", r"\0", r"\~"] {
        for zero_expression in [r"\zX", r"\p\zX", r"\zX\p"] {
            for margin_request in ["", ".mc\n"] {
                let source = format!(
                    ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No A\n.No {predecessor}\n{margin_request}.No {zero_expression}\n.No BODY\n"
                );
                let native = without_line_indentation(&native_terminal(&source));
                assert!(
                    native.contains("XBODY"),
                    "{predecessor:?}/{zero_expression:?}/{margin_request:?} native output: {native:?}"
                );
                let lowered = lowered_terminal(&source);
                assert!(
                    lowered.contains("XBODY"),
                    "{predecessor:?}/{zero_expression:?}/{margin_request:?} lowered output: {lowered:?}"
                );
                assert!(
                    !lowered.contains("X BODY"),
                    "boundary moved after pending glyph: {lowered:?}"
                );
            }
        }
    }
}
