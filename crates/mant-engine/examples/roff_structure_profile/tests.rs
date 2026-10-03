//! Structure observation and loss-detection controls.
use std::collections::BTreeMap;

use super::{
    NoFillSourceLine, equation_visible_text, is_no_fill_row_text, is_zero_width_guard_line,
    retained_no_fill_rows,
};

fn parsed_structure(source: &str) -> (super::AstStructure, super::AstTopology, super::Document) {
    let report = super::Parser::new(super::ParseOptions {
        includes: super::IncludePolicy::Deny,
        compression: super::Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let (expected, topology) = super::ast_profile(&report.document.root);
    let document =
        mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes()).unwrap();
    (expected, topology, document)
}

#[test]
fn opaque_table_words_and_code_styles_are_not_equation_obligations() {
    // The exact source ran pristine ASCII/UTF8/HTML/tree/lint first.
    // roff_parseln excludes tbl from roff_eqndelim; code font is style.
    let (expected, expected_topology, document) = parsed_structure(
        ".TH AUDIT 1\n.SH BODY\n.EQ\ndelim $$\n.EN\n.TS\nl fCR.\n$x over 2$\n.TE\n\\f[CR]ordinary code\\fP\n",
    );
    let (observed, topology) = super::ir_profile(&document);
    assert_eq!(expected.table_equations, 0);
    assert_eq!(observed.table_equation_candidates, 0);
    assert_eq!(observed.inline_equation_candidates, 0);
    assert!(expected_topology.equations.is_empty());
    assert!(topology.equations.is_empty());
    assert!(
        super::compare_structure(&expected, &observed, &expected_topology, &topology).is_empty()
    );
}

#[test]
fn section_heading_links_are_counted_and_loss_is_detected() {
    let (expected, expected_topology, mut document) = parsed_structure(concat!(
        ".Dd September 11, 2026\n.Dt AUDIT 1\n.Os\n",
        ".Sh NAME\n.Nm audit\n.Nd heading link probe\n",
        ".Sh DESCRIPTION\n.Ss Eo\n.Xr printf 3\nReference\n.Ec\n.Pp\nVisible body.\n",
    ));
    let (observed, topology) = super::ir_profile(&document);
    assert_eq!(expected.manual_links, 1);
    assert_eq!(observed.manual_links, 1);
    assert!(
        super::compare_structure(&expected, &observed, &expected_topology, &topology).is_empty()
    );

    let heading = &mut document.sections[1].children[0].heading;
    // Preserve the words, but remove the typed link: topology must notice.
    heading.content = vec![super::Inline::Text {
        value: heading.plain_text(),
    }];
    let (mutated, topology) = super::ir_profile(&document);
    let violations = super::compare_structure(&expected, &mutated, &expected_topology, &topology);
    assert!(
        violations
            .iter()
            .any(|item| item.starts_with("manual-links:"))
    );
}

#[test]
fn nonpositive_rs_literals_do_not_require_positive_layout() {
    for argument in ["-7", "-2n", "-1.5m", "0", "+0n"] {
        let (expected, expected_topology, document) = parsed_structure(&format!(
            ".TH AUDIT 1\n.SH BODY\n.RS {argument}\n.PP\nVisible body.\n.RE\n"
        ));
        assert_eq!(expected.max_relative_indent_depth, 1, "{argument}");
        assert_eq!(expected.positive_relative_indent_scopes, 0, "{argument}");
        let (observed, topology) = super::ir_profile(&document);
        assert_eq!(observed.max_indent_columns, 0, "{argument}");
        assert!(
            super::compare_structure(&expected, &observed, &expected_topology, &topology)
                .is_empty()
        );
    }
}

#[test]
fn positive_rs_loss_is_detected_even_below_a_negative_scope() {
    for scopes in [".RS 4\n", ".RS\n", ".RS -2\n.RS 4\n"] {
        let (expected, expected_topology, mut document) = parsed_structure(&format!(
            ".TH AUDIT 1\n.SH BODY\n{scopes}.PP\nVisible body.\n.RE\n"
        ));
        assert_eq!(expected.positive_relative_indent_scopes, 1, "{scopes}");
        let (observed, topology) = super::ir_profile(&document);
        assert!(observed.max_indent_columns > 0, "{scopes}");
        assert!(
            super::compare_structure(&expected, &observed, &expected_topology, &topology)
                .is_empty()
        );
        for block in &mut document.sections[0].blocks {
            if let super::Block::Paragraph { layout, .. } = block {
                layout.indent_columns = 0;
            }
        }
        let (mutated, topology) = super::ir_profile(&document);
        assert_eq!(mutated.max_indent_columns, 0);
        let violations =
            super::compare_structure(&expected, &mutated, &expected_topology, &topology);
        assert!(
            violations
                .iter()
                .any(|item| item.starts_with("relative-indent:")),
            "{scopes}"
        );
    }
}

#[test]
fn unsupported_rs_arguments_retain_the_conservative_obligation() {
    for argument in ["-2q", "-1i", "-999999n", "-2+1", "NaN"] {
        let (expected, _, _) = parsed_structure(&format!(
            ".TH AUDIT 1\n.SH BODY\n.RS {argument}\n.PP\nVisible body.\n.RE\n"
        ));
        assert_eq!(expected.positive_relative_indent_scopes, 1, "{argument}");
    }
}

#[test]
fn punctuated_mdoc_definition_ordinals_compare_as_recovered_ordered_lists() {
    let (expected, topology, document) = parsed_structure(concat!(
        ".Dd September 12, 2026\n.Dt AUDIT 1\n.Os\n.Sh BODY\n",
        ".Bl -tag -width Ds\n.It 1.\nFirst.\n.It 2.\nSecond.\n.El\n"
    ));
    let (observed, observed_topology) = super::ir_profile(&document);
    assert_eq!(topology.lists[0].kind, super::ListTopologyKind::Generic);
    assert!(
        super::compare_structure(&expected, &observed, &topology, &observed_topology).is_empty()
    );
}

#[test]
fn ip_list_obligations_require_complete_authored_named_bullets() {
    let source = concat!(
        ".TH AUDIT 1\n.SH BODY\n",
        ".IP *\nStar key.\n.IP o\nLetter key.\n.IP +\nPlus key.\n",
        ".IP \\(bu\nBullet.\n.IP \\fB\\[bu]\\fR\nStyled bullet.\n",
        ".IP \\(buSuffix\nNot just a bullet.\n",
        ".IP \\(bu 4\nIndented bullet.\n.TP\n\\(bu\nTagged bullet.\n",
    );
    let parsed = super::Parser::new(super::ParseOptions {
        includes: super::IncludePolicy::Deny,
        compression: super::Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let (expected, _) = super::ast_profile(&parsed.document.root);
    assert_eq!(expected.generic_list_items, 4);
    let mut violations = Vec::new();
    super::underflow(
        &mut violations,
        "generic-list-items",
        expected.generic_list_items,
        0,
    );
    assert_eq!(violations.len(), 1, "losing named bullets must still fail");
}

#[test]
fn standalone_roff_font_switches_do_not_claim_visible_no_fill_lines() {
    assert!(!is_no_fill_row_text(r"\f[C]"));
    assert!(!is_no_fill_row_text(r"\fR"));
    assert!(!is_no_fill_row_text(r"    \f(CW"));
    assert!(!is_no_fill_row_text(r"\f[B]\f[R]\f[B]"));
    assert!(is_no_fill_row_text(r"\f[C]visible\f[R]"));
    assert!(is_no_fill_row_text("visible"));
}

#[test]
fn bounded_zero_width_guard_runs_claim_one_no_fill_row() {
    assert!(is_zero_width_guard_line(r"\&"));
    assert!(is_zero_width_guard_line(r"\& \&"));
    assert!(!is_zero_width_guard_line(r"\&\c"));
    let mut lines = BTreeMap::new();
    lines.insert(
        10,
        NoFillSourceLine {
            printable: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        11,
        NoFillSourceLine {
            zero_width_blank: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        12,
        NoFillSourceLine {
            zero_width_blank: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        13,
        NoFillSourceLine {
            printable: true,
            ..NoFillSourceLine::default()
        },
    );
    lines.insert(
        14,
        NoFillSourceLine {
            zero_width_blank: true,
            ..NoFillSourceLine::default()
        },
    );
    assert_eq!(retained_no_fill_rows(&lines), 3);
}

#[test]
fn equation_text_normalizes_bracketed_and_two_character_specials() {
    assert_eq!(
        equation_visible_text(r"1 + \(lf x \(rf \[->] infinity"),
        "1 + ⌊ x ⌋ → infinity"
    );
}
