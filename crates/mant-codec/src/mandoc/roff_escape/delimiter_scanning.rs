//! Fixed-CVS contracts for quoted escape-argument delimiters.
//!
//! Every expectation here was cross-checked against the pinned pristine
//! CVS mandoc (`roff_escape.c` 1.17, `term.c` `term_word()`) rendered with
//! `-Tutf8 -Owidth=78` over one-probe documents: the delimiter syntax, not
//! its displayed character, decides how an argument ends
//! (roff_escape.c:278-314 decides, :342-380 scans), a rejected delimiter
//! ends the argument right after itself (:292-299, :303-311), an unclosed
//! argument ends at its last proven-consumed unit (`iend`), and only the
//! `Aow` families keep an unclosed payload (:343-351).

use super::{decode, visible_text};

/// The RR05 main case: `\o` delimited by the complete `\(aq` escape.
#[test]
fn overstrike_with_escaped_delimiters_consumes_the_whole_escape() {
    // CVS renders `X\bY` (display `Y`) and keeps the tail; the control
    assert_eq!(visible_text(r"\o\(aqXY\(aq Z"), "Y Z");

    assert_eq!(visible_text(r"\o\(aqXY\(aqZ"), "YZ");
    assert_eq!(visible_text(r"\o\[aq]XY\[aq]Z"), "YZ");
}

/// The delimiter syntax decides the ending rule, not its displayed value.
#[test]
fn literal_and_escaped_delimiters_are_distinct_syntaxes() {
    // A literal delimiter is one raw character.
    for source in [r"\o'XY'Z", r"\o|XY|Z", r"\o!XY!Z"] {
        assert_eq!(visible_text(source), "YZ", "{source}");
    }
    // An escaped delimiter is identified by its trigger: `\(aq`, `\[aq]`,
    // `\e` (displayed backslash) and the UNDEF spelling `\q` all delimit,
    // each with its own ending rule.
    assert_eq!(visible_text(r"\o\(aqXY\(aqZ"), "YZ");
    assert_eq!(visible_text(r"\o\[aq]XY\[aq]Z"), "YZ");
    assert_eq!(visible_text(r"\o\eXY\eZ"), "YZ");
    assert_eq!(visible_text(r"\o\E'XY\E'Z"), "YZ");
    // UNDEF opening: the backslash is dropped and the trigger character
    // becomes a literal delimiter (roff_escape.c:289-290, 313-314).
    assert_eq!(visible_text(r"\o\qXYqZ"), "YZ");
    assert_eq!(visible_text(r"\o\\XY\\"), "Y\\");
}

/// Visually different delimiters share an ending rule when their trigger
/// matches, and visually equal ones do not when it differs.
#[test]
fn closing_matches_on_the_trigger_not_the_displayed_character() {
    // `\(dq` displays a double quote but closes like `\(aq`: both are
    // `(`-triggered (roff_escape.c:363-365 compares buf[snam]).
    assert_eq!(visible_text(r"\o\(aqXY\(dqZ"), "YZ");
    // `\[dq]` displays the same double quote but is `[`-triggered, so it
    // does not close a `(`-triggered argument: the input stays unclosed
    // and `\o` keeps its scanned payload.
    assert_eq!(visible_text(r"\o\(aqXY\[dq]"), "]");
    // A literal apostrophe is payload while the argument is escape-delimited.
    assert_eq!(visible_text(r"\o\(aqX'Y\(aqZ"), "YZ");
}

/// Nested escapes ride along inside the payload until a matching closer.
#[test]
fn nested_escapes_stay_in_the_payload_until_the_matching_closer() {
    // The whole nested `\N'39'` is payload (roff_escape.c:357-368 skips it,
    // the CVS overstrike loop renders its spelling as cells).
    assert_eq!(visible_text(r"\o\(aqB\N'39'B\(aqZ"), "BZ");
    assert_eq!(visible_text(r"\o'B\N'39'B'Z"), "BZ");
    // A nested `\o` with its own literal delimiter does not close an
    // escape-delimited outer argument.
    assert_eq!(visible_text(r"\o\(aqB\o'XY'B\(aqZ"), "BZ");
    assert_eq!(visible_text(r"\o\(aqX\N'8'Y\(aqZ"), "YZ");
}

/// Every quoted family consumes an escaped delimiter without leaking.
#[test]
fn every_quoted_family_supports_escaped_delimiters() {
    // `\C` names a character with the payload.
    assert_eq!(visible_text(r"A\C\(aqaq\(aqB"), "A'B");
    assert_eq!(visible_text(r"A\C\[aq]aq\[aq]B"), "A'B");
    // Postprocessor and motion families render nothing on this device but
    // must still consume their whole argument (CVS ESCAPE_IGNORE buffers
    // ASCII_NBRZW only).
    for trigger in ['Z', 'b', 'D', 'R', 'H', 'L', 'S', 'v', 'x'] {
        let source = format!(r"A\{trigger}\(aqXY\(aqB");
        assert_eq!(visible_text(&source), "AB", "{source}");
    }
    // `\X` keeps its postprocessor command semantics with an escaped
    // delimiter.
    assert_eq!(
        decode(r"\X\(aqtty: link https://example.test\(aq"),
        vec![super::RoffInlineEvent::Link(Some(
            "https://example.test".to_owned()
        ))]
    );
    // `\h` keeps its positive-advance word boundary.
    assert_eq!(visible_text(r"A\h\(aq5n\(aqB"), "A B");
    // `\N` takes its number from the payload.
    assert_eq!(visible_text(r"A\N\(aq65\(aqB"), "AAB");
}

