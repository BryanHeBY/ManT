use super::*;
use mant_ir::{EntryKind, EntryNameEvidence, EntryOwner, ParameterKind, ResolvedContent};
use mant_protocol::{EntryProjection, EvidenceBasis, ExplanationOptions, ExplanationQuery};

mod addresses;
mod scale_probe;

#[test]
fn native_tbl_cells_lower_into_one_shared_content_store() {
    // This exact UTF-8/78 input was checked against the fixed CVS reference.
    // tbl_term.c::tbl_word emits authored text once per data cell; the native
    // collector retains an empty cell without manufacturing a visible atom.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "table.1",
            b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l.\nleft;right\nempty;\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("table.1", &bundle, InputFormat::Man)
        .expect("native table lowers to the final private IR consumer");
    let Block::Table {
        rows, fixed_view, ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("first section block is a table")
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(rows[1].cells.len(), 2);
    assert!(rows[1].cells[1].blocks.is_empty());
    assert!(fixed_view.is_none());
    assert!(mant_ir::validate_content_store(&document.content_store).is_ok());
}

#[test]
fn boxed_tbl_uses_native_physical_rows_and_shared_cell_atoms() {
    // Exact input was checked with fixed CVS -T utf8 -O width=78 before
    // this assertion. tbl_term.c::term_tbl draws the frame while tbl_word
    // contributes the authored cell text to the same logical atom store.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "box.1",
            b".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\nleft;right\nempty;\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("box.1", &bundle, InputFormat::Man)
        .expect("boxed table lowers to the final private IR consumer");
    let Block::Table {
        fixed_view: Some(key),
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("boxed table has native fixed geometry")
    };
    let lines = document
        .content_store
        .fixed_view(*key)
        .unwrap()
        .physical_lines(document.content())
        .expect("valid fixed geometry materializes");
    assert_eq!(
        lines,
        [
            "     ┌───────────────┐",
            "     │ left    right │",
            "     │ empty         │",
            "     └───────────────┘",
        ]
    );
    assert_eq!(
        document
            .content_store
            .atoms
            .iter()
            .filter(|atom| atom.kind.text() == Some("left"))
            .count(),
        1
    );
}

#[test]
fn native_tbl_span_rule_and_alignment_follow_fixed_cvs_geometry() {
    // Each exact source was checked with fixed CVS -T utf8 -O width=78.
    // tbl_term.c::term_tbl handles colspan, horizontal rules, and native
    // alignment; the IR stores these physical rows separately from cells.
    let cases: [(&str, &str, &[&str]); 3] = [
        (
            "box-span.1",
            ".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl s l.\nwide;tail\n.TE\n",
            &[
                "     ┌──────────────┐",
                "     │ wide    tail │",
                "     └──────────────┘",
            ],
        ),
        (
            "box-rule.1",
            ".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\na;b\n_\nc;d\n.TE\n",
            &[
                "     ┌───────┐",
                "     │ a   b │",
                "     ├───────┤",
                "     │ c   d │",
                "     └───────┘",
            ],
        ),
        (
            "box-align.1",
            ".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nr c.\nleft;center\n.TE\n",
            &[
                "     ┌───────────────┐",
                "     │ left   center │",
                "     └───────────────┘",
            ],
        ),
    ];
    for (name, source, expected) in cases {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = project_native_manual(name, &bundle, InputFormat::Man)
            .expect("native table lowers with checked physical geometry");
        let Block::Table {
            fixed_view: Some(key),
            ..
        } = &document.sections[0].blocks[0]
        else {
            panic!("{name}: table has native fixed geometry")
        };
        let lines = document
            .content_store
            .fixed_view(*key)
            .unwrap()
            .physical_lines(document.content())
            .unwrap();
        assert_eq!(lines, expected, "{name}");
    }
}

#[test]
fn native_boxed_tbl_reaches_real_narrow_horizontal_buffer() {
    use mant_ir::ResolvedContent;
    use mant_ui::DocumentView;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    // Exact source and full-width table rows were checked with fixed CVS
    // UTF-8/78. C09b's fixed-row viewport clips these native columns rather
    // than reflowing them when the terminal narrows.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "box.1",
            b".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\nleft;right\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("box.1", &bundle, InputFormat::Man).unwrap();
    let resolved = ResolvedContent {
        label: "T(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let view = DocumentView::new(&resolved);
    assert!(view.max_fixed_columns() > 12);
    let narrow = view.render_with_horizontal_offset(12, 0);
    let row = narrow
        .text
        .lines
        .iter()
        .position(|line| line.to_string().contains("left"))
        .unwrap();
    let area = Rect::new(0, 0, 12, 1);
    let mut buffer = Buffer::empty(area);
    Paragraph::new(narrow.text.lines[row].clone()).render(area, &mut buffer);
    assert_eq!(buffer[(5, 0)].symbol(), "│");
    assert_eq!(buffer[(7, 0)].symbol(), "l");
    let found = narrow.search("right");
    assert_eq!(
        found.len(),
        1,
        "authored cell text remains searchable when clipped"
    );
    assert_eq!(found[0].row, row);
    assert!(found[0].start_column >= 12);
    assert!(
        narrow.search("│").is_empty(),
        "native border is not logical text"
    );

    let scrolled = view.render_with_horizontal_offset(12, 9);
    let mut shifted = Buffer::empty(area);
    Paragraph::new(scrolled.text.lines[row].clone()).render(area, &mut shifted);
    assert_eq!(shifted[(0, 0)].symbol(), "f");
    assert_eq!(shifted[(11, 0)].symbol(), "│");
    assert!(
        scrolled.text.lines[row].to_string().contains("right"),
        "horizontal offset reveals the clipped cell occurrence",
    );
}

#[test]
fn native_boxed_tbl_keeps_styled_cell_content() {
    // Exact input was checked with fixed CVS UTF-8/78. tbl_term.c::tbl_word
    // keeps the font scope while terminal overstrike is only presentation.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "style.1",
            b".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\n\\fBbold\\fP;plain\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("style.1", &bundle, InputFormat::Man)
        .expect("styled table remains a valid fixed view");
    let Block::Table {
        fixed_view: Some(key),
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("styled table has native fixed geometry")
    };
    let lines = document
        .content_store
        .fixed_view(*key)
        .unwrap()
        .physical_lines(document.content())
        .unwrap();
    assert!(lines.iter().any(|line| line.contains("bold   plain")));
    assert!(
        document
            .content_store
            .atoms
            .iter()
            .any(|atom| atom.kind.text() == Some("bold") && atom.style.strong)
    );
}

#[test]
fn native_eqn_words_reach_the_final_ir_inline_consumer() {
    fn words(nodes: &[Inline], content: mant_ir::ContentContext<'_>, output: &mut String) {
        for node in nodes {
            match node {
                Inline::Text { content: reference } | Inline::Code { content: reference } => {
                    output.push_str(content.resolve_text(*reference).unwrap());
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => words(children, content, output),
                Inline::Anchor { .. } | Inline::LineBreak { .. } => {}
            }
        }
    }
    // Exact UTF-8/78 source was checked with fixed CVS. Its man_term.c
    // ROFFT_EQN branch calls term_eqn without imposing a separate display
    // break, so simple formula words stay in the surrounding prose root.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "eqn.1",
            b".TH T 1\n.SH DESCRIPTION\nbefore\n.EQ\nx + y\n.EN\nafter\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("eqn.1", &bundle, InputFormat::Man)
        .expect("native equation reaches final IR");
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("inline equation remains in prose")
    };
    let mut text = String::new();
    words(children, document.content(), &mut text);
    assert!(text.contains("before"));
    assert!(text.contains('x'));
    assert!(text.contains('y'));
    assert!(text.contains("after"));
}

#[test]
fn native_eqn_fraction_and_radical_execute_in_both_macrosets() {
    // All exact sources were checked with fixed CVS UTF-8/78. In
    // eqn_term.c::eqn_box, `over` emits a slash and `sqrt` emits a radical
    // before its child expression; no Rust equation layout is synthesized.
    let cases = [
        (
            "fraction.1",
            InputFormat::Man,
            ".TH T 1\n.SH DESCRIPTION\n.EQ\nx over y\n.EN\n",
            "x/y",
        ),
        (
            "radical.1",
            InputFormat::Man,
            ".TH T 1\n.SH DESCRIPTION\n.EQ\nsqrt { x + y }\n.EN\n",
            "√(x + y)",
        ),
        (
            "fraction.1",
            InputFormat::Mdoc,
            ".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.EQ\nx over y\n.EN\n",
            "x/y",
        ),
    ];
    for (name, format, source, expected) in cases {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = project_native_manual(name, &bundle, format).unwrap();
        let text = document
            .content_store
            .atoms
            .iter()
            .filter_map(|atom| atom.kind.text())
            .collect::<String>();
        assert!(text.contains(expected), "{name}: {text}");
    }
}

#[test]
fn native_nofill_and_literal_display_share_logical_content_with_fixed_rows() {
    // Both exact sources were checked with fixed CVS UTF-8/78 before these
    // assertions. man/mdoc terminal handlers preserve two authored lines;
    // the second begins with two authored spaces after the native indent.
    let cases = [
        (
            "nf.1",
            InputFormat::Man,
            ".TH T 1\n.SH DESCRIPTION\nbefore\n.nf\nalpha  beta\n  gamma\n.fi\nafter\n",
        ),
        (
            "bd.1",
            InputFormat::Mdoc,
            ".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Bd -literal\nalpha  beta\n  gamma\n.Ed\nafter\n",
        ),
    ];
    for (name, format, source) in cases {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = project_native_manual(name, &bundle, format)
            .expect("no-fill content reaches the final IR consumer");
        let blocks = &document.sections[0].blocks;
        assert_eq!(blocks.len(), 3, "{name}");
        let Block::FixedDisplay { view, children, .. } = &blocks[1] else {
            panic!("{name}: middle block is fixed display")
        };
        let lines = document
            .content_store
            .fixed_view(*view)
            .unwrap()
            .physical_lines(document.content())
            .unwrap();
        assert_eq!(lines, ["     alpha  beta", "       gamma"], "{name}");
        assert!(
            children
                .iter()
                .any(|node| matches!(node, Inline::LineBreak { .. })),
            "{name}"
        );
    }
}

#[test]
fn native_nofill_long_blank_and_eof_rows_remain_fixed() {
    // Each exact source was checked with fixed CVS UTF-8 width=20. The
    // terminal output preserves a long row, an interior blank row, and an
    // unterminated no-fill region at EOF without introducing a soft wrap.
    let cases: [(&str, &str, &[&str]); 3] = [
        (
            "long.1",
            ".TH T 1\n.SH D\n.nf\nabcdefghijklmnopqrstuvwxyz0123456789\n.fi\n",
            &["     abcdefghijklmnopqrstuvwxyz0123456789"],
        ),
        (
            "blank.1",
            ".TH T 1\n.SH D\n.nf\nalpha\n\nbeta\n.fi\n",
            &["     alpha", "", "     beta"],
        ),
        (
            "eof.1",
            ".TH T 1\n.SH D\n.nf\nalpha\nbeta\n",
            &["     alpha", "     beta"],
        ),
    ];
    for (name, source, expected) in cases {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = project_native_manual(name, &bundle, InputFormat::Man).unwrap();
        let fixed = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::FixedDisplay { view, .. } => Some(*view),
                _ => None,
            })
            .expect("no-fill region has a fixed view");
        let lines = document
            .content_store
            .fixed_view(fixed)
            .unwrap()
            .physical_lines(document.content())
            .unwrap();
        assert_eq!(lines, expected, "{name}");
    }
}

