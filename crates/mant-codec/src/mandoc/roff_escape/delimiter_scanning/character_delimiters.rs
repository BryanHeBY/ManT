//! CVS 1.18's complete-character delimiter rule and its recovery boundaries.
//!
//! Every exact one-probe source was run with the active pristine reference
//! before these golds: `.TH TEST 1 "2026-10-13"`, `.SH DESCRIPTION`, source.
//! `roff_escape.c` compares normalized snam..send with dnam..iarg only for
//! SPECIAL; ERROR, UNICODE and other classes keep same-trigger termination.
//! `term.c::term_word()` projects the overstrike payload's final native cell.

use super::visible_text;
use crate::mandoc::roff_escape::scanner::{QuotedOutcome, scan_quoted_argument};

#[test]
fn special_delimiters_keep_unequal_spellings_in_the_payload() {
    for (source, expected) in [
        (r"A\o\(aqXY\(dqZ", "AZ"),
        (r"A\o\[aq]XY\[dq]Z", "AZ"),
        (r"A\o\(aqXY\[aq]Z", "AZ"),
        (r"A\o\[aq]XY\(aqZ", "AZ"),
        (r"A\o\C'aq'XY\C'dq'Z", "AZ"),
        (r"A\o\C|aq|XY\C!aq!Z", "AZ"),
        (r"A\o\C'aq'XY\C'aq'Z", "AYZ"),
        (r"A\o\C|aq|XY\C|aq|Z", "AYZ"),
        (r"A\o\E(aqXY\(aqZ", "AYZ"),
        (r"A\o\(aqXY\E(aqZ", "AYZ"),
        (r"A\o\E[aq]XY\[dq]Z", "AZ"),
        (r"A\C\(aqaq\(dqB", "A"),
        (r"A\C\[aq]aq\[dq]B", "A"),
        (r"A\h\(aq5n\(dqB", "A"),
        (r"A\h\[aq]5n\[dq]B", "A"),
    ] {
        assert_eq!(visible_text(source), expected, "{source}");
    }
}

#[test]
fn a_later_equal_delimiter_closes_after_the_whole_unequal_escape() {
    for (source, payload) in [
        (r"A\o\(aqXY\(dqQ\(aqZ", r"XY\(dqQ"),
        (r"A\o\[aq]XY\[dq]Q\[aq]Z", r"XY\[dq]Q"),
        (r"A\o\C|aq|XY\C!aq!Q\C|aq|Z", r"XY\C!aq!Q"),
    ] {
        assert_eq!(visible_text(source), "AQZ", "{source}");
        let characters: Vec<char> = source.chars().collect();
        let (outcome, end, exhausted) = scan_quoted_argument(&characters, 3, 'o');
        assert!(!exhausted, "{source}");
        let QuotedOutcome::Closed { payload: range } = outcome else {
            panic!("complete native delimiter must close: {source}: {outcome:?}");
        };
        assert_eq!(characters[range].iter().collect::<String>(), payload);
        assert_eq!(characters[end..].iter().collect::<String>(), "Z");
    }
}

#[test]
fn completed_nested_class_preserves_non_special_trigger_recovery() {
    for (source, expected) in [
        (r"A\o\fRXY\fBZ", "AYZ"),
        (r"A\o\fRXY\f[CB]Z", "AYZ"),
        (r"A\o\[aq]XY\[]Z", "AYZ"),
        (r"A\o\[aq]XY\[x]Z", "AYZ"),
        (r"A\o\[aq]XY\[u0041]Z", "AYZ"),
        (r"A\o\C'aq'XY\C'u0041'Z", "AYZ"),
        (r"A\o\C'aq'XY\C'x'Z", "AYZ"),
        // Invalid Unicode stays SPECIAL; a diagnostic does not turn it
        // into the non-SPECIAL class used by the delimiter decision.
        (r"A\o\[aq]XY\[uD800]Z", "AZ"),
        (r"A\o\[aq]XY\[u0000]Z", "AYZ"),
        (r"A\o\[aq]XY\[u1F600]Z", "AYZ"),
        (r"A\o\[aq]XY\[u10FFFF]Z", "AYZ"),
        (r"A\o\[aq]XY\[u0abcd]Z", "AZ"),
        (r"A\o\[aq]XY\[u11FFFF]Z", "AZ"),
        (r"A\o\[aq]XY\[u10fffG]Z", "AZ"),
        (r"A\o\[aq]XY\[uα]Z", "AZ"),
        (r"A\o\[aq]XY\[uαβ]Z", "AZ"),
    ] {
        assert_eq!(visible_text(source), expected, "{source}");
    }
}

#[test]
fn char_descriptors_use_the_pinned_postprocessing_class() {
    // roff_escape.c's charNN loop starts at iarg, including the "char"
    // prefix. All admitted 6/7-byte values fail its range check and retain
    // SPECIAL; malformed lengths also retain SPECIAL. These 14 exact
    // reference probes reject the tempting digit-only NUMBERED inference.
    for name in [
        "char33", "char65", "char99", "char160", "char255", "char256", "char9", "char0065",
    ] {
        let source = format!(r"A\o\[aq]XY\[{name}]Z");
        assert_eq!(visible_text(&source), "AZ", "{source}");
    }
    for name in [
        "char33", "char65", "char99", "char160", "char255", "char256",
    ] {
        let source = format!(r"A\o\C'aq'XY\C'{name}'Z");
        assert_eq!(visible_text(&source), "AZ", "{source}");
    }
}

#[test]
fn ignored_quoted_families_share_the_complete_delimiter_consumption() {
    // Exact 18 native sources retain AB: unequal complete SPECIAL is a
    // payload escape, and the later exact delimiter closes the ignored word.
    for outer in ['Z', 'b', 'D', 'R', 'H', 'L', 'S', 'v', 'x'] {
        for (opening, other, closing) in
            [(r"\(aq", r"\(dq", r"\(aq"), (r"\[aq]", r"\[dq]", r"\[aq]")]
        {
            let source = format!(r"A\{outer}{opening}XY{other}Q{closing}B");
            assert_eq!(visible_text(&source), "AB", "{source}");
        }
    }
}