/// CVS rejects delimiters that cannot delimit anything (`ESCAPE_DELIM)`:
/// the family renders nothing and the argument ends right after the
/// rejected delimiter (roff_escape.c:292-299, 303-311).
#[test]
fn rejected_delimiters_end_the_argument_and_keep_the_tail() {
    // Literal rejections: `\v(`, `\h `, `\D(` cannot open an argument.
    assert_eq!(visible_text(r"A\v(5)v B"), "A5)v B");
    assert_eq!(visible_text(r"A\h 5n B"), "A5n B");
    // `\D` only warns upstream and keeps scanning, so the rest is consumed
    // by its (unclosed) argument.
    assert_eq!(visible_text(r"A\D(l 1i)B"), "A");
    // Escaped rejections: `\h\v` and `\N\v` are non-printing delimiter
    // names for families that reject them.
    assert_eq!(visible_text(r"A\h\v5n\vB"), "AnB");
    // `\o` never rejects: `(` delimits an overstrike whose payload keeps
    // scanning to the end of input.
    assert_eq!(visible_text(r"A\o(5)B"), "AB");
}

/// An unclosed argument ends at its last proven-consumed unit (CVS `iend`),
/// so a delimiter that never received payload returns to the text.
#[test]
fn unclosed_arguments_release_their_unconsumed_delimiter() {
    assert_eq!(visible_text(r"A\vB"), "AB");
    assert_eq!(visible_text(r"A\o'"), "A'");
    assert_eq!(visible_text(r"A\o\(aq"), "A'");
    assert_eq!(visible_text(r"A\h'"), "A'");
    // `\o` is an `Aow` family: the scanned payload survives without a
    // closer (roff_escape.c:349), and nothing is rendered twice.
    assert_eq!(visible_text(r"A\o'XY"), "AY");
    assert_eq!(visible_text(r"A\o\(aqXY"), "AY");
    // Other families drop the unclosed payload entirely, but every
    // scanned unit stays consumed (CVS `iend`), so nothing leaks back.
    assert_eq!(visible_text(r"A\C'aqB"), "A");
    assert_eq!(visible_text(r"A\h'5nB"), "A");
    assert_eq!(visible_text(r"A\Z'XB"), "A");
}

/// Empty closed arguments and missing arguments render nothing.
#[test]
fn empty_and_missing_arguments_are_inert() {
    assert_eq!(visible_text(r"A\o''B"), "AB");
    assert_eq!(visible_text(r"A\h''B"), "AB");
    assert_eq!(visible_text(r"A\Z''B"), "AB");
    // `\C''` is a closed but empty name: CVS rejects it (ESCAPE_BADCHAR),
    // so no fallback spelling may appear.
    assert_eq!(visible_text(r"A\C''B"), "AB");
    // No delimiter at all: only the trigger is consumed and nothing
    // follows, so the family renders nothing.
    assert_eq!(visible_text(r"A\oB"), "AB");
    assert_eq!(visible_text(r"A\C"), "A");
}

/// `\N` ends at its first non-digit, and a non-printing escaped delimiter
/// is rejected; unaccepted numbered spellings keep `ManT`'s established
/// visible recovery design.
#[test]
fn numbered_arguments_end_at_the_first_non_digit() {
    // Closed by the digit rule: payload "6" renders the C0 replacement
    // glyph and the remainder stays text.
    assert_eq!(visible_text(r"A\N'6X5'B"), "A\u{fffd}5'B");
    assert_eq!(visible_text(r"A\N\(aq6X5\(aqB"), "A\u{fffd}5'B");
    // Escaped-delimited `\N` applies the digit rule to "5" too: the ENQ
    // control renders as the replacement glyph, the remainder stays text.

    assert_eq!(visible_text(r"A\N\(aq5nX\(aqB"), "A\u{fffd}X'B");
    // ManT keeps the unaccepted spelling visible (recovery design).
    assert_eq!(visible_text(r"A\N\v65\vB"), r"A\N\v65B");
}

/// Sizes have no escaped-delimiter syntax upstream: `\s'...'` is a fixed
/// literal quote, and a backslash falls back to one counted unit.
#[test]
fn size_arguments_have_no_escaped_delimiter_syntax() {
    // The `\s` shape switch takes one unit with CVS maxl counting, so the
    // nested `\(aq` is consumed whole without counting and the size ends
    // after the next unit; the remainder is ordinary text.
    assert_eq!(visible_text(r"A\s\(aq10\(aqB"), "A0'B");
    assert_eq!(visible_text(r"A\s+\(aq10\(aqB"), "A0'B");
    // Counted names consume nested escapes whole without counting them.
    assert_eq!(visible_text(r"A\(\e'XYB"), r"A\(\e'XYB");
}

/// Depth, adversarial nesting and many-escape lines stay bounded and make
/// monotonic progress.
#[test]
fn delimiter_scanning_stays_bounded_under_adversarial_nesting() {
    // Far beyond the nesting budget: behave like an unclosed argument that
    // consumes the remainder (only `\o` keeps its payload).
    let deep = format!("{}Z", r"\o".repeat(5_000));
    assert_eq!(visible_text(&deep), "");
    let deep_unclosed = format!("{}XY", r"\o'".repeat(100_000));
    assert_eq!(visible_text(&deep_unclosed), "Y");
    // A long line of complete escaped-delimiter overstrikes decodes fully.
    let many = r"\o\(aqXY\(aq".repeat(20_000);
    assert_eq!(visible_text(&many), "Y".repeat(20_000));
    // Deep escaped nesting still terminates through the shared budget.
    let nested = format!(r"{}\(aqZ", r"\o".repeat(1_000));
    assert_eq!(visible_text(&nested), "");
}
