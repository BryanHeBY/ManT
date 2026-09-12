//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

#[test]
fn lowers_tbl_and_eqn_payloads_into_structured_blocks() {
    let path = temporary_source(
        "table-equation",
        ".TH PAYLOAD 1\n.SH TABLE\n.TS\ntab(|);\nl r.\nleft|right\n.TE\n\
         .SH EQUATION\n.EQ\nx + {width over 2}\n.EN\n",
    );

    let document = parse_manual_source(&path).expect("lower table and equation");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::Table { ref rows, .. } if rows.len() == 1 && rows[0].cells.len() == 2
    ));
    assert!(matches!(
        document.sections[1].blocks[0],
        Block::Equation { ref value, .. } if value == "x + width / 2"
    ));
}

#[test]
fn large_tbl_rows_scale_without_changing_their_topology() {
    const ROW_COUNT: usize = 2_048;
    let mut source = String::from(".TH TABLE-SCALE 7\n.SH TABLE\n.TS\nl l.\n");
    for index in 0..ROW_COUNT {
        writeln!(source, "left {index}\tright {index}").expect("append table row");
    }
    source.push_str(".TE\n");

    let document = parse_manual_bytes(std::path::Path::new("table-scale.7"), source.as_bytes())
        .expect("lower large table");

    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("large tbl input must remain one table");
    };
    assert_eq!(rows.len(), ROW_COUNT);
    assert!(matches!(
        rows.first().and_then(|row| row.cells.first()),
        Some(mant_ir::TableCell { blocks, .. })
            if matches!(blocks.as_slice(), [Block::Paragraph { children, .. }]
                if inline_text(children) == "left 0")
    ));
    assert!(matches!(
        rows.last().and_then(|row| row.cells.get(1)),
        Some(mant_ir::TableCell { blocks, .. })
            if matches!(blocks.as_slice(), [Block::Paragraph { children, .. }]
                if inline_text(children) == format!("right {}", ROW_COUNT - 1))
    ));
}

#[test]
fn keeps_inline_equations_in_macro_arguments_and_filled_prose() {
    let document = parse_manual_bytes(
        std::path::Path::new("inline-equation.7"),
        b".TH EQNPROBE2 7\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\n.TP\n.BR Dp\\~ \"$dx sub 1 ~ ldots ~ dx sub n$\"\nDraw a polygon with,\nfor $i = 1 , ldots , n + 1$,\nits vertex.\n",
    )
    .expect("lower inline equations");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "expected one definition list: {:?}",
            document.sections[0].blocks
        );
    };
    let [item] = items.as_slice() else {
        panic!("expected one equation definition");
    };
    assert_eq!(inline_text(&item.terms[0]), "Dp dx _ 1 ... dx _ n");
    let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
        panic!("expected one filled description: {:?}", item.description);
    };
    assert_eq!(
        inline_text(children),
        "Draw a polygon with, for i = 1 , ... , n + 1, its vertex."
    );
    assert!(
        children
            .iter()
            .any(|child| matches!(child, Inline::Code { value } if value == "i = 1 , ... , n + 1"))
    );
}

#[test]
fn normalizes_inline_equations_retained_as_tbl_cell_text() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-inline-equation.3"),
        b".TH TABLE-EQN 3\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl l.\n%0%\tfor values in % [ 0 , ~pi over 2 ]%\n.TE\n",
    )
    .expect("lower table equations");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected equation table");
    };
    let [left, right] = rows[0].cells.as_slice() else {
        panic!("expected two cells");
    };
    let [Block::Paragraph { children: left, .. }] = left.blocks.as_slice() else {
        panic!("expected left paragraph");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = right.blocks.as_slice()
    else {
        panic!("expected right paragraph");
    };
    assert!(matches!(left.as_slice(), [Inline::Code { value }] if value == "0"));
    assert_eq!(inline_text(right), "for values in [ 0 , π / 2 ]");
    assert!(
        right
            .iter()
            .any(|child| matches!(child, Inline::Code { .. }))
    );
}

