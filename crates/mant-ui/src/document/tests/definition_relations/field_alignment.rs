//! Shared fields retain labels and position the body from the printed end.

use super::*;

const HEADER: &str = concat!(
    ".TH PROBE 1 \"November 18, 2026\" Test Manual\n.SH NAME\n",
    "probe \\- exercise descriptors\n.SH DESCRIPTION\n",
);

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
