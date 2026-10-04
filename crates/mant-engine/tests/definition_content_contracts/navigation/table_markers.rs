//! A table's existing marker or fence grammar receives zero-width navigation.
use super::*;
use fixtures::{Body, Case, Head, Navigation, source, specimen};
use neighbors::{assert_snapshot, owner_mut, projection, without_empty_links};

const FIRST_CELL: &str = "Table中";
const LAST_CELL: &str = "Cell終";

#[derive(Clone, Copy, Debug)]
enum Position {
    Prefix,
    Tail,
}

fn paragraph(children: Vec<Inline>, line: u32) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: Some(source(line)),
    }
}

fn cell(children: Vec<Inline>, line: u32) -> TableCell {
    TableCell {
        kind: TableCellKind::Text,
        blocks: vec![paragraph(children, line)],
        break_after: false,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

fn table(data: bool) -> Block {
    let marker = TableRow {
        kind: TableRowKind::Data,
        cells: vec![cell(
            vec![Inline::Strong {
                children: vec![Inline::Anchor {
                    id: "table-navigation".into(),
                    fragment_aliases: vec!["Table.Navigation".into()],
                    owner_source: Some(source(35)),
                }],
            }],
            35,
        )],
    };
    assert!(table_row_is_navigation_only(&marker));
    let mut rows = vec![marker];
    if data {
        rows.push(TableRow {
            kind: TableRowKind::Data,
            cells: vec![
                cell(vec![text(FIRST_CELL)], 40),
                cell(vec![text(LAST_CELL)], 41),
            ],
        });
    }
    Block::Table {
        rows,
        column_preferences: ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: Some(source(35)),
    }
}

fn value(case: Case, position: Position, data: bool) -> ResolvedContent {
    let mut value = specimen(case, true);
    let prefix = item(&value).description[0].clone();
    let spacing = item(&value).description[1].clone();
    owner_mut(&mut value).description = match position {
        Position::Prefix => vec![prefix, spacing, table(data)],
        Position::Tail => vec![spacing, table(data), prefix],
    };
    value
}

fn assert_fences(imported: &ResolvedContent, data: bool) {
    struct Fences(Vec<(String, Option<String>)>);
    impl<'ir> visit::Visit<'ir> for Fences {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::Preformatted {
                children, language, ..
            } = block
            {
                self.0.push((inline_plain_text(children), language.clone()));
            }
            visit::walk_block(self, block);
        }
    }
    let mut fences = Fences(vec![]);
    visit::Visit::visit_document(&mut fences, imported.document.as_ref().unwrap());
    let expected = if data {
        vec![("Table中 | Cell終".to_owned(), None)]
    } else {
        vec![]
    };
    assert_eq!(
        fences.0, expected,
        "navigation must not glue to or enter a fence"
    );
}

fn assert_rows(imported: &ResolvedContent, case: Case, data: bool, options: MarkdownOptions) {
    let plain = mant_render::render_query_man(imported);
    assert_eq!(
        plain.matches(NAME).count(),
        usize::from(case.head.has_word())
    );
    assert_eq!(plain.matches(TAIL).count(), 1);
    for word in [FIRST_CELL, LAST_CELL] {
        assert_eq!(plain.matches(word).count(), usize::from(data), "{plain}");
    }
    assert_fences(imported, data);
    if options.preserve_anchors {
        // Raw authored anchor syntax keeps its frozen literal-body policy.
        // The entire shape and plain output still equal the exact baseline;
        // no anchor text, whitespace or blank line is filtered from either.
        return;
    }
    if data && case.head.has_word() {
        let head = plain.find(NAME).unwrap() + NAME.len();
        let body = plain.find(FIRST_CELL).unwrap();
        assert_eq!(
            plain[head..body].matches('\n').count(),
            usize::from(case.spacing) + 1,
            "the executed positive BODY gap remains independent: {plain}"
        );
    }
    if data {
        let body = plain.find(LAST_CELL).unwrap() + LAST_CELL.len();
        let tail = plain.find(TAIL).unwrap();
        assert_eq!(plain[body..tail].matches('\n').count(), 2, "{plain}");
    }
}

fn assert_markers(markdown: &str, options: MarkdownOptions) {
    for id in ["table-navigation", "Table.Navigation"] {
        let marker = format!("id=\"{id}\"");
        assert_eq!(
            markdown.matches(&marker).count(),
            usize::from(options.preserve_anchors),
            "{markdown}"
        );
    }
}

#[test]
fn empty_references_use_table_markers_or_fences_without_extra_rows() {
    for position in [Position::Prefix, Position::Tail] {
        for data in [false, true] {
            for head in [Head::Empty, Head::Word] {
                for spacing in [0, 1] {
                    for relation in [
                        HeadBodyRelation::joined(),
                        HeadBodyRelation::separated(),
                        HeadBodyRelation::Separate,
                    ] {
                        let case = Case {
                            navigation: Navigation::External,
                            body: Body::Table,
                            head,
                            spacing,
                            relation,
                        };
                        let original = value(case, position, data);
                        let restored = round_trip(&original);
                        let baseline = round_trip(&without_empty_links(&original));
                        assert_snapshot(&original, &restored, Navigation::External);
                        for preserve_anchors in [false, true] {
                            let options = MarkdownOptions {
                                preserve_anchors,
                                preserve_semantics: false,
                            };
                            let actual = render_markdown_with_options(&restored, options);
                            let control = render_markdown_with_options(&baseline, options);
                            let imported = projection(&actual, &control, Navigation::External);
                            assert_rows(&imported, case, data, options);
                            assert_markers(&actual, options);
                        }
                        assert_eq!(restored, original);
                    }
                }
            }
        }
    }
}