#[test]
fn preserves_tbl_rows_across_interleaved_comments_and_text_blocks() {
    let source = b".TH COMMENTED-TABLE 1\n.SH TABLE\n.TS\nl l.\na\t1\n.\\\" disabled text block T{\n.\\\" ignored\n.\\\" T}\nb\t2\nc\t3\nT{\n.BR d (1)\nT}\t4\ne\t5\n.TE\n";
    let document = parse_manual_bytes(std::path::Path::new("commented-table.1"), source)
        .expect("lower commented table");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(rows.len(), 5);
    let first_cells = rows
        .iter()
        .map(|row| match row.cells[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            cells => panic!("expected one paragraph per table cell: {cells:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(first_cells, ["a", "b", "c", "d(1)", "e"]);
}

#[test]
fn keeps_tbl_vertical_span_markers_out_of_visible_cells() {
    let document = parse_manual_bytes(
        std::path::Path::new("vertical-table-span.1"),
        b".TH VERTICAL-TABLE-SPAN 1\n.SH ATTRIBUTES\n.TS\nl l l.\nInterface\tAttribute\tValue\nT{\n.BR demo (1)\nT}\tThread safety\tMT-Safe\n\\^\tAsync-signal safety\tAS-Unsafe\n\\^\tAsync-cancel safety\tAC-Unsafe\n.TE\n",
    )
    .expect("lower vertical table span");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[1].cells[0].row_span, 3);
    assert!(rows[2].cells[0].blocks.is_empty());
    assert!(rows[3].cells[0].blocks.is_empty());
}

#[test]
fn preserves_tbl_rows_nested_in_unfilled_mdoc_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("unfilled-table.7"),
        b".Dd August 19, 2026\n.Dt UNFILLED-TABLE 7\n.Os\n.Sh DESCRIPTION\n\
.Bd -unfilled -offset indent\n.TS\ntab(@);\nl l.\nleft@right\nnext@value\n.TE\n.Ed\n",
    )
    .expect("lower table nested in an unfilled display");

    let table = document.sections[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("nested table must remain structured");
    assert_eq!(table.len(), 2);
    assert_eq!(table[0].cells.len(), 2);
    assert!(
        document.sections[0].blocks.iter().all(
            |block| !matches!(block, Block::Preformatted { children, .. } if children.is_empty())
        ),
        "the surrounding display must not leave an empty placeholder"
    );
}

#[test]
fn tbl_text_blocks_recover_complete_mdoc_default_name_semantics() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-text-block.3"),
        b".Dd August 19, 2026\n.Dt TABLE-TEXT-BLOCK 3\n.Os\n\
.Sh NAME\n.Nm table-text-block\n.Nd test tbl text blocks\n\
.Sh ATTRIBUTES\n.TS\nallbox;\nl l.\nInterface\tValue\n\
T{\n.Nm\nT}\tMT-Safe\n.TE\n",
    )
    .expect("lower tbl text blocks");

    let Block::Table { rows, .. } = &document.sections[1].blocks[0] else {
        panic!("expected attributes table");
    };
    let [Block::Paragraph { children, .. }] = rows[1].cells[0].blocks.as_slice() else {
        panic!("expected native table cell");
    };
    assert_eq!(inline_text(children), "table-text-block");
    assert!(matches!(children.as_slice(), [Inline::Strong { .. }]));
}

#[test]
fn tbl_text_blocks_recover_complete_man_font_requests() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-text-alternation.7"),
        b".TH TABLE-TEXT-ALTERNATION 7\n.SH DESCRIPTION\n.TS\nl l.\nT{\n\
.BI \\[aq] s1 \\[aq] s2 \\[aq]\nT}\tT{\n\
.I s1\nproduces the same formatted output as\n.IR s2 .\nT}\n.TE\n",
    )
    .expect("lower alternating man macros inside a tbl text block");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a structured table");
    };
    let [left, right] = rows[0].cells.as_slice() else {
        panic!("expected both native table cells");
    };
    let [Block::Paragraph { children: left, .. }] = left.blocks.as_slice() else {
        panic!("expected a left table-cell paragraph");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = right.blocks.as_slice()
    else {
        panic!("expected a right table-cell paragraph");
    };
    assert_eq!(inline_text(left), "'s1's2'");
    assert_eq!(
        inline_text(right),
        "s1 produces the same formatted output as s2."
    );
    assert!(
        right
            .iter()
            .any(|inline| matches!(inline, Inline::Emphasis { .. }))
    );
}