#[test]
fn long_nofill_row_preserves_one_logical_run_and_native_columns() {
    // This exact 10,000-scalar input was checked with fixed CVS UTF-8/78.
    // man_term.c::print_man_node sets TERMP_BRNEVER for NODE_NOFILL, and
    // term_newln flushes the entire authored line without wrapping it.
    let source = format!(".TH T 1\n.SH D\n.nf\n{}\n.fi\n", "a".repeat(10_000));
    let mut bundle = SourceBundle::new();
    bundle.insert("long-nofill.1", source.into_bytes()).unwrap();
    let document = project_native_manual("long-nofill.1", &bundle, InputFormat::Man)
        .expect("long native no-fill row reaches final IR");
    let Block::FixedDisplay { view, .. } = &document.sections[0].blocks[0] else {
        panic!("no-fill row has a fixed view")
    };
    let lines = document
        .content_store
        .fixed_view(*view)
        .unwrap()
        .physical_lines(document.content())
        .unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].len(), 10_005);
    assert!(lines[0].starts_with("     a"));
    assert!(lines[0].ends_with("aaaa"));
}

#[test]
fn nofill_mixed_width_affine_ranges_keep_native_columns() {
    // Exact input was checked with fixed CVS UTF-8/78. term.c::term_flushln
    // advances by each glyph's terminal width; adjacent wide scalars share
    // one affine mapping without merging across the narrow neighbors.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "wide-nofill.1",
            ".TH T 1\n.SH D\n.nf\nA界界Z\n.fi\n".as_bytes().to_vec(),
        )
        .unwrap();
    let document = project_native_manual("wide-nofill.1", &bundle, InputFormat::Man).unwrap();
    let Block::FixedDisplay { view, .. } = &document.sections[0].blocks[0] else {
        panic!("native no-fill is fixed")
    };
    let fixed = document.content_store.fixed_view(*view).unwrap();
    assert_eq!(
        fixed.physical_lines(document.content()).unwrap(),
        ["     A界界Z"]
    );
    let placements = &fixed.lines[0].placements;
    assert_eq!(placements.len(), 3);
    assert_eq!(
        (placements[0].start_column, placements[0].end_column),
        (5, 6)
    );
    assert_eq!(
        (placements[1].start_column, placements[1].end_column),
        (6, 10)
    );
    assert_eq!(
        (placements[2].start_column, placements[2].end_column),
        (10, 11)
    );
}

