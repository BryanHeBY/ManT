//! Existing regressions grouped by layout behavior; expected values remain independent.
use super::*;

#[test]
fn excessive_display_and_list_offsets_are_bounded_without_losing_content() {
    struct LayoutBounds;
    impl<'ir> Visit<'ir> for LayoutBounds {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Some(layout) = mant_ir::geometry::block_layout(block) {
                assert!(layout.indent_columns <= 4096);
            }
            visit::walk_block(self, block);
        }
    }
    for offset in ["65535n", "4096n", "100000i"] {
        for body in [
            ".Bl -bullet\n.It\nCONTENT\n.El",
            ".Bl -tag\n.It NAME\nCONTENT\n.El",
            ".D1 CONTENT",
        ] {
            let source = format!(
                ".Dd September 5, 2026\n.Dt OFFSET 1\n.Os\n.Sh DESCRIPTION\n.Bd -ragged -offset {offset}\n{body}\n.Ed\n"
            );
            let document =
                parse_manual_bytes(std::path::Path::new("offset.1"), source.as_bytes()).unwrap();
            LayoutBounds.visit_document(&document);
            let bounded = document
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some("manual.indentation-limit"));
            // Every listed child has an actual body/display displacement;
            // a body beyond the source limit must also report bounding.
            assert!(bounded);
            assert!(
                mant_engine::query_roff_bytes(source.as_bytes())
                    .map(|query| mant_render::render_query_text(&query).contains("CONTENT"))
                    .unwrap()
            );
        }
    }
}

#[test]
fn unclosed_compact_run_stays_separate_and_resets_at_indent_scope() {
    let path = temporary_source(
        "unclosed-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .PD 0\n\
         --loose\n\
         .TP\n\
         --described\n\
         Own description.\n\
         .TP\n\
         .PD 0\n\
         outer\n\
         .RS\n\
         .TP\n\
         inner\n\
         .PD\n\
         Inner description.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower unclosed compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one outer definition list");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(inline_text(&items[0].terms[0]), "--loose");
    assert!(items[0].description.is_empty());
    assert_eq!(inline_text(&items[1].terms[0]), "--described");
    assert_eq!(inline_text(&items[2].terms[0]), "outer");
    let [
        Block::DefinitionList {
            items: inner_items, ..
        },
    ] = items[2].description.as_slice()
    else {
        panic!("expected one nested definition list");
    };
    assert_eq!(inner_items.len(), 1);
    assert_eq!(inline_text(&inner_items[0].terms[0]), "inner");
    assert!(!inner_items[0].description.is_empty());
}

#[test]
fn preserves_man_synopsis_flow_and_alternating_fonts() {
    let path = temporary_source(
        "man-synopsis-flow",
        ".TH MAN 1\n\
         .SH SYNOPSIS\n\
         .B man\n\
         .RI [\\| \"man options\" \\|]\n\
         .RI [\\|[\\| section \\|]\n\
         .IR page \\ \\|.\\|.\\|.\\|]\\ \\.\\|.\\|.\\&\n\
         .br\n\
         .B man\n\
         .B \\-k\n\
         .RI [\\| \"apropos options\" \\|]\n\
         .I regexp\n\
         \\&.\\|.\\|.\\&\n\
         .br\n\
         .B man\n\
         .BR \\-w \\||\\| \\-W\n\
         .RI [\\| \"man options\" \\|]\n\
         .I page\n\
         \\&.\\|.\\|.\\&\n",
    );

    let document = parse_manual_source(&path).expect("lower man synopsis");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one synopsis paragraph");
    };
    assert_eq!(
        inline_text(children),
        "man [man options] [[section] page ...] ...\n\
         man -k [apropos options] regexp ...\n\
         man -w|-W [man options] page ..."
    );
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak))
            .count(),
        2
    );
    assert!(children.iter().any(
        |node| matches!(node, Inline::Emphasis { children } if inline_text(children) == "man options")
    ));
    assert!(
        children.iter().any(
            |node| matches!(node, Inline::Strong { children } if inline_text(children) == "-w")
        )
    );
    assert!(
        children.iter().any(
            |node| matches!(node, Inline::Strong { children } if inline_text(children) == "-W")
        )
    );
}

