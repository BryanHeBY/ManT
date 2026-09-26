//! Whole-label names for an independently established definition owner.
//!
//! These spellings do not create an owner. PP/RS candidates need stronger
//! structure and declaration evidence; TP/It definitions can bind a complete
//! surviving technical label without guessing aliases from its words.

use std::ops::Range;

/// Exact callable name interval in a complete `name(section)` manual label.
/// This is a ManT declaration rule, not an inferred native hyperlink.
#[doc(hidden)]
#[must_use]
pub fn manual_call_name_range(text: &str) -> Option<Range<usize>> {
    let start = text.len() - text.trim_start().len();
    let label = text.trim();
    let (name, section) = label.strip_suffix(')')?.rsplit_once('(')?;
    if name.contains(['(', ')'])
        || !crate::is_inferred_manual_topic(name)
        || section == "0"
        || !crate::is_manual_section(section)
    {
        return None;
    }
    Some(start..start.checked_add(name.len())?)
}

/// A pure list marker, not a semantic subject even in a definition-shaped
/// physical owner. The ordinal grammar is shared with Flow's existing rule.
#[doc(hidden)]
#[must_use]
pub fn is_presentation_term(text: &str) -> bool {
    let label = text.trim();
    is_ordinal_marker(label) || matches!(label, "•" | "∙" | "*" | "-" | "–" | "—")
}

/// Original Flow ordinal marker grammar, shared with Fixed read-time proof.
#[doc(hidden)]
#[must_use]
pub fn is_ordinal_marker(value: &str) -> bool {
    let value = value.trim();
    let digits = if let Some(digits) = value.strip_suffix('.') {
        Some(digits)
    } else if let Some(digits) = value.strip_suffix(')') {
        Some(digits.strip_prefix('(').unwrap_or(digits))
    } else {
        value
            .strip_prefix('[')
            .and_then(|digits| digits.strip_suffix(']'))
    };
    digits.is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
    })
}

/// Bind the complete visible spelling of a real definition as one Term name.
/// Punctuation and spaces inside a diagnostic or glossary subject remain part
/// of that name; neither supplies extra aliases or partial word selectors.
#[doc(hidden)]
#[must_use]
pub fn complete_term_label_range(text: &str) -> Option<Range<usize>> {
    let start = text.len() - text.trim_start().len();
    let label = text.trim();
    if label.is_empty() || label.contains(['\r', '\n']) || is_presentation_term(label) {
        return None;
    }
    Some(start..start.checked_add(label.len())?)
}

#[cfg(test)]
mod tests {
    use super::{complete_term_label_range, is_presentation_term, manual_call_name_range};

    #[test]
    fn manual_call_and_full_term_are_distinct_name_proofs() {
        // Exact E03/E03b/E04 inputs first ran pinned CVS -Tutf8; in E03b
        // man_html.c::man_PP_pre/man_RS_pre emitted only paragraph/div markup.
        assert_eq!(manual_call_name_range(" git-add(1) "), Some(1..8));
        assert_eq!(manual_call_name_range("Note"), None);
        assert_eq!(manual_call_name_range("git-add(foo)"), None);
        // Exact PP/RS inputs ran pinned CVS -Tutf8 first. man_html.c emits
        // only paragraph/div for these labels; a path, URL, mail-like label,
        // or section 0 is insufficient inferred catalog evidence.
        for value in [
            "https://example.test(1)",
            "/tmp/tool(1)",
            "user@tool(1)",
            "tool(0)",
        ] {
            assert_eq!(manual_call_name_range(value), None, "{value}");
        }
        assert_eq!(complete_term_label_range("working tree"), Some(0..12));
        assert_eq!(
            complete_term_label_range("file: not in gzip format"),
            Some(0..24)
        );
        assert!(is_presentation_term("1."));
        assert!(is_presentation_term("•"));
    }
}