#[test]
fn literal_display_keeps_link_occurrence_on_shared_content() {
    // Exact input was checked with fixed CVS UTF-8 width=20. In
    // mdoc_term.c::termp_lk_pre, the label and URL execute inside the
    // no-fill display; terminal underline is only presentation.
    let mut bundle = SourceBundle::new();
    bundle.insert("literal-link.1", b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Bd -literal\n.Lk https://example.test label\nnext\n.Ed\n".to_vec()).unwrap();
    let document = project_native_manual("literal-link.1", &bundle, InputFormat::Mdoc)
        .expect("native literal link reaches final IR");
    let Block::FixedDisplay { view, children, .. } = &document.sections[0].blocks[0] else {
        panic!("literal display is fixed")
    };
    let lines = document
        .content_store
        .fixed_view(*view)
        .unwrap()
        .physical_lines(document.content())
        .unwrap();
    assert_eq!(lines, ["     label: https://example.test", "     next"]);
    assert!(
        children
            .iter()
            .any(|node| matches!(node, Inline::Link { .. }))
    );
    assert_eq!(document.content_store.links.len(), 1);
}

#[test]
fn native_nofill_long_row_clips_in_real_horizontal_buffer() {
    use mant_ir::ResolvedContent;
    use mant_ui::DocumentView;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    // Exact long no-fill source was checked with fixed CVS UTF-8 width=20;
    // its one physical row remains one row at narrow viewport widths.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "long.1",
            b".TH T 1\n.SH D\n.nf\nabcdefghijklmnopqrstuvwxyz0123456789\n.fi\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("long.1", &bundle, InputFormat::Man).unwrap();
    let resolved = ResolvedContent {
        label: "T(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let view = DocumentView::new(&resolved);
    let narrow = view.render_with_horizontal_offset(12, 0);
    let row = narrow
        .text
        .lines
        .iter()
        .position(|line| line.to_string().contains("abcdefg"))
        .unwrap();
    let shifted = view.render_with_horizontal_offset(12, 20);
    assert_eq!(narrow.row_count, shifted.row_count);
    let area = Rect::new(0, 0, 12, 1);
    let mut buffer = Buffer::empty(area);
    Paragraph::new(shifted.text.lines[row].clone()).render(area, &mut buffer);
    assert_eq!(buffer[(0, 0)].symbol(), "p");
    assert_eq!(buffer[(11, 0)].symbol(), "0");
}

#[test]
fn nested_literal_display_retains_native_absolute_columns() {
    // Exact mdoc and man inputs were checked with fixed CVS UTF-8/78.
    // mdoc_term.c::termp_bd_pre and man_term.c::pre_TP establish different
    // native body origins; the fixed view stores each origin only once.
    let cases = [
        (
            "nested.1",
            InputFormat::Mdoc,
            ".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a\n.Bd -literal\nfirst\n  second\n.Ed\n.El\n",
            &["             first", "               second"] as &[_],
        ),
        (
            "nested.1",
            InputFormat::Man,
            ".TH T 1\n.SH OPTIONS\n.TP\n.B --alpha\n.nf\nfirst\n  second\n.fi\n",
            &["            first", "              second"],
        ),
    ];
    for (name, format, source, expected) in cases {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = project_native_manual(name, &bundle, format).unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
            panic!("nested fixed region belongs to a definition")
        };
        let view = items[0]
            .description
            .iter()
            .find_map(|block| match block {
                Block::FixedDisplay { view, .. } => Some(*view),
                _ => None,
            })
            .expect("definition contains fixed display");
        let lines = document
            .content_store
            .fixed_view(view)
            .unwrap()
            .physical_lines(document.content())
            .unwrap();
        assert_eq!(lines, expected);
    }
}

#[test]
fn literal_target_remains_zero_width_at_its_fixed_row() {
    use mant_ir::ResolvedContent;
    use mant_ui::DocumentView;

    // Exact source was checked with fixed CVS tree and HTML. tag.c keeps
    // the authored .Tg on its own zero-width node, while mdoc_term.c emits
    // the following no-fill text on the same physical display row.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "anchor.1",
            b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Bd -literal\n.Tg Anchor\nfirst\n.Ed\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("anchor.1", &bundle, InputFormat::Mdoc).unwrap();
    let Block::FixedDisplay { children, .. } = &document.sections[0].blocks[0] else {
        panic!("literal target belongs to fixed display")
    };
    assert!(
        children
            .iter()
            .any(|node| matches!(node, Inline::Anchor { .. }))
    );
    let resolved = ResolvedContent {
        label: "T(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let view = DocumentView::new(&resolved);
    let rendered = view.render(20);
    let row = rendered
        .text
        .lines
        .iter()
        .position(|line| line.to_string().contains("first"))
        .unwrap();
    assert_eq!(rendered.anchor_row("Anchor"), Some(row));
}

#[test]
fn literal_targets_between_and_after_rows_keep_their_structural_position() {
    use mant_ir::ResolvedContent;
    use mant_ui::DocumentView;

    // Exact inputs were checked with fixed CVS HTML. post_tg() leaves these
    // targets inside the literal display: Mid lies between physical rows,
    // while Tail follows the final text row without moving to an old root.
    for (target, body, following) in [
        ("Mid", "first\n.Tg Mid\nsecond\n", Some("second")),
        ("Tail", "first\n.Tg Tail\n", None),
    ] {
        let mut bundle = SourceBundle::new();
        let source =
            format!(".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Bd -literal\n{body}.Ed\n");
        bundle.insert("anchor.1", source.into_bytes()).unwrap();
        let document = project_native_manual("anchor.1", &bundle, InputFormat::Mdoc).unwrap();
        let Block::FixedDisplay { children, .. } = &document.sections[0].blocks[0] else {
            panic!("target belongs to literal display")
        };
        assert!(
            children
                .iter()
                .any(|node| matches!(node, Inline::Anchor { .. }))
        );
        let resolved = ResolvedContent {
            label: "T(1)".to_owned(),
            address: None,
            document: Some(document),
            tldr: None,
        };
        let rendered = DocumentView::new(&resolved).render(20);
        let first = rendered
            .text
            .lines
            .iter()
            .position(|line| line.to_string().contains("first"))
            .unwrap();
        let anchor = rendered.anchor_row(target).unwrap();
        assert!(anchor > first, "{target}: anchor must follow first row");
        if let Some(next) = following {
            let second = rendered
                .text
                .lines
                .iter()
                .position(|line| line.to_string().contains(next))
                .unwrap();
            // The terminal path has no visible row for .Tg itself.  The
            // zero-width target sits at the next row's beginning.
            assert_eq!(anchor, second, "{target}: target precedes second row text");
        }
    }
}

#[test]
fn large_native_fixed_table_keeps_one_logical_cell_store() {
    use std::collections::HashSet;
    use std::fmt::Write as _;

    // This exact 1,000-row source was run through fixed CVS UTF-8/78 before
    // the assertion. tbl_term.c::term_tbl repeats the allbox rule between
    // data rows; its drawing belongs to fixed geometry, not extra cell text.
    let mut source = ".TH T 1\n.SH DATA\n.TS\nallbox tab(;);\nl l.\n".to_owned();
    for index in 0..1_000 {
        writeln!(source, "left_{index};right_{index}").expect("write to String");
    }
    source.push_str(".TE\n");
    let mut bundle = SourceBundle::new();
    bundle.insert("large-table.1", source.into_bytes()).unwrap();
    let document = project_native_manual("large-table.1", &bundle, InputFormat::Man)
        .expect("large supported native table reaches the final IR");
    let Block::Table {
        rows,
        fixed_view: Some(view),
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("allbox table retains native geometry")
    };
    assert_eq!(rows.len(), 1_000);
    assert_eq!(document.content_store.fixed_views.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(rows[999].cells.len(), 2);
    let mut unique_cells = HashSet::new();
    for text in document
        .content_store
        .atoms
        .iter()
        .filter_map(|atom| atom.kind.text())
        .filter(|text| text.starts_with("left_") || text.starts_with("right_"))
    {
        assert!(unique_cells.insert(text), "duplicated logical cell {text}");
    }
    assert_eq!(unique_cells.len(), 2_000);
    for expected in ["left_0", "right_0", "left_999", "right_999"] {
        assert_eq!(
            document
                .content_store
                .atoms
                .iter()
                .filter(|atom| atom.kind.text() == Some(expected))
                .count(),
            1,
            "{expected} has one logical occurrence despite fixed placement",
        );
    }
    let lines = document
        .content_store
        .fixed_view(*view)
        .unwrap()
        .physical_lines(document.content())
        .unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("left_0") && line.contains("right_0"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("left_999") && line.contains("right_999"))
    );
}

#[test]
fn native_tbl_layout_rule_omits_ignored_data_text() {
    // Exact source checked with fixed CVS UTF-8/78. tbl_term.c::tbl_data
    // renders its layout `_`/`=` rule before consulting the row data.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "layout-rule.1",
            b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l\n_ =.\na;b\nx;y\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("layout-rule.1", &bundle, InputFormat::Man)
        .expect("layout rule lowers without manufacturing data text");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("table block")
    };
    assert!(matches!(
        rows[1].kind,
        mant_ir::TableRowKind::LayoutRule { .. }
    ));
    assert!(rows[1].cells.is_empty());
    assert!(mant_ir::validate_content_store(&document.content_store).is_ok());
}

#[test]
fn ascii_overstrike_drops_only_the_unsafe_display_override() {
    // The exact `.TH X 1`, `.SH NAME`, `X \[ct] Y` input was first run
    // through the fixed ASCII/78 reference; term_ascii.c::ascii_uc2str
    // projects the cent sign as `/\bc`. The native terminal renderer retains
    // that historical output, while the structured IR keeps the logical `¢`
    // without exposing backspace as an inline display glyph.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("cent.1", b".TH X 1\n.SH NAME\nX \\[ct] Y\n".to_vec())
        .unwrap();
    let native = libmandoc_rs::structured::StructuredRenderer::new()
        .with_profile(libmandoc_rs::structured::StructuredProfile::Ascii)
        .render_bundle("cent.1", &bundle, InputFormat::Man)
        .expect("legal ASCII manual retains its native structured result");
    let projection = NativeProseProjection::new(native).expect("native prose projects");
    let document = lower_projection(projection).expect("native prose lowers to document IR");
    mant_ir::validate_content_store(&document.content_store).unwrap();
    let cent = document
        .content_store
        .atoms
        .iter()
        .find(|atom| atom.kind.text() == Some("¢"))
        .expect("cent sign is retained as a logical atom");
    assert!(matches!(
        &cent.kind,
        mant_ir::ContentAtomKind::Text {
            display_override: None,
            ..
        }
    ));
}

#[test]
fn cross_wrapper_terms_reach_real_entry_facts_without_body_borrowing() {
    // The same source was run through the pinned reference before this
    // assertion. `man_macro.c::blk_imp` binds TQ to the preceding TP body;
    // `man_term.c::pre_TP` executes BR/B wrappers in formatter order.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "c03.1",
            br#".TH C03 1
.SH OPTIONS
.TP
.BR --output , " -o=" FILE
.TQ
.B -O
Write file.
.TP
.B --empty
.TP
.B --same
Body A.
.TP
.B --same
Body B.
"#
            .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("c03.1", &bundle, InputFormat::Man)
        .expect("native structure lowers to semantic IR");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("first structured block is a definition list")
    };
    assert_eq!(items.len(), 1, "each man TP owns its native list block");
    assert_eq!(items[0].terms.len(), 2);
    let facts = items[0].entry.as_ref().expect("option facts discovered");
    assert_eq!(
        facts.kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    assert_eq!(facts.names, ["--output", "-o", "-O"]);
    assert_eq!(facts.forms.len(), 3);
    assert_eq!(
        facts
            .forms
            .iter()
            .map(|form| {
                document
                    .content()
                    .entry_form(EntryOwner::Definition(&items[0]), form)
                    .unwrap()
                    .map(|form| mant_ir::inline_plain_text(document.content(), &form))
                    .unwrap()
            })
            .collect::<Vec<_>>(),
        ["--output", "-o=FILE", "-O"]
    );
    assert!(
        facts
            .name_bindings
            .iter()
            .all(|binding| binding.evidence == EntryNameEvidence::Lexical)
    );
    assert_eq!(items[0].description.len(), 1);

    let definition_items = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.as_slice()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(definition_items.len(), 4);
    assert!(definition_items[1].description.is_empty());
    assert_eq!(definition_items[2].description.len(), 1);
    assert_eq!(definition_items[3].description.len(), 1);
    assert_ne!(definition_items[2].source, definition_items[3].source);

    assert_real_query_consumers(document);
}

fn assert_real_query_consumers(document: Document) {
    let query = ResolvedContent {
        label: "c03(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let outline = mant_query::build_outline_projection(&query, EntryProjection::All, None)
        .expect("the actual outline consumer accepts native EntryFacts");
    let outline_json = serde_json::to_string(&outline).unwrap();
    assert!(outline_json.contains("--output"));
    assert!(outline_json.contains("-o=FILE"));

    let explanation = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "-o".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .expect("the actual explanation consumer accepts native EntryFacts");
    assert_eq!(explanation.total, 1);
    assert_eq!(explanation.returned, 1);
    let evidence = &explanation.evidence[0];
    let entry = evidence.entry.as_ref().expect("semantic facts retained");
    assert_eq!(entry.names, ["--output", "-o", "-O"]);
    assert_eq!(
        entry
            .forms
            .iter()
            .map(|form| mant_ir::inline_plain_text(
                explanation.content_projection.as_ref().unwrap().content(),
                form
            ))
            .collect::<Vec<_>>(),
        ["--output", "-o=FILE", "-O"]
    );
    assert!(evidence.bases.iter().any(|basis| matches!(
        basis,
        EvidenceBasis::Name { matches }
            if matches.iter().any(|matched| matched.name == "-o")
    )));
    assert_eq!(entry.name_bindings.len(), 3);

    let empty = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "--empty".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .expect("the empty native owner remains independently queryable");
    assert_eq!(empty.total, 1);
    let duplicate = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "--same".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .expect("duplicate native owners remain independently queryable");
    assert_eq!(duplicate.total, 2);
}

#[test]
fn mdoc_multiple_labels_bind_native_markup_to_exact_forms() {
    // The exact source was run through the pinned reference before this
    // assertion. `mdoc_term.c::termp_it_pre` preserves both Fl occurrences
    // around the authored separator.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "labels.1",
            b".Dd September 21, 2026\n.Dt LABELS 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Fl b\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("labels.1", &bundle, InputFormat::Mdoc)
        .expect("native label evidence lowers to semantic IR");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("tag list retained")
    };
    let facts = items[0].entry.as_ref().expect("option facts discovered");
    assert_eq!(facts.names, ["-a", "-b"]);
    assert_eq!(facts.forms.len(), 2);
    assert_eq!(
        facts
            .forms
            .iter()
            .map(|form| {
                document
                    .content()
                    .entry_form(EntryOwner::Definition(&items[0]), form)
                    .unwrap()
                    .map(|form| mant_ir::inline_plain_text(document.content(), &form))
                    .unwrap()
            })
            .collect::<Vec<_>>(),
        ["-a", "-b"]
    );
    assert_eq!(facts.name_bindings.len(), 2);
    assert!(
        facts.name_bindings.iter().all(|binding| {
            binding.evidence == EntryNameEvidence::NativeMarkup && binding.occurrences.len() == 1
        }),
        "{facts:#?}"
    );
}

#[test]
fn man_marker_source_evidence_controls_list_kind_and_sequence_merging() {
    // The exact input was run through the pinned reference first. Pinned
    // `man_term.c::pre_IP` renders each authored marker; only the named roff
    // bullet is structural evidence, while consecutive ordinals form one run.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "markers.1",
            b".TH MARKERS 1\n.SH STEPS\n.IP \\(bu\nBullet.\n.IP \"*\"\nStar.\n.IP 3.\nThird.\n.IP 4.\nFourth.\n.IP 9.\nNinth.\n.IP \"(1)\"\nParenthesized.\n.IP 1.\nDot.\n.IP 2)\nParen.\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("markers.1", &bundle, InputFormat::Man)
        .expect("man marker evidence lowers without text guessing");
    assert!(matches!(
        &document.sections[0].blocks[0],
        Block::List {
            kind: ListKind::Bullet,
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[1],
        Block::DefinitionList { items, .. } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[2],
        Block::List {
            kind: ListKind::Ordered { start: Some(3) },
            items,
            ..
        } if items.len() == 2
    ));
    assert!(matches!(
        &document.sections[0].blocks[3],
        Block::List {
            kind: ListKind::Ordered { start: Some(9) },
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[4],
        Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[5],
        Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[6],
        Block::List {
            kind: ListKind::Ordered { start: Some(2) },
            items,
            ..
        } if items.len() == 1
    ));
}

#[test]
fn tp_width_tq_boundary_and_rs_continuation_survive_lowering() {
    // Each fragment was run through the pinned reference first. The native
    // path follows `pre_TP` for width/tag separation, keeps TQ independent
    // from marker lists, and treats the sibling RS produced by `blk_exp` as
    // content of the preceding ordered item.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "boundaries.1",
            b".TH X 1\n.SH D\n.TP 4\n\\(bu\nBULLET\n.TP\n\\(bu\n.TQ\nALIAS\nBODY\n.IP 1.\nONE\n.RS\ncontinuation\n.RE\n.IP 2.\nTWO\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("boundaries.1", &bundle, InputFormat::Man)
        .expect("native marker boundaries lower to semantic IR");
    let blocks = &document.sections[0].blocks;
    assert!(matches!(
        &blocks[0],
        Block::List {
            kind: ListKind::Bullet,
            items,
            ..
        } if items.len() == 2
    ));
    assert!(matches!(
        &blocks[1],
        Block::DefinitionList { items, .. }
            if mant_ir::inline_plain_text(document.content(), &items[0].terms[0]) == "ALIAS"
    ));
    let Block::List {
        kind: ListKind::Ordered { start: Some(1) },
        items,
        ..
    } = &blocks[2]
    else {
        panic!("RS-separated ordinals remain one ordered list: {blocks:#?}")
    };
    assert_eq!(items.len(), 2);
    let first_text = items[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => {
                Some(mant_ir::inline_plain_text(document.content(), children))
            }
            _ => None,
        })
        .collect::<String>();
    assert!(first_text.contains("ONE"), "{first_text:?}");
    assert!(first_text.contains("continuation"), "{first_text:?}");
}

#[test]
fn nested_rs_list_keeps_outer_ordinal_state_and_item_ownership() {
    // The exact source was run through the pinned reference first. The RS
    // subtree owns its local bullet state, while the outer 1./2. sequence and
    // the nested list's attachment to item 1 remain intact.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "nested-rs.1",
            b".TH X 1\n.SH D\n.IP 1.\nONE\n.RS\n.IP \\(bu\nNESTED\n.RE\n.IP 2.\nTWO\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("nested-rs.1", &bundle, InputFormat::Man)
        .expect("nested RS list lowers with scoped marker state");
    let Block::List {
        kind: ListKind::Ordered { start: Some(1) },
        items,
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("outer ordered list retained: {:#?}", document.sections)
    };
    assert_eq!(items.len(), 2);
    assert!(items[0].blocks.iter().any(|block| matches!(
        block,
        Block::List {
            kind: ListKind::Bullet,
            items,
            ..
        } if items.len() == 1
    )));
}

#[test]
fn independent_mdoc_lists_keep_their_container_boundaries() {
    // This exact source was run through the pinned reference first. The two
    // Bl/El containers remain independent even though their visible bullets
    // are adjacent in terminal output.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "separate-lists.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh D\n.Bl -bullet\n.It\nONE\n.El\n.Bl -bullet -compact\n.It\nTWO\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("separate-lists.1", &bundle, InputFormat::Mdoc)
        .expect("authored mdoc containers remain distinct");
    assert_eq!(document.sections[0].blocks.len(), 2, "{document:#?}");
    assert!(matches!(
        &document.sections[0].blocks[0],
        Block::List { compact: false, items, .. } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[1],
        Block::List { compact: true, items, .. } if items.len() == 1
    ));
}

#[test]
fn one_native_form_keeps_hint_evidence_without_widening_names() {
    // The exact source was run through the pinned reference first. Native Fl
    // evidence on both sides of an authored slash remains two occurrences
    // inside one form.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "hint-runs.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh D\n.Bl -tag\n.It Fl a No / Fl b\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("hint-runs.1", &bundle, InputFormat::Mdoc)
        .expect("multiple hint runs reach EntryFacts");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("tag list retained")
    };
    let facts = items[0].entry.as_ref().expect("option facts discovered");
    assert_eq!(facts.names, ["-a"]);
    assert_eq!(facts.forms.len(), 1);
    assert!(facts.name_bindings.iter().all(|binding| {
        binding.evidence == EntryNameEvidence::NativeMarkup && binding.occurrences.len() == 1
    }));
}

#[test]
fn literal_separator_definition_keeps_its_term() {
    // The exact source was run through the pinned reference first; the pipe
    // is a literal label rather than a declaration separator without sides.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("operator.1", b".TH X 1\n.SH D\n.TP\n|\nBODY\n".to_vec())
        .unwrap();
    let document = project_native_manual("operator.1", &bundle, InputFormat::Man)
        .expect("literal operator lowers as a definition term");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("definition list retained")
    };
    assert_eq!(
        mant_ir::inline_plain_text(document.content(), &items[0].terms[0]),
        "|"
    );
}