#[test]
fn preserves_man_sy_heads_with_body_content_and_inline_fonts() {
    let document = parse_manual_bytes(
        std::path::Path::new("sy-heads.1"),
        b".TH SY-HEADS 1 \"August 17, 2026\"\n\
.SH SYNOPSIS\n\
.SY getent\n\
.RI [ option ]\n\
.I database\n\
.YS\n\
.SH DESCRIPTION\n\
.SY #!\\f[I]interpreter\\f[]\n\
.RI [ optional-arg ]\n\
.YS\n",
    )
    .expect("lower SY heads");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one synopsis paragraph");
    };
    assert_eq!(inline_text(children), "getent [option] database");
    assert!(matches!(
        children.first(),
        Some(Inline::Strong { children }) if inline_text(children) == "getent"
    ));

    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("expected one description paragraph");
    };
    assert_eq!(inline_text(children), "#!interpreter [optional-arg]");
    assert!(matches!(
        children.first(),
        Some(Inline::Strong { children })
            if children.iter().any(|inline| matches!(
                inline,
                Inline::Emphasis { children } if inline_text(children) == "interpreter"
            ))
    ));
    assert!(
        document.diagnostics.is_empty(),
        "{:?}",
        document.diagnostics
    );
}

#[test]
fn keeps_man_synopsis_lines_together_inside_no_fill_examples() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-synopsis.2"),
        b".TH NO-FILL-SYNOPSIS 2\n\
.SH DESCRIPTION\n\
.EX\n\
.SY #!\\f[I]interpreter\\f[]\n\
.RI [ optional-arg ]\n\
.YS\n\
.EE\n",
    )
    .expect("lower synopsis inside example");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill synopsis must remain one preformatted block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "#!interpreter\n[optional-arg]");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
}

#[test]
fn preserves_explicit_blank_rows_inside_no_fill_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-blank-row.7"),
        b".TH NO-FILL-BLANK-ROW 7\n\
.SH EXAMPLE\n\
.EX\n\
first line\n\
\n\
second line\n\
.EE\n",
    )
    .expect("lower no-fill blank row");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\n\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
}

#[test]
fn preserves_zero_width_guard_rows_inside_no_fill_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-zero-width-row.7"),
        b".TH NO-FILL-ZERO-WIDTH-ROW 7\n\
.SH EXAMPLE\n\
.EX\n\
first line\n\
\\&\n\
second line\n\
.EE\n",
    )
    .expect("lower no-fill zero-width row");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\n\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
}

#[test]
fn preserves_lines_inside_font_blocks_nested_in_literal_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("literal-font-block.7"),
        b".Dd August 20, 2026\n\
.Dt LITERAL-FONT-BLOCK 7\n\
.Os\n\
.Sh EXAMPLE\n\
.Bd -literal\n\
.Bf Sy\n\
first line\n\
second line\n\
.Ef\n\
.Ed\n",
    )
    .expect("lower font block inside literal display");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "literal display must remain one preformatted block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
}

#[test]
fn preserves_literal_display_lines_inside_literal_font_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("literal-display-inside-font-block.7"),
        b".Dd August 21, 2026\n\
.Dt LITERAL-DISPLAY-INSIDE-FONT-BLOCK 7\n\
.Os\n\
.Sh EXAMPLE\n\
.Bf Li\n\
.Bd -literal\n\
first line\n\
second line\n\
.Ed\n\
.Ef\n",
    )
    .expect("lower literal display inside literal font block");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "fonted literal display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
}

#[test]
fn preserves_lines_inside_compatible_compact_nested_literal_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("nested-literal-display.7"),
        b".Dd August 21, 2026\n\
.Dt NESTED-LITERAL-DISPLAY 7\n\
.Os\n\
.Sh EXAMPLE\n\
.Bd -literal -compact\n\
first line\n\
.Bd -literal -compact\n\
second line\n\
third line\n\
.Ed\n\
.Ed\n",
    )
    .expect("lower nested literal display");

    let texts = document.sections[0]
        .blocks
        .iter()
        .map(|block| {
            let Block::Preformatted {
                children, layout, ..
            } = block
            else {
                panic!("unexpected nested literal block: {block:?}");
            };
            assert_eq!(layout.indent_columns, 0);
            assert_eq!(layout.spacing_before_lines, 0);
            inline_text(children)
        })
        .collect::<Vec<_>>();
    assert_eq!(texts.join("\n"), "first line\nsecond line\nthird line");
}