#[test]
fn mixed_mdoc_table_requests_keep_raw_operands_without_recovery_diagnostics() {
    for body in [
        ".Cm TOKENA\n.Pp\nTOKENB",
        ".Em TOKENA\n.Bl -bullet\n.It\nTOKENB\n.El",
        ".Cm TOKENA\n.Bd -literal\nTOKENB\n.Ed",
    ] {
        let source = format!(
            ".Dd September 6, 2026\n.Dt MIXED 1\n.Os\n.Sh DESCRIPTION\n.TS\nl.\nT{{\n{body}\nT}}\n.TE\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let text = mant_render::render_query_text(&query);
        assert!(
            text.contains("TOKENA") && text.contains("TOKENB"),
            "{source}: {text}"
        );
        assert_eq!(text.matches("TOKENA").count(), 1, "{text}");
        assert_eq!(text.matches("TOKENB").count(), 1, "{text}");
        assert!(
            query
                .document
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .all(|d| d.code.as_deref() != Some("manual.unhandled-table-text-block"))
        );
    }
}

#[test]
fn mixed_table_requests_never_replace_complete_native_cell_content() {
    for font in ["B", "I", "BR"] {
        for paragraph in ["PP", "TP"] {
            for apostrophe in [false, true] {
                let control = if apostrophe { "'" } else { "." };
                let source = format!(
                    ".TH MIXED 1\n.SH DESCRIPTION\n.TS\nl l.\nT{{\n{control}{font} TOKENA\n{control}{paragraph}\nTOKENB\nT}}\tNEIGHBOR\n.TE\n"
                );
                let document =
                    parse_manual_bytes(std::path::Path::new("mixed-table.1"), source.as_bytes())
                        .unwrap();
                let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
                    panic!("table structure must survive {source}")
                };
                let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
                    panic!("table cell")
                };
                let text = inline_text(children);
                assert!(
                    text.contains("TOKENA") && text.contains("TOKENB"),
                    "{source}: {text}"
                );
                assert!(text.find("TOKENA") < text.find("TOKENB"));
                assert!(!text.contains("NEIGHBOR"));
                assert_eq!(text.matches("TOKENA").count(), 1);
                assert_eq!(text.matches("TOKENB").count(), 1);
                assert!(
                    document
                        .diagnostics
                        .iter()
                        .all(|d| d.code.as_deref() != Some("manual.unhandled-table-text-block"))
                );
            }
        }
    }
}

#[test]
fn table_text_blocks_recover_complete_inline_macro_semantics() {
    for (header, request, expected) in [
        (
            ".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Fl Fl help",
            "--help",
        ),
        (
            ".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Cm TOKENA Ns : Ns Ar TOKENB",
            "TOKENA:TOKENB",
        ),
        (
            ".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Oo Fl a Oc No TOKENA",
            "[-a] TOKENA",
        ),
        (
            ".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Sm off\n.Cm TOKENA\n.Ar TOKENB\n.Sm on\n.No TOKENC",
            "TOKENATOKENB TOKENC",
        ),
        (
            ".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Oo\n.Fl a\n.Oc\n.No TOKENA",
            "[-a] TOKENA",
        ),
        (".TH PROBE 1\n.SH DESCRIPTION", ".B Fl", "Fl"),
        (".TH PROBE 1\n.SH DESCRIPTION", ".I Ar Ns Op", "Ar Ns Op"),
    ] {
        let table_source = format!("{header}\n.TS\nl.\nT{{\n{request}\nT}}\n.TE\n");
        let table =
            parse_manual_bytes(std::path::Path::new("table.1"), table_source.as_bytes()).unwrap();
        let [Block::Table { rows, .. }] = table.sections[0].blocks.as_slice() else {
            panic!("expected table: {table:#?}")
        };
        let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("expected table cell paragraph")
        };
        let actual = inline_text(children);
        assert_eq!(actual, expected, "{request}");
    }
}

#[test]
fn table_text_blocks_keep_native_request_operands_out_of_visible_content() {
    for request in [".ll 50n\nBODY", ".po 0n\nBODY", ".br\nBODY"] {
        let source = format!(".TH PROBE 1\n.SH DESCRIPTION\n.TS\nl.\nT{{\n{request}\nT}}\n.TE\n");
        let document = parse_manual_bytes(
            std::path::Path::new("native-table-request.1"),
            source.as_bytes(),
        )
        .unwrap();
        let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected table: {document:#?}")
        };
        let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("expected table cell paragraph")
        };
        assert_eq!(inline_text(children), "BODY", "{request}");
    }
}

