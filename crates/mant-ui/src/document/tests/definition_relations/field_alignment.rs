//! Shared fields retain labels and position the body from the printed end.

use super::*;

const HEADER: &str = concat!(
    ".TH PROBE 1 \"November 18, 2026\" Test Manual\n.SH NAME\n",
    "probe \\- exercise descriptors\n.SH DESCRIPTION\n",
);

#[test]
fn aligned_option_padding_preserves_terminal_cell_style() {
    let expected = theme::style(theme::StyleRole::Parameter).fg;
    for padding in [0, 1, 8] {
        // CVS keep the HEAD and BODY together for a fitting .IP label.
        let source = format!(
            ".TH PROBE 1\n.SH OPTIONS\n.IP \"--foo{}\" 24\nBODYWORD\n",
            " ".repeat(padding)
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let view = DocumentView::new(&query);
        for width in [16, 78] {
            let rendered = view.render(width);
            let hit = &rendered.search("--foo")[0];
            let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
            let mut buffer = Buffer::empty(area);
            Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
            for (column, glyph) in (hit.start_column..hit.end_column).zip(["-", "-", "f", "o", "o"])
            {
                let cell = &buffer[(column.try_into().unwrap(), hit.row.try_into().unwrap())];
                assert_eq!(cell.symbol(), glyph);
                assert_eq!(
                    cell.style().fg,
                    expected,
                    "{padding} spaces at width {width}"
                );
            }
            assert_eq!(hit.end_column - hit.start_column, 5);
        }
    }
}

#[test]
fn repeated_reference_occurrences_survive_replaced_field_padding() {
    const PREFIX: &str = concat!(
        ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n",
        ".Nd probe page\n.Sh DESCRIPTION\n",
    );
    for tail in [
        "",
        " Ns No \" \"",
        " Ns No \"        \"",
        r" Ns No \~",
        "\n.br",
    ] {
        // CVS termp_xr_pre writes printf(3) before the tag's trailing padding.
        // HTML keeps two Xr occurrences. NBSP stays in the label, and .br
        // closes its row before either BODY.
        let mut source = PREFIX.to_owned();
        for body in ["FIRSTBODY", "SECONDBODY"] {
            use std::fmt::Write as _;
            writeln!(
                source,
                ".Bl -tag -width 24n\n.It Xo\n.Xr printf 3{tail}\n.Xc\n{body}\n.El"
            )
            .unwrap();
        }
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
        let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
            .unwrap()
            .into();
        assert_eq!(query.document, restored.document);
        let view = DocumentView::new(&restored);
        let references = view
            .references
            .iter()
            .filter(|r| r.label == "printf(3)")
            .collect::<Vec<_>>();
        let [first, second] = references.as_slice() else {
            panic!("two independent reference occurrences");
        };
        assert_ne!(first.id, second.id);
        let navigation = view.navigation();
        for reference in &references {
            assert!(navigation.iter().any(|node| {
                node.target_id.as_str() == reference.id.as_ref()
                    || view
                        .associated_reference_choices(&node.id)
                        .iter()
                        .any(|(id, _)| id.as_str() == reference.id.as_ref())
            }));
        }
        for width in [16, 78] {
            let rendered = view.render(width);
            let hits = rendered.search("printf(3)");
            assert_eq!(hits.len(), 2);
            for (hit, reference) in hits.iter().zip(&references) {
                assert_eq!(
                    rendered.anchor_row(&reference.id),
                    Some(hit.row),
                    "{source}"
                );
                assert_eq!(
                    rendered.link_target_at(hit.row, hit.start_column),
                    Some(&LinkTarget::Manual {
                        name: "printf".into(),
                        manual_section: Some("3".into())
                    })
                );
                assert_eq!(rendered.link_target_at(hit.row, hit.end_column), None);
            }
            if width == 78 {
                let body = &rendered.search("FIRSTBODY")[0];
                if tail == "\n.br" {
                    assert_eq!(body.row, hits[0].row + 1);
                } else {
                    assert_eq!(body.row, hits[0].row);
                    // termp_it_pre adds two columns to the requested width.
                    assert_eq!(body.start_column - hits[0].start_column, 26);
                    let copied = rendered.selected_text(RenderedSelection {
                        anchor: TextPosition {
                            row: hits[0].row,
                            column: hits[0].start_column,
                        },
                        focus: TextPosition {
                            row: body.row,
                            column: body.end_column - 1,
                        },
                    });
                    let prefix = if tail.ends_with(r"\~") {
                        "printf(3)\u{a0}"
                    } else {
                        "printf(3)"
                    };
                    assert_eq!(copied, format!("{prefix:<26}FIRSTBODY"));
                }
            }
        }
    }
}

#[test]
fn ip_end_padding_preserves_painted_columns_and_copy() {
    // Fixed CVS term_flushln() keeps the fitting IP HEAD active without
    // writing its final ASCII padding into the BODY field.
    for (label, operand, field) in [("1.", "1.", 4), ("LABEL", r"\fBLABEL\fR", 8)] {
        for tail in [" ".repeat(8), " ".repeat(1000)] {
            let source = format!("{HEADER}.IP \"{operand}{tail}\" {field}\nBODY\n");
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
            let query = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
                .map(ResolvedContent::from)
                .unwrap();
            let view = DocumentView::new(&query);
            for width in [24, 78, 24] {
                let rendered = view.render(width);
                let label_hit = &rendered.search(label)[0];
                let body = &rendered.search("BODY")[0];
                assert_eq!(label_hit.row, body.row, "{source}");
                assert_eq!(
                    body.start_column - label_hit.start_column,
                    field,
                    "{source}"
                );
                assert_word_cells(&rendered, label, width, None);
                assert_word_cells(&rendered, "BODY", width, None);
                let selected = rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: label_hit.row,
                        column: label_hit.start_column,
                    },
                    focus: TextPosition {
                        row: body.row,
                        column: body.end_column - 1,
                    },
                });
                assert_eq!(selected, format!("{label:<field$}BODY"), "{source}");
            }
        }
    }
}

#[test]
fn ending_a_field_retains_link_and_anchor_ranges_without_linking_its_gap() {
    // A public IR term can carry a link through the final styled text run.
    let mut query =
        mant_loader::load_roff_bytes(format!("{HEADER}.IP \"LABEL        \" 8\nBODY\n").as_bytes())
            .unwrap();
    let Block::DefinitionList { items, .. } =
        &mut query.document.as_mut().unwrap().sections[1].blocks[0]
    else {
        panic!("IP definition");
    };
    let uri = "https://example.test/label";
    let target = LinkTarget::External(ExternalUri::parse(uri).unwrap());
    items[0].terms[0].content = vec![
        Inline::Link {
            target: mant_ir::LinkTarget::External { uri: uri.into() },
            title: None,
            children: vec![
                Inline::Emphasis {
                    children: vec![text("LABEL  ")],
                },
                text("      "),
            ],
        },
        Inline::anchor("field-end"),
    ];
    let view = DocumentView::new(&query);
    assert!(view.validate_fragment("field-end").is_ok());
    let rendered = view.render(24);
    assert_word_cells(&rendered, "LABEL", 24, Some(&target));
    let label = &rendered.search("LABEL")[0];
    let body = &rendered.search("BODY")[0];
    assert_eq!(body.start_column - label.start_column, 8);
    assert_eq!(rendered.link_target_at(label.row, label.end_column), None);
    assert_eq!(rendered.anchor_row("field-end"), Some(label.row));
}
