//! fonts table-lowering contracts; original sources and gold remain unchanged.
use super::*;

#[test]
fn table_layout_font_reaches_inline_consumers() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-font.7"),
        b".TH FONT 7\n.TS\ntab(|);\nlb li.\nLEFT|RIGHT\n.TE\nafter\n",
    )
    .unwrap();
    let Block::Table { rows, .. } = &document.blocks[0] else {
        panic!("table");
    };
    let [Block::Paragraph { children: left, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("left cell");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = rows[0].cells[1].blocks.as_slice()
    else {
        panic!("right cell");
    };
    // CVS tbl_html.c::print_tbl scopes layout->font to one cell.
    assert!(matches!(left.as_slice(), [Inline::Strong { .. }]));
    assert!(matches!(right.as_slice(), [Inline::Emphasis { .. }]));
}

#[test]
fn table_layout_font_can_be_overridden_and_restored_within_one_cell() {
    // Exact input checked with the fixed -Thtml/-Tutf8/-Tlint oracle.
    // CVS tbl_html.c::print_tbl selects layout->font before print_text(),
    // where html.c::print_text() executes each \fR and \fP in source order.
    let document = parse_manual_bytes(
        std::path::Path::new("table-font-override.7"),
        b".TH REVIEW 7\n.SH DESCRIPTION\n.TS\nlb.\nA\\fRB\\fPC\n.TE\nafter\n",
    )
    .unwrap();
    let [
        Block::Table { rows, .. },
        Block::Paragraph {
            children: after, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("table and following paragraph");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("table cell");
    };
    assert!(matches!(children.as_slice(),
        [Inline::Strong { children: a }, Inline::Text { value: b }, Inline::Strong { children: c }]
        if matches!(a.as_slice(), [Inline::Text { value }] if value == "A")
            && b == "B"
            && matches!(c.as_slice(), [Inline::Text { value }] if value == "C")));
    assert!(matches!(after.as_slice(), [Inline::Text { value }] if value == "after"));
}

#[test]
fn source_recovered_code_cell_restores_presentation_after_inner_font_scope() {
    // Exact input checked with fixed CVS -Tutf8/-Thtml/-Tlint. tbl_html.c
    // retains one Li environment for the fCR cell; an admitted inline .Em
    // extension must restore that environment after its own font scope.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.TS\nl fCR.\nT{\n.Em WORD\nafter\nT}\n.TE\n";
    let document =
        parse_manual_bytes(std::path::Path::new("table-code-inner-scope.1"), source).unwrap();
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("table")
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("cell")
    };
    let after = children
        .iter()
        .find(|inline| inline_text(std::slice::from_ref(*inline)).contains("after"));
    assert!(
        matches!(after, Some(Inline::Code { value }) if value.contains("after")),
        "{children:#?}"
    );
}

#[test]
fn table_cell_font_selection_does_not_leak_to_next_cell_or_prose() {
    // Exact input checked with the fixed -Thtml/-Tutf8/-Tlint oracle.
    // CVS tbl_term.c::tbl_word pushes layout->font and term_fontpopq()
    // restores the prior current font before rendering the next cell.
    let document = parse_manual_bytes(
        std::path::Path::new("table-font-scope.7"),
        b".TH REVIEW 7\n.SH DESCRIPTION\n.TS\ntab(|);\nlb l.\nA\\fIB|C\n.TE\nafter\n",
    )
    .unwrap();
    let [
        Block::Table { rows, .. },
        Block::Paragraph {
            children: after, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("table and following paragraph");
    };
    let cell = |index: usize| match rows[0].cells[index].blocks.as_slice() {
        [Block::Paragraph { children, .. }] => children.as_slice(),
        other => panic!("cell {index}: {other:?}"),
    };
    assert!(matches!(cell(0),
        [Inline::Strong { children: a }, Inline::Emphasis { children: b }]
        if matches!(a.as_slice(), [Inline::Text { value }] if value == "A")
            && matches!(b.as_slice(), [Inline::Text { value }] if value == "B")));
    assert!(matches!(cell(1), [Inline::Text { value }] if value == "C"));
    assert!(matches!(after.as_slice(), [Inline::Text { value }] if value == "after"));
}

#[test]
fn roman_table_cell_keeps_terminal_font_register_at_its_stack_level() {
    // Exact input checked with the fixed -Tutf8/-Thtml oracle.  The outputs
    // differ: CVS tbl_term.c::tbl_word does not push for ESCAPE_FONTROMAN, so
    // term.c::term_fontrepl() in the first cell changes the current stack
    // level and subsequent terminal words inherit it. HTML restores metac
    // after each cell instead. This IR follows the terminal font register.
    let document = parse_manual_bytes(
        std::path::Path::new("table-roman-register.7"),
        b".TH REVIEW 7\n.SH DESCRIPTION\n.TS\ntab(|);\nl l.\nA\\fIB|C\n.TE\nafter\n",
    )
    .unwrap();
    let [
        Block::Table { rows, .. },
        Block::Paragraph {
            children: after, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("table and following paragraph");
    };
    let cell = |index: usize| match rows[0].cells[index].blocks.as_slice() {
        [Block::Paragraph { children, .. }] => children.as_slice(),
        other => panic!("cell {index}: {other:?}"),
    };
    assert!(matches!(cell(0),
        [Inline::Text { value: a }, Inline::Emphasis { children: b }]
        if a == "A" && matches!(b.as_slice(), [Inline::Text { value }] if value == "B")));
    assert!(matches!(cell(1), [Inline::Emphasis { children }] if
        matches!(children.as_slice(), [Inline::Text { value }] if value == "C")));
    assert!(
        matches!(after.as_slice(), [Inline::Emphasis { children }] if
        matches!(children.as_slice(), [Inline::Text { value }] if value == "after"))
    );
}

#[test]
fn native_table_constant_width_fonts_reach_ir_without_flattening_style() {
    // The exact input was checked with the fixed -Thtml/-Ttree oracle.
    // CVS tbl_layout.c::cellmod selects CR/CB/CI and tbl_html.c::print_tbl
    // applies html_setfont to each data cell independently.
    let document = parse_manual_bytes(
        std::path::Path::new("table-code-font.1"),
        b".TH FONT 1\n.SH DESCRIPTION\n.TS\nl fCR l fCB l fCI.\nplain\tbold\titalic\n.TE\n",
    )
    .unwrap();
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("table");
    };
    let cell = |index: usize| match rows[0].cells[index].blocks.as_slice() {
        [Block::Paragraph { children, .. }] => children.as_slice(),
        other => panic!("cell {index}: {other:?}"),
    };
    assert!(matches!(cell(0), [Inline::Code { value }] if value == "plain"));
    assert!(matches!(cell(1), [Inline::Strong { children }] if
        matches!(children.as_slice(), [Inline::Code { value }] if value == "bold")));
    assert!(matches!(cell(2), [Inline::Emphasis { children }] if
        matches!(children.as_slice(), [Inline::Code { value }] if value == "italic")));
}

#[test]
fn constant_width_table_layout_keeps_terminal_font_registers_separate_from_display() {
    fn runs(children: &[Inline]) -> Vec<(String, String)> {
        fn visit(children: &[Inline], style: &str, output: &mut Vec<(String, String)>) {
            for child in children {
                match child {
                    Inline::Text { value } => output.push((style.to_owned(), value.clone())),
                    Inline::Code { value } => {
                        output.push((format!("{style}code:"), value.clone()));
                    }
                    Inline::Strong { children } => {
                        visit(children, &format!("{style}bold:"), output);
                    }
                    Inline::Emphasis { children } => {
                        visit(children, &format!("{style}italic:"), output);
                    }
                    other => panic!("unexpected inline in font fixture: {other:?}"),
                }
            }
        }
        let mut output = Vec::new();
        visit(children, "", &mut output);
        output
    }

    // Each exact source was run through the fixed -Tutf8/-Thtml/-Tlint
    // reference before these assertions. CVS tbl_term.c::tbl_word skips a
    // terminal push for CR, pushes bold/underline for CB/CI, and pops each
    // cell stack; term.c::term_fontlast controls the later \fP. In contrast,
    // tbl_html.c::print_tbl/html.c::html_setfont keep a code presentation
    // for all three layouts and restore it after in-cell font escapes.
    for (label, layout, cell_text, expected_cell, expected_after) in [
        ("cr", "CR", r"A", vec![("code:", "A")], ""),
        ("cb", "CB", r"A", vec![("bold:code:", "A")], "bold:"),
        ("ci", "CI", r"A", vec![("italic:code:", "A")], "bold:"),
        (
            "cr-inner",
            "CR",
            r"A\fIB\fPC",
            vec![("code:", "A"), ("italic:", "B"), ("code:", "C")],
            "italic:",
        ),
        (
            "cb-inner",
            "CB",
            r"A\fRB\fPC",
            vec![("bold:code:", "A"), ("", "B"), ("bold:code:", "C")],
            "",
        ),
        (
            "ci-inner",
            "CI",
            r"A\fRB\fPC",
            vec![("italic:code:", "A"), ("", "B"), ("italic:code:", "C")],
            "",
        ),
    ] {
        let source = format!(
            ".TH FONT 7\n.SH DESCRIPTION\n.ft B\n.TS\nl f{layout}.\n{cell_text}\n.TE\n\\fPAFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("table-code-execution-{label}.7")),
            source.as_bytes(),
        )
        .unwrap();
        let [
            Block::Table { rows, .. },
            Block::Paragraph {
                children: after, ..
            },
        ] = document.sections[0].blocks.as_slice()
        else {
            panic!("{label}: expected table and following paragraph");
        };
        let [Block::Paragraph { children: cell, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("{label}: expected one text cell");
        };
        assert_eq!(
            runs(cell),
            expected_cell
                .into_iter()
                .map(|(style, value)| (style.to_owned(), value.to_owned()))
                .collect::<Vec<_>>(),
            "{label}: {cell:?}"
        );
        assert_eq!(
            runs(after),
            [(expected_after.to_owned(), "AFTER".to_owned())],
            "{label}: {after:?}"
        );
    }
}

#[test]
fn cr_cell_without_terminal_push_leaves_in_word_font_for_plain_next_cell() {
    // Exact source checked with the fixed -Tutf8/-Thtml/-Tlint oracle. CVS
    // tbl_term.c::tbl_word skips term_fontpush for CR, so its in-word \fI
    // changes the base stack level inherited by the following Roman cell.
    // HTML scopes each cell instead; this IR follows terminal execution
    // across the table and retains constant width for the CR cell only.
    let document = parse_manual_bytes(
        std::path::Path::new("table-cr-next-cell.7"),
        b".TH FONT 7\n.SH DESCRIPTION\n.ft B\n.TS\nl fCR l.\nA\\fIB\tC\n.TE\n\\fPAFTER\n",
    )
    .unwrap();
    let [
        Block::Table { rows, .. },
        Block::Paragraph {
            children: after, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected table and following paragraph");
    };
    let cell = |index: usize| match rows[0].cells[index].blocks.as_slice() {
        [Block::Paragraph { children, .. }] => children.as_slice(),
        other => panic!("unexpected cell {index}: {other:?}"),
    };
    assert!(matches!(cell(0),
        [Inline::Code { value: a }, Inline::Emphasis { children: b }]
        if a == "A" && matches!(b.as_slice(), [Inline::Text { value }] if value == "B")));
    assert!(matches!(cell(1),
        [Inline::Emphasis { children }]
        if matches!(children.as_slice(), [Inline::Text { value }] if value == "C")));
    assert!(matches!(after.as_slice(),
        [Inline::Strong { children }]
        if matches!(children.as_slice(), [Inline::Text { value }] if value == "AFTER")));
}

#[test]
fn code_and_roman_with_equal_terminal_font_keep_distinct_previous_register_origins() {
    // Both exact sources were checked with the fixed -Tutf8/-Thtml/-Tlint
    // oracle. `.ft CR` gives terminal current=Roman and previous=Roman, but
    // HTML current=Code and previous=Roman. CVS term.c::term_fontlast swaps
    // register *positions*, not values; tbl_term.c::tbl_word never pushes CR.
    // The temporary tbl_html.c::print_tbl code style therefore must not
    // rewrite the terminal previous register after the cell.
    for (label, text) in [("plain", r"A"), ("inword", r"A\fRB\fPC")] {
        let source =
            format!(".TH FONT 7\n.SH DESCRIPTION\n.ft CR\n.TS\nl fCR.\n{text}\n.TE\n\\fPAFTER\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("table-equal-terminal-font-{label}.7")),
            source.as_bytes(),
        )
        .unwrap();
        let [
            Block::Table { rows, .. },
            Block::Paragraph {
                children: after, ..
            },
        ] = document.sections[0].blocks.as_slice()
        else {
            panic!("{label}: expected table and following paragraph");
        };
        let [Block::Paragraph { children: cell, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("{label}: expected one text cell");
        };
        if label == "plain" {
            assert!(matches!(cell.as_slice(), [Inline::Code { value }] if value == "A"));
        } else {
            assert!(matches!(cell.as_slice(),
                [Inline::Code { value: a }, Inline::Text { value: b }, Inline::Code { value: c }]
                if a == "A" && b == "B" && c == "C"));
        }
        assert!(
            matches!(after.as_slice(), [Inline::Text { value }] if value == "AFTER"),
            "{label}: {after:?}"
        );
    }
}

#[test]
fn declined_tbl_recovery_never_consumes_native_macro_expansion_siblings() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-appended-macro-sibling.1"),
        b".TH TBL-APPENDED-MACRO-SIBLING 1\n.am1 B\nADDED_TEXT\n..\n.SH DESCRIPTION\n.TS\nl.\nT{\n.B ORIGINAL_OPERAND\nT}\n.TE\n",
    )
    .expect("lower a table with an appended macro body");
    let text = visible_document_text(&document);
    assert!(text.contains("ADDED_TEXT"), "{text}");
    assert!(text.contains("ORIGINAL_OPERAND"), "{text}");
}