#[test]
fn shared_declaration_grammar_distinguishes_arguments_from_aliases() {
    // This exact source was run through the pinned UTF-8/78 reference first.
    // `man_term.c::pre_alternate` can put a comma in its own styled atom, but
    // that formatter boundary does not decide whether it separates aliases.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "form-grammar.1",
            b".TH X 1\n.SH OPTIONS\n.TP\n.BR --set=KEY , VALUE\nBODY\n.TP\n.B --set=KEY,VALUE\nSECOND\n.TP\n.BR -a , --all\nTHIRD\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("form-grammar.1", &bundle, InputFormat::Man)
        .expect("native structural forms use the shared declaration grammar");
    let items = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => items.first(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 3, "{document:#?}");
    let forms = items
        .iter()
        .map(|item| {
            let facts = item.entry.as_ref().expect("option facts");
            facts
                .forms
                .iter()
                .map(|form| {
                    document
                        .content()
                        .entry_form(EntryOwner::Definition(item), form)
                        .unwrap()
                        .map(|form| mant_ir::inline_plain_text(document.content(), &form))
                        .expect("form resolves")
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        forms,
        [
            vec!["--set=KEY,VALUE".to_owned()],
            vec!["--set=KEY,VALUE".to_owned()],
            vec!["-a".to_owned(), "--all".to_owned()],
        ]
    );
}

#[test]
fn mdoc_native_kinds_nesting_and_targets_survive_ir_lowering() {
    // Reference output and the corresponding
    // `mdoc_term.c::termp_bl_pre/termp_it_pre` path were inspected before
    // adding this assertion; `tag.c::tag_move_id` owns Tg attachment.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "c03.1",
            br".Dd September 21, 2026
.Dt C03 1
.Os
.Sh OPTIONS
.Bl -tag -compact
.It Fl o Ar file
Write file.
.Tg item-target
.It Fl q
.Bl -enum
.It
Nested one.
.It
Nested two.
.El
.El
"
            .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("c03.1", &bundle, InputFormat::Mdoc)
        .expect("mdoc structure lowers to semantic IR");
    let Block::DefinitionList { items, compact, .. } = &document.sections[0].blocks[0] else {
        panic!("tag list retained")
    };
    assert!(*compact);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["-o"]);
    assert_eq!(items[1].entry.as_ref().unwrap().names, ["-q"]);
    let Block::List {
        kind,
        items: nested,
        ..
    } = &items[1].description[0]
    else {
        panic!("nested enum retained inside its owning item")
    };
    assert_eq!(*kind, ListKind::Ordered { start: Some(1) });
    assert_eq!(nested.len(), 2);
    assert!(
        items[1].terms[0].iter().any(
            |inline| matches!(inline, Inline::Anchor { id, .. } if id.as_str() == "item-target")
        )
    );
}
