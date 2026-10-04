//! Term tail origins stay outside body scalars across transparent containers.

use super::super::*;
use super::fixtures::{
    assert_word, cell, clipped, copy_cells, json_content, linked, paragraph_nodes, table, text,
};
use mant_ir::{
    ColumnPreferences, DefinitionLayout, DefinitionTerm, HeadBodyRelation, InlineLayout,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Head {
    Single,
    Open,
    Completed,
    Hard,
    Navigation,
}

impl Head {
    const fn breaks(self) -> usize {
        match self {
            Self::Single | Self::Navigation => 0,
            Self::Open | Self::Hard => 1,
            Self::Completed => 2,
        }
    }

    const fn has_text(self) -> bool {
        matches!(self, Self::Single | Self::Open | Self::Completed)
    }

    const fn physical_rows(self) -> usize {
        if matches!(self, Self::Navigation) {
            0
        } else {
            self.breaks() + 1
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Container {
    Root,
    Plain,
    Table,
    Nested,
}

#[derive(Clone, Copy, Debug)]
struct DefinitionCase {
    head: Head,
    tail_hint: bool,
    correction: i32,
    origin: i32,
    relation: HeadBodyRelation,
    container: Container,
}

impl DefinitionCase {
    fn hint(self, row: usize) -> i32 {
        let hinted = if self.tail_hint {
            self.head.breaks()
        } else {
            0
        };
        if row == hinted { self.correction } else { 0 }
    }
}

fn definition(case: DefinitionCase) -> Block {
    let mut content = vec![Inline::anchor("head-start")];
    if case.head.has_text() {
        content.push(Inline::Strong {
            children: vec![text("X")],
        });
    }
    content.extend((0..case.head.breaks()).map(|_| Inline::LineBreak {}));
    content.push(Inline::anchor("head-tail"));
    let hinted = if case.tail_hint {
        case.head.breaks()
    } else {
        0
    };
    Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            terms: vec![DefinitionTerm {
                content,
                inline_layout: InlineLayout {
                    row_hints: if case.correction == 0 {
                        vec![]
                    } else {
                        vec![mant_ir::RowLayoutHint {
                            row: u32::try_from(hinted).unwrap(),
                            indent_columns: case.correction,
                        }]
                    },
                },
            }],
            description: vec![paragraph_nodes(vec![
                Inline::anchor("body"),
                text("TERM "),
                linked("Y"),
            ])],
            head_body_relation: case.relation,
            layout: DefinitionLayout {
                body_indent_columns: 8,
                min_term_gap_columns: 2,
                ..Default::default()
            },
            entry: None,
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn definition_columns() -> ColumnPreferences {
    // Generic automatic columns may reduce child indentation to reserve half
    // their measured width for reading. This matrix isolates owner tail facts
    // in a wide declared column; the automatic route is checked separately.
    ColumnPreferences {
        widths: vec![64],
        ..Default::default()
    }
}

fn contained_definition(case: DefinitionCase) -> Block {
    let block = definition(case);
    match case.container {
        Container::Root => {
            let Block::DefinitionList {
                items,
                compact,
                declaration_groups,
                source,
                ..
            } = block
            else {
                unreachable!()
            };
            Block::DefinitionList {
                items,
                compact,
                declaration_groups,
                source,
                layout: LayoutHint {
                    indent_columns: case.origin,
                    ..Default::default()
                },
            }
        }
        Container::Plain => Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![ListItem {
                blocks: vec![block],
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
            }],
            layout: LayoutHint {
                indent_columns: case.origin,
                ..Default::default()
            },
            source: None,
        },
        Container::Table => table(
            vec![cell(vec![block], false)],
            definition_columns(),
            case.origin,
        ),
        Container::Nested => table(
            vec![cell(
                vec![table(
                    vec![cell(vec![block], false)],
                    definition_columns(),
                    0,
                )],
                false,
            )],
            definition_columns(),
            case.origin,
        ),
    }
}

fn assert_definition(case: DefinitionCase) {
    let content = json_content(vec![contained_definition(case), paragraph("AFTER")]);
    let rendered = DocumentView::new(&content).render(80);
    let rows = case.head.physical_rows();
    let shared = rows > 0 && case.relation != HeadBodyRelation::Separate;
    let body_row = if shared { rows - 1 } else { rows };
    let tail_width = usize::from(case.head == Head::Single);
    let head_column = clipped(case.origin + case.hint(case.head.breaks()));
    let body_origin = clipped(case.origin + 8);
    let gap = if shared && case.relation == HeadBodyRelation::separated() {
        body_origin.saturating_sub(head_column + tail_width).max(2)
    } else {
        0
    };
    let body_column = if shared {
        head_column + tail_width + gap
    } else {
        body_origin
    };
    let body = assert_word(&rendered, "TERM", false);
    let label = assert_word(&rendered, "Y", true);
    assert_eq!(
        (body.row, body.start_column),
        (body_row, body_column),
        "{case:?}"
    );
    assert_eq!(
        (label.row, label.start_column),
        (body_row, body_column + 5),
        "{case:?}"
    );
    assert_eq!(rendered.anchor_row("body"), Some(body_row), "{case:?}");
    assert_eq!(rendered.anchor_row("head-start"), Some(0), "{case:?}");
    assert_eq!(
        rendered.anchor_row("head-tail"),
        Some(case.head.breaks()),
        "{case:?}"
    );
    assert_eq!(
        rendered.row_count,
        body_row + 2,
        "{case:?}: {:?}",
        rendered.text
    );
    assert_eq!(assert_word(&rendered, "AFTER", false).row, body_row + 1);
    let prefix = if shared {
        head_column.min(clipped(case.origin))
    } else {
        body_origin
    };
    let head = if shared && tail_width == 1 { "X" } else { "" };
    let expected = format!("{}{head}{}TERM Y", " ".repeat(prefix), " ".repeat(gap));
    assert_eq!(copy_cells(&rendered, body_row, 0, 79), expected, "{case:?}");
    assert_definition_head(&rendered, case, body_row);
}

fn assert_definition_head(rendered: &RenderedDocument, case: DefinitionCase, body_row: usize) {
    if case.head.has_text() {
        let head = assert_word(rendered, "X", false);
        assert_eq!(
            (head.row, head.start_column),
            (0, clipped(case.origin + case.hint(0))),
            "{case:?}"
        );
        assert!(rendered.text.lines[0].spans.iter().any(|span| {
            span.content.contains('X') && span.style.add_modifier.contains(Modifier::BOLD)
        }));
    }
    for row in usize::from(case.head.has_text())..body_row {
        assert_eq!(copy_cells(rendered, row, 0, 79), "", "{case:?}: row={row}");
    }
}

#[test]
fn term_tail_hints_never_become_body_scalars_through_containers() {
    for head in [
        Head::Single,
        Head::Open,
        Head::Completed,
        Head::Hard,
        Head::Navigation,
    ] {
        for (tail_hint, correction) in [(false, 0), (false, 4), (true, 4), (false, -4), (true, -4)]
        {
            for origin in [-3, 0, 3] {
                for relation in [
                    HeadBodyRelation::joined(),
                    HeadBodyRelation::separated(),
                    HeadBodyRelation::Separate,
                ] {
                    for container in [
                        Container::Root,
                        Container::Plain,
                        Container::Table,
                        Container::Nested,
                    ] {
                        assert_definition(DefinitionCase {
                            head,
                            tail_hint,
                            correction,
                            origin,
                            relation,
                            container,
                        });
                    }
                }
            }
        }
    }
}

#[test]
fn automatic_definition_column_adapts_reading_without_rewriting_owner_layout() {
    let case = DefinitionCase {
        head: Head::Single,
        tail_hint: false,
        correction: 0,
        origin: 0,
        relation: HeadBodyRelation::Separate,
        container: Container::Table,
    };
    let content = json_content(vec![
        table(
            vec![cell(vec![definition(case)], false)],
            ColumnPreferences::default(),
            0,
        ),
        paragraph("AFTER"),
    ]);
    let Block::Table { rows, .. } = &content.document.as_ref().unwrap().blocks[0] else {
        panic!("automatic table");
    };
    let Block::DefinitionList { items, .. } = &rows[0].cells[0].blocks[0] else {
        panic!("original definition owner");
    };
    assert_eq!(items[0].layout.body_indent_columns, 8);
    assert_eq!(items[0].head_body_relation, HeadBodyRelation::Separate);
    let rendered = DocumentView::new(&content).render(80);
    // The measured field is 8 origin + 6 body cells = 14 columns. The
    // documented narrow-field policy reserves half (7), translating the
    // displayed body origin from 8 to 7 without changing the source layout.
    let body = assert_word(&rendered, "TERM", false);
    let label = assert_word(&rendered, "Y", true);
    assert_eq!((body.row, body.start_column), (1, 7));
    assert_eq!((label.row, label.start_column), (1, 12));
    assert_eq!(copy_cells(&rendered, 1, 0, 79), "       TERM Y");
    assert_eq!(rendered.anchor_row("body"), Some(1));
    assert_eq!(rendered.anchor_row("head-start"), Some(0));
    assert_eq!(rendered.anchor_row("head-tail"), Some(0));
    assert_eq!(rendered.row_count, 3);
}
