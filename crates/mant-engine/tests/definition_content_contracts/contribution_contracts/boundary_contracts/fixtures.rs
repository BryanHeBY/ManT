use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum EmptyTable {
    NoRows,
    NoCells,
    EmptyBlocks,
    EmptyLiteral,
}

pub(super) const fn options(preserve_anchors: bool) -> MarkdownOptions {
    MarkdownOptions {
        preserve_anchors,
        preserve_semantics: false,
    }
}

pub(super) fn import(value: &ResolvedContent, options: MarkdownOptions) -> ResolvedContent {
    let markdown = render_markdown_with_options(value, options);
    mant_loader::load_markdown_text(&markdown, None).unwrap()
}

pub(super) fn import_root_fragment(
    value: &ResolvedContent,
    preserve_anchors: bool,
) -> ResolvedContent {
    // Whole-artifact comparisons above retain the reader's documented literal
    // heading-anchor policy. Isolate this scope for exact gap counts without
    // stripping source HTML or inventing an expectation from rendered output.
    let blocks = mant_codec::encode::render_blocks_fragment(
        &value.document.as_ref().unwrap().blocks,
        mant_codec::encode::MarkdownFragmentOptions { preserve_anchors },
    );
    let markdown = format!("# Probe\n\n{}", blocks.join("\n\n"));
    mant_loader::load_markdown_text(&markdown, None).unwrap()
}

pub(super) fn gap_sequence(gap: Gap, position: Position, physical: Physical) -> Vec<Block> {
    let mut blocks = vec![];
    if !matches!(position, Position::Prefix) {
        blocks.push(prose(vec![text(BEFORE)], 19));
    }
    if !matches!(position, Position::Tail) {
        blocks.push(spaced_carrier(gap));
    }
    blocks.extend(physical.blocks());
    if matches!(position, Position::Tail) {
        blocks.push(spaced_carrier(gap));
    } else {
        blocks.push(prose(vec![text(AFTER)], 21));
    }
    blocks
}

pub(super) fn root_gap(gap: Gap, prefix: bool) -> Vec<Block> {
    let body = prose(vec![text(BODY)], 20);
    if prefix {
        vec![spaced_carrier(gap), body]
    } else {
        vec![body, spaced_carrier(gap)]
    }
}

pub(super) fn only_gap(context: Context, gap: Gap) -> ResolvedContent {
    struct RemoveHead;
    impl visit::VisitMut for RemoveHead {
        fn visit_list_item_mut(&mut self, item: &mut ListItem) {
            if item.entry.take().is_some() {
                let Block::Paragraph { children, .. } = &mut item.blocks[0] else {
                    panic!("original list name root")
                };
                children.clear();
            }
            visit::walk_list_item_mut(self, item);
        }

        fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
            if item.entry.take().is_some() {
                item.terms[0].content.clear();
            }
            visit::walk_definition_item_mut(self, item);
        }
    }
    let mut value = install(context, vec![spaced_carrier(gap)]);
    visit::VisitMut::visit_document_mut(&mut RemoveHead, value.document.as_mut().unwrap());
    value
}

pub(super) fn empty_table(shape: EmptyTable, break_after: bool) -> Block {
    let rows = match shape {
        EmptyTable::NoRows => vec![],
        EmptyTable::NoCells => vec![TableRow {
            kind: TableRowKind::Data,
            cells: vec![],
        }],
        EmptyTable::EmptyBlocks | EmptyTable::EmptyLiteral => vec![TableRow {
            kind: TableRowKind::Data,
            cells: vec![TableCell {
                kind: TableCellKind::Text,
                blocks: if matches!(shape, EmptyTable::EmptyLiteral) {
                    vec![Block::Preformatted {
                        children: vec![text("")],
                        inline_layout: InlineLayout::default(),
                        language: None,
                        layout: LayoutHint::default(),
                        source: Some(source(12)),
                    }]
                } else {
                    vec![]
                },
                break_after,
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
    };
    Block::Table {
        rows,
        column_preferences: ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: Some(source(12)),
    }
}
