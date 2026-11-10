//! Empty-looking content can still own a data row, rule, or list marker.
use super::*;

#[derive(Clone, Copy, Debug)]
enum TableRowShape {
    Navigation,
    EmptyText,
    NoCells,
    Rule,
}

impl TableRowShape {
    fn row(self) -> TableRow {
        let (kind, cells) = match self {
            Self::Navigation => (TableRowKind::Data, vec![cell(paragraph(vec![anchor()]))]),
            Self::EmptyText => (TableRowKind::Data, vec![cell(paragraph(vec![text("")]))]),
            Self::NoCells => (TableRowKind::Data, vec![]),
            Self::Rule => (TableRowKind::HorizontalRule, vec![]),
        };
        TableRow {
            kind,
            cells,
            column_borders: vec![],
        }
    }

    const fn rows(self) -> &'static [&'static str] {
        match self {
            Self::Navigation => &[],
            Self::EmptyText | Self::NoCells => &[""],
            Self::Rule => &["---"],
        }
    }
}

fn cell(block: Block) -> TableCell {
    TableCell {
        kind: TableCellKind::Text,
        blocks: vec![block],
        break_after: false,
        column_span: 1,
        row_span: 1,
        alignment: None,
        preferred_width_columns: None,
    }
}

#[derive(Clone, Copy, Debug)]
enum TableMode {
    Intrinsic,
    Declared,
    Stacked,
}

fn table(mode: TableMode, shape: TableRowShape, spacing: u16) -> Block {
    let rows = vec![shape.row()];
    let origin = if matches!(mode, TableMode::Stacked) {
        -1
    } else {
        0
    };
    assert_eq!(
        geometry::table_requires_origin_preserving_stack(&rows, origin),
        matches!(mode, TableMode::Stacked)
    );
    Block::Table {
        rows,
        column_count: Some(1),
        alignment: None,
        borders: TableBorders::default(),
        column_preferences: ColumnPreferences {
            widths: if matches!(mode, TableMode::Declared) {
                vec![3]
            } else {
                vec![]
            },
            ..ColumnPreferences::default()
        },
        layout: LayoutHint {
            indent_columns: origin,
            spacing_before_lines: spacing,
            ..LayoutHint::default()
        },
        source: None,
    }
}

#[test]
fn physical_table_rows_are_not_reduced_to_visible_text() {
    let mut cases = 0;
    for shape in [
        TableRowShape::Navigation,
        TableRowShape::EmptyText,
        TableRowShape::NoCells,
        TableRowShape::Rule,
    ] {
        for mode in [
            TableMode::Intrinsic,
            TableMode::Declared,
            TableMode::Stacked,
        ] {
            for neighbors in [false, true] {
                for spacing in [0, 1] {
                    let mut blocks = vec![table(mode, shape, spacing)];
                    if neighbors {
                        blocks.insert(0, paragraph(vec![text("BEFORE")]));
                        blocks.push(paragraph(vec![text("AFTER")]));
                    }
                    let mut original = base(0);
                    original.document.as_mut().unwrap().blocks = blocks;
                    let issues = validate_document(original.document.as_ref().unwrap());
                    assert!(
                        issues.is_empty(),
                        "{shape:?}/{mode:?}/{neighbors}/{spacing}: {issues:?}"
                    );
                    for wire in [false, true] {
                        let value = if wire {
                            query_wire(&original)
                        } else {
                            original.clone()
                        };
                        let id = format!("{shape:?}/{mode:?}/{neighbors}/{spacing}/{wire}");
                        let mut rows = vec![LABEL.to_owned()];
                        if neighbors || spacing > 0 || !shape.rows().is_empty() {
                            rows.push(String::new());
                        }
                        if neighbors {
                            rows.push("BEFORE".into());
                        }
                        rows.extend(std::iter::repeat_n(String::new(), usize::from(spacing)));
                        rows.extend(shape.rows().iter().map(|row| (*row).to_owned()));
                        if neighbors {
                            rows.push("AFTER".into());
                        }
                        assert_eq!(render_query_text(&value), physical_text(&rows), "{id}");
                        if matches!(shape, TableRowShape::Navigation) {
                            assert_eq!(
                                DocumentIndex::build(value.document.as_ref().unwrap())
                                    .fragment_target("Source.Nav"),
                                Some(&NodeId::from("nav")),
                                "{id}"
                            );
                        }
                        audit("CG-B-table", &id, "exact-structural-rows");
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 96);
}

#[test]
fn physical_list_markers_preserve_ordinals_and_item_spacing() {
    let mut cases = 0;
    for (kind, first, second) in [
        (ListKind::Bullet, "•", "•"),
        (ListKind::Dash, "-", "-"),
        (ListKind::Ordered { start: Some(3) }, "3.", "4."),
    ] {
        for body in [Body::None, Body::Anchor, Body::HardRow] {
            for compact in [false, true] {
                let item = |blocks| ListItem {
                    blocks,
                    source: None,
                    entry: None,
                    layout: ListItemLayout::default(),
                };
                let mut value = base(0);
                value.document.as_mut().unwrap().blocks.push(Block::List {
                    kind,
                    compact,
                    items: vec![
                        item(vec![paragraph(vec![text("BEFORE")])]),
                        item(body.blocks()),
                    ],
                    layout: LayoutHint::default(),
                    source: None,
                });
                assert_eq!(validate_document(value.document.as_ref().unwrap()), []);
                let expected = format!(
                    "{LABEL}\n\n{first} BEFORE\n{}{second}{}",
                    if compact { "" } else { "\n" },
                    if body == Body::HardRow { " \n" } else { "" }
                );
                assert_eq!(
                    render_query_text(&value),
                    expected,
                    "{kind:?}/{body:?}/{compact}"
                );
                audit(
                    "CG-B-list",
                    &format!("{kind:?}/{body:?}/{compact}"),
                    "physical-marker/ordinal/item-gap",
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 18);
}