#[test]
fn tbl_text_blocks_preserve_the_tiocpkt_control_key_spacing() {
    let document = parse_manual_bytes(
        std::path::Path::new("tiocpkt-table.2const"),
        b".TH TIOCPKT 2const\n.SH DESCRIPTION\n.TS\nl.\nT{\n.BR \\[ha]S / \\[ha]Q .\nT}\n.TE\n",
    )
    .expect("lower the TIOCPKT tbl control-key witness");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected one table paragraph");
    };
    assert_eq!(inline_text(children), "^S/^Q.");
}

#[test]
fn table_text_blocks_recover_mdoc_inline_structure() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-mdoc-requests.8"),
        b".Dd August 19, 2026\n.Dt TABLE-MDOC-REQUESTS 8\n.Os\n.Sh DESCRIPTION\n\
.TS\ntab(@);\nl l.\nT{\n.Cm sip Ar addr Ns Op / Ns Ar mask\nT}@T{\n\
bitwise and of the address with\n.Ar mask\nequals\n.Ar addr .\n.Ar addr\n\
can be an IPv4 or IPv6 address.\nT}\n.TE\n",
    )
    .expect("lower mdoc operands in table text blocks");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a structured table");
    };
    let [left, right] = rows[0].cells.as_slice() else {
        panic!("expected two table cells");
    };
    let [Block::Paragraph { children: left, .. }] = left.blocks.as_slice() else {
        panic!("expected selector cell");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = right.blocks.as_slice()
    else {
        panic!("expected description cell");
    };
    assert_eq!(inline_text(left), "sip addr[/mask]");
    assert_eq!(
        inline_text(right),
        "bitwise and of the address with mask equals addr. addr can be an IPv4 or IPv6 address."
    );
    assert!(
        left.iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
    assert!(
        right
            .iter()
            .any(|inline| matches!(inline, Inline::Emphasis { .. }))
    );
}

#[test]
fn decodes_named_characters_inside_equations() {
    let document = parse_manual_bytes(
        std::path::Path::new("equation-characters.1"),
        b".TH EQUATION-CHARACTERS 1\n.SH EQUATION\n.EQ\n\\[*p] \\[mi] x\n.EN\n",
    )
    .expect("lower equation characters");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::Equation { ref value, .. } if value == "\u{03c0} \u{2212} x"
    ));
}

#[test]
fn lowers_every_mdoc_column_list_cell() {
    let document = parse_manual_bytes(
        std::path::Path::new("columns.3"),
        b".Dd August 19, 2026\n.Dt COLUMNS 3\n.Os\n.Sh DESCRIPTION\n\
.Bl -column name type description\n.It Dv CLSET_TIMEOUT Ta \"struct timeval *\" Ta \"set total timeout\"\n.El\n",
    )
    .expect("lower mdoc column list");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected column list to lower as a table");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 3);
    let rendered = rows[0]
        .cells
        .iter()
        .map(|cell| match cell.blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            blocks => panic!("expected one paragraph per cell, got {blocks:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rendered,
        ["CLSET_TIMEOUT", "struct timeval *", "set total timeout"]
    );
}

#[test]
fn native_tbl_diagnostics_do_not_invent_unparsed_source_cells() {
    let document = parse_manual_bytes(
        std::path::Path::new("unexpanded-table-cell.7"),
        b".TH UNEXPANDED-TABLE-CELL 7\n.SH DESCRIPTION\n.TS\nl l.\n1\t\\*[unknown-label]\n.TE\n",
    )
    .expect("lower unresolved formatter string in a table cell");

    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a structured table");
    };
    // CVS tbl consumes the malformed string escape as part of the second
    // field and exposes only the surviving first cell. Lowering must retain
    // that executed native topology rather than recreating a source-looking
    // second cell from bytes that no formatter rendered.
    assert_eq!(rows[0].cells.len(), 1);
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected one native table-cell paragraph");
    };
    assert_eq!(inline_text(children), "1");
    assert!(
        !document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.unexpanded-table-cell")
        })
    );
}