#[test]
fn preserves_every_executed_no_fill_blank_row() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-blank-run.7"),
        b".TH NO-FILL-BLANK-RUN 7\n\
.SH EXAMPLE\n\
.EX\n\
first line\n\
\n\
\n\
second line\n\
.EE\n",
    )
    .expect("lower no-fill blank run");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "no-fill display must remain preformatted: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\n\n\nsecond line");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        3
    );
}

#[test]
fn distinguishes_filled_source_wrapping_from_indented_output_lines() {
    let path = temporary_source(
        "filled-line-boundaries",
        concat!(
            ".TH TOOL 1\n",
            ".SH SYNOPSIS\n",
            "tool [first]\n",
            "    [second]\n",
            "    [third]\n",
            ".PP\n",
            "Ordinary source wrapping\n",
            "remains one filled paragraph.\n",
        ),
    );

    let document = parse_manual_source(&path).expect("lower filled line boundaries");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [
        Block::Paragraph {
            children: synopsis, ..
        },
        Block::Paragraph {
            children: prose, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected synopsis and prose paragraphs");
    };
    assert_eq!(
        inline_text(synopsis),
        "tool [first]\n    [second]\n    [third]"
    );
    assert_eq!(
        synopsis
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        2
    );
    assert_eq!(
        inline_text(prose),
        "Ordinary source wrapping remains one filled paragraph."
    );
}

#[test]
fn honours_roff_no_space_line_continuations() {
    let document = parse_manual_bytes(
        std::path::Path::new("line-continuation.1"),
        b".TH LINE-CONTINUATION 1\n\
.SH DESCRIPTION\n\
extsize=\\c\n\
nnnn; multi-\\c\n\
block; (\\c\n\
.BR read (2)\n\
.EX\n\
literal-\\c\n\
continuation\n\
.EE\n",
    )
    .expect("lower no-space line continuations");

    let [
        Block::Paragraph {
            children: prose, ..
        },
        Block::Preformatted {
            children: literal, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected one filled and one no-fill block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(prose), "extsize=nnnn; multi-block; (read(2)");
    assert_eq!(inline_text(literal), "literal-continuation");
}

#[test]
fn keeps_explicit_horizontal_separation_at_a_tight_line_join() {
    let document = parse_manual_bytes(
        std::path::Path::new("motion-continuation.1"),
        b".TH MOTION-CONTINUATION 1\n\
.SH DESCRIPTION\n\
\\h'-04' 1.\\h'+01'\\c\n\
The next line.\n",
    )
    .expect("lower a horizontally spaced continued line");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:?}", document.sections[0].blocks);
    };
    assert_eq!(inline_text(children), " 1. The next line.");
}

#[test]
fn preserves_man_paragraph_distance_between_indented_paragraphs() {
    let path = temporary_source(
        "paragraph-distance",
        ".TH SPACING 1\n\
         .SH OPTIONS\n\
         .IP \"\\fB-a\\fR\" 4\n\
         First.\n\
         .IP \"\\fB-b\\fR\" 4\n\
         Second.\n\
         .PD 0\n\
         .IP \"\\fB-c\\fR\" 4\n\
         Third.\n\
         .IP \"\\fB-d\\fR\" 4\n\
         Fourth.\n\
         .PD\n\
         .IP \"\\fB-e\\fR\" 4\n\
         Fifth.\n",
    );

    let document = parse_manual_source(&path).expect("lower paragraph distance");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, compact, .. }] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected one definition list");
    };
    assert!(!compact);
    assert_eq!(items.len(), 5);
    assert_eq!(
        items
            .iter()
            .map(|item| item.layout.spacing_before_lines)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(0), Some(0), Some(1)]
    );
}

