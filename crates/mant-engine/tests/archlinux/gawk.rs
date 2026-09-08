//! Tests for the Arch Linux `gawk(1)` fixture — operator precedence table
//! inline-term decisions.

use crate::common::{self, GAWK_SECTIONS};
use crate::fixtures::{archlinux_manual, archlinux_manual_query};
use mant_engine::render_query_man;

/// Section topology: 23 sections from NAME through COPYING PERMISSIONS.
#[test]
fn keeps_section_topology() {
    common::assert_section_topology("archlinux/gawk", archlinux_manual("gawk"), GAWK_SECTIONS);
}

/// The operator precedence table in "PATTERNS AND ACTIONS" contains
/// short terms (`* / %`, `&&`, `space`) that the model must flag as
/// `inline_term = true`, and wider terms (`< > <= >= == !=`) that stay
/// `inline_term = false`.
#[test]
fn operator_table_has_correct_inline_term_decisions() {
    let doc = archlinux_manual("gawk");
    let section = common::section(doc, "PATTERNS AND ACTIONS");
    let items = common::nested_definition_items(section);

    // Short operator terms → inline.
    for needle in ["* / %", "&&", "space"] {
        let item = items
            .iter()
            .find(|item| {
                item.terms
                    .iter()
                    .any(|term| common::inline_text(term) == needle)
            })
            .unwrap_or_else(|| panic!("missing gawk operator term {needle:?}"));
        assert!(
            item.layout.inline_term,
            "gawk operator {needle:?} should be inline_term=true"
        );
    }

    // Wide relational-operator term → not inline.
    let relational = items
        .iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| common::inline_text(term).contains("< >"))
        })
        .expect("gawk relational operator term");
    assert!(
        !relational.layout.inline_term,
        "gawk wide operator term should be inline_term=false"
    );
}

// gawk EXIT STATUS is prose paragraphs, not a bullet list — skip
// bullet-normalisation test for this fixture.

/// Inline operator bodies use the resolved source column, not a renderer's
/// hard-coded single-space separator. The compound width expression in this
/// fixture uses the documented unsupported-measurement fallback.
#[test]
fn man_format_preserves_resolved_operator_body_columns() {
    let output = render_query_man(&archlinux_manual_query("gawk"));
    let document = archlinux_manual("gawk");
    let items = common::nested_definition_items(common::section(document, "PATTERNS AND ACTIONS"));
    for (term, body) in [
        ("* / %", "Multiplication, division, and modulus."),
        ("space", "String concatenation."),
    ] {
        let item = items
            .iter()
            .find(|item| {
                item.terms
                    .iter()
                    .any(|head| common::inline_text(head) == term)
            })
            .unwrap();
        assert!(item.layout.inline_term);
        assert_eq!(item.layout.body_indent_columns, 7);
        let line = output.lines().find(|line| line.contains(body)).unwrap();
        assert_eq!(
            line.find(body).unwrap() - line.find(term).unwrap(),
            7,
            "{line}"
        );
    }
}

// gawk legitimately contains `\f` inside regex character-class literals
// (`/[ \t\f\n\r\v]/`), so the standard markup-leak assertion does not
// apply. Skipping this check intentionally.

/// No duplicate vertical spacing.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_bounded_vertical_spacing(&archlinux_manual("gawk").sections, "archlinux/gawk");
}
