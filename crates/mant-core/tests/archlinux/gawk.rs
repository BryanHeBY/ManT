//! Tests for the Arch Linux `gawk(1)` fixture — operator precedence table
//! inline-term decisions.

use crate::common::{self, GAWK_SECTIONS};
use crate::fixtures::{archlinux_manual, archlinux_manual_query};
use mant_core::render_query_man;

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
            item.inline_term,
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
        !relational.inline_term,
        "gawk wide operator term should be inline_term=false"
    );
}

// gawk EXIT STATUS is prose paragraphs, not a bullet list — skip
// bullet-normalisation test for this fixture.

/// --format man renders inline operator terms tight (single space, no
/// leaked indent).
#[test]
fn man_format_renders_operator_table_tight() {
    let output = render_query_man(&archlinux_manual_query("gawk"));

    assert!(
        output.contains("* / % Multiplication, division, and modulus."),
        "got: {output:?}"
    );
    assert!(
        output.contains("space String concatenation."),
        "got: {output:?}"
    );
    // No leaked double-space.
    assert!(!output.contains("* / %  "), "got: {output:?}");
    assert!(!output.contains("space  "), "got: {output:?}");
}

// gawk legitimately contains `\f` inside regex character-class literals
// (`/[ \t\f\n\r\v]/`), so the standard markup-leak assertion does not
// apply. Skipping this check intentionally.

/// No duplicate vertical spacing.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_no_duplicate_vertical_spacing(
        &archlinux_manual("gawk").sections,
        "archlinux/gawk",
    );
}