#[test]
fn does_not_duplicate_explicit_space_before_a_transparent_indent() {
    let path = temporary_source(
        "explicit-space-before-indent",
        ".TH SPACING 1\n\
         .SH CONTENT\n\
         Before.\n\
         .sp\n\
         .RS 4\n\
         After.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower explicit indented spacing");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [
        Block::Paragraph { .. },
        Block::VerticalSpace { lines: 1, .. },
        Block::Paragraph { layout, .. },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected prose, one explicit gap, and indented prose");
    };
    assert_eq!(layout.indent_columns, 4);
    assert_eq!(
        layout.spacing_before_lines, 0,
        "the explicit gap must not be repeated as wrapper boundary spacing",
    );
}

#[test]
fn relative_indent_does_not_invent_paragraph_distance() {
    let path = temporary_source(
        "relative-indent-spacing",
        ".TH SPACING 7\n\
         .SH DESCRIPTION\n\
         .PP\n\
         first term\n\
         .RS 4\n\
         First description.\n\
         .RE\n\
         .PP\n\
         second term\n\
         .RS 4\n\
         Second description.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower relative-indent spacing");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [
        Block::Paragraph {
            layout: first_term, ..
        },
        Block::Paragraph {
            layout: first_description,
            ..
        },
        Block::Paragraph {
            layout: second_term,
            ..
        },
        Block::Paragraph {
            layout: second_description,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("expected two terms followed by their indented descriptions");
    };
    assert_eq!(
        (first_term.indent_columns, first_term.spacing_before_lines),
        (0, 0)
    );
    assert_eq!(
        (
            first_description.indent_columns,
            first_description.spacing_before_lines,
        ),
        (4, 0),
        "RS changes indentation without adding paragraph distance",
    );
    assert_eq!(
        (second_term.indent_columns, second_term.spacing_before_lines),
        (0, 1),
        "the following PP still owns the distance between entries",
    );
    assert_eq!(
        (
            second_description.indent_columns,
            second_description.spacing_before_lines,
        ),
        (4, 0),
    );
}

#[test]
fn relative_indent_preserves_child_owned_paragraph_distance() {
    let path = temporary_source(
        "relative-indent-child-spacing",
        ".TH SPACING 7\n\
         .SH DESCRIPTION\n\
         Before.\n\
         .RS 4\n\
         .PP\n\
         Explicit nested paragraph.\n\
         .RE\n",
    );

    let document = parse_manual_source(&path).expect("lower nested paragraph spacing");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::Paragraph { .. }, Block::Paragraph { layout, .. }] =
        document.sections[0].blocks.as_slice()
    else {
        panic!("expected outer prose and one explicitly separated nested paragraph");
    };
    assert_eq!(layout.indent_columns, 4);
    assert_eq!(
        layout.spacing_before_lines, 1,
        "PP inside RS must retain its own paragraph distance",
    );
}

#[test]
fn propagates_nested_no_space_and_preserves_prefix_content() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-space.7"),
        b".Dd August 19, 2026\n.Dt NO-SPACE 7\n.Os\n.Sh DESCRIPTION\n\
.Em Bell Labs Ns -derived\n\
.Ar job Ns s :\n\
.Sm off\n\
.Pf [\\-]ddd Cm \\&. No ddd\n\
.Sm on\n",
    )
    .expect("lower nested no-space macros");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one no-space paragraph");
    };
    assert_eq!(inline_text(children), "Bell Labs-derived jobs: [-]ddd.ddd");
}

#[test]
fn discards_temporary_indent_arguments_without_hiding_the_next_line() {
    let document = parse_manual_bytes(
        std::path::Path::new("temporary-indent.8"),
        b".TH TEMPORARY-INDENT 8\n.SH EXAMPLES\n.ti +8n\nexample% command\n.ti\nexample% other\n",
    )
    .expect("lower temporary indentation requests");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one examples paragraph");
    };
    assert_eq!(inline_text(children), "example% command example% other");
}

#[test]
fn lowers_normalized_ordered_lists_and_literal_displays() {
    let path = temporary_source(
        "normalized",
        ".Dd July 19, 2026\n.Dt NORMALIZED 1\n.Os\n.Sh CONTENT\n\
         .Bl -enum -compact\n.It\nfirst\n.It\nsecond\n.El\n\
         .Bd -literal -offset 6n\nline one\nline two\n.Ed\n",
    );

    let document = parse_manual_source(&path).expect("lower normalized mdoc");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::List {
            kind: mant_ir::ListKind::Ordered { .. },
            compact: true,
            ..
        }
    ));
    assert!(matches!(
        document.sections[0].blocks[1],
        Block::Preformatted { layout, .. } if layout.indent_columns == 6
    ));
}

#[test]
fn keeps_relative_indent_references_inside_man_ip_enumerations() {
    let document = parse_manual_bytes(
        std::path::Path::new("ip-reference-enumeration.1"),
        b".TH IP-REFERENCE-ENUMERATION 1\n.SH NOTES\n\
.IP \" 1.\" 4\nFirst reference\n.RS 4\nfile:///first\n.RE\n\
.IP \" 2.\" 4\nSecond reference\n.RS 4\nfile:///second\n.RE\n\
.IP \" 3.\" 4\nThird reference\n.RS 4\nfile:///third\n.RE\n",
    )
    .expect("lower numbered IP references");

    let notes = &document.sections[0];
    let [
        Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            items,
            ..
        },
    ] = notes.blocks.as_slice()
    else {
        panic!("numbered references must form one ordered list");
    };
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|item| {
        item.blocks.len() == 2
            && item.blocks.iter().all(
                |block| matches!(block, Block::Paragraph { layout, .. } if layout.indent_columns == 1),
            )
    }));
    assert!(SemanticIndex::build(&document).section("notes").is_empty());
}

#[test]
fn keeps_multiline_cells_aligned_after_an_empty_text_block() {
    let source = b".TH EMPTY-TABLE-CELL 7\n.SH TABLE\n.TS\ntab(@);\nl l l.\n\
T{\nT}@T{\nCore\nT}@T{\nProduction-grade, first-class\nT}\n.TE\n";
    let document = parse_manual_bytes(std::path::Path::new("empty-table-cell.7"), source)
        .expect("lower a row beginning with an empty text block");

    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one table");
    };
    let [row] = rows.as_slice() else {
        panic!("expected one table row");
    };
    let values = row
        .cells
        .iter()
        .map(|cell| match cell.blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            [] => String::new(),
            blocks => panic!("unexpected table cell blocks: {blocks:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(values, ["", "Core", "Production-grade, first-class"]);
}

#[test]
fn mdoc_header_references_require_both_synopsis_and_line_start_for_include() {
    for section in ["DESCRIPTION", "SYNOPSIS"] {
        for (request, inline) in [(".In stdio.h", false), (".No See In stdio.h", true)] {
            for table in [false, true] {
                let body = if table {
                    format!(".TS\nl.\nT{{\n{request}\nT}}\n.TE")
                } else {
                    request.to_owned()
                };
                let source =
                    format!(".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh {section}\n{body}\n");
                let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
                let text = mant_render::render_query_text(&query);
                assert!(text.contains("<stdio.h>"), "{source}: {text}");
                assert_eq!(
                    text.contains("#include"),
                    section == "SYNOPSIS" && !inline,
                    "{source}: {text}"
                );
            }
        }
    }
}

#[test]
fn carries_mdoc_spacing_state_into_display_lines() {
    let document = parse_manual_bytes(
        std::path::Path::new("display-spacing.8"),
        b".Dd August 24, 2026\n.Dt DISPLAY-SPACING 8\n.Os\n.Sh FORMAT\n\
.Sm off\n.D1 Ar name : uid : gid\n.Sm on\n",
    )
    .expect("lower display-scoped mdoc spacing controls");

    let Block::Preformatted { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one display line");
    };
    assert_eq!(inline_text(children), "name:uid:gid");
}

#[test]
fn carries_mdoc_spacing_state_across_list_item_boundaries() {
    let document = parse_manual_bytes(
        std::path::Path::new("list-spacing.8"),
        b".Dd August 19, 2026\n.Dt LIST-SPACING 8\n.Os\n.Sh COMMANDS\n\
.Bl -tag -width Ds\n.Sm off\n.It Ic O Ar device\n.Sm on\n.It Ic done\nFinished.\n.El\n",
    )
    .expect("lower list-scoped mdoc spacing controls");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a command definition list");
    };
    assert_eq!(inline_text(&items[0].terms[0]), "Odevice");
    assert_eq!(inline_text(&items[1].terms[0]), "done");
}

#[test]
fn carries_mdoc_spacing_state_out_of_nested_synopsis_enclosures() {
    let document = parse_manual_bytes(
        std::path::Path::new("nested-synopsis-spacing.8"),
        b".Dd August 19, 2026\n.Dt NESTED-SYNOPSIS-SPACING 8\n.Os\n.Sh SYNOPSIS\n\
.Nm demo\n.Sm off\n.Oo Fl m\\~\n.Ar memory\n.Sm on\n.Oc\n\
.Op Fl o Ar variable Ns Cm = Ns Ar value\n.Ar name\n",
    )
    .expect("lower nested synopsis spacing transitions");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected synopsis paragraph");
    };
    assert_eq!(
        inline_text(children),
        "demo [-m memory] [-o variable=value] name"
    );
}

#[test]
fn adjacent_no_fill_regions_scale_without_changing_their_topology() {
    const REGION_COUNT: usize = 2_048;
    let mut source = String::from(".TH NO-FILL-SCALE 7\n.SH EXAMPLE\n");
    for index in 0..REGION_COUNT {
        writeln!(source, ".nf\nline {index}\n.fi").expect("append no-fill region");
    }

    let document = parse_manual_bytes(std::path::Path::new("no-fill-scale.7"), source.as_bytes())
        .expect("lower adjacent no-fill regions");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "adjacent regions must remain one preformatted block: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        REGION_COUNT - 1
    );
    assert!(inline_text(children).starts_with("line 0\nline 1\n"));
    assert!(
        inline_text(children).ends_with(&format!("line {}", REGION_COUNT - 1)),
        "last no-fill region must remain visible"
    );
}

#[test]
fn preserves_man_paragraph_and_heading_distance_as_one_layout_model() {
    let path = temporary_source(
        "vertical-layout",
        ".TH SPACING 1\n\
         .SH FIRST\n\
         First paragraph.\n\
         .PP\n\
         Second paragraph.\n\
         .SS CHILD\n\
         Child body.\n\
         .PD 0\n\
         .SS COMPACT\n\
         Compact child.\n\
         .SH NEXT\n\
         Next body.\n\
         .PD\n\
         .SH FINAL\n\
         Final body.\n",
    );

    let document = parse_manual_source(&path).expect("lower vertical layout");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [first, next, final_section] = document.sections.as_slice() else {
        panic!("expected three top-level sections");
    };
    assert_eq!(first.spacing_before_lines, 0);
    let [Block::Paragraph { .. }, Block::Paragraph { layout, .. }] = first.blocks.as_slice() else {
        panic!("expected two semantic paragraphs");
    };
    assert_eq!(layout.spacing_before_lines, 1);

    let [child, compact] = first.children.as_slice() else {
        panic!("expected two subsections");
    };
    assert_eq!(child.spacing_before_lines, 1);
    assert_eq!(compact.spacing_before_lines, 0);
    assert_eq!(next.spacing_before_lines, 0);
    assert_eq!(final_section.spacing_before_lines, 1);
}

#[test]
fn preserves_mdoc_paragraph_and_heading_distance() {
    let path = temporary_source(
        "mdoc-vertical-layout",
        ".Dd July 19, 2026\n\
         .Dt SPACING 1\n\
         .Os\n\
         .Sh FIRST\n\
         First paragraph.\n\
         .Pp\n\
         Second paragraph.\n\
         .Ss CHILD\n\
         Child body.\n",
    );

    let document = parse_manual_source(&path).expect("lower mdoc vertical layout");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [first] = document.sections.as_slice() else {
        panic!("expected one top-level section");
    };
    assert_eq!(first.spacing_before_lines, 1);
    assert!(matches!(
        first.blocks.get(1),
        Some(Block::VerticalSpace { lines: 1, .. })
    ));
    assert_eq!(first.children[0].spacing_before_lines, 1);
}
