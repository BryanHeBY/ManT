//! Lowers Markdown block containers into the source-neutral document model.

use std::ops::Range;

use mant_ir::{
    Block, ContentRootKind, Diagnostic, LayoutHint, ListItem, ListKind, TableAlignment, TableCell,
    TableRow,
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Tag, TagEnd};

use super::{
    EventCursor,
    content::MarkdownContent,
    inline::{parse_inline_run, parse_inlines, starts_inline_run},
    source::MarkdownSource,
};

pub(super) fn parse_blocks_until(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    diagnostics: &mut Vec<Diagnostic>,
    end: TagEnd,
) -> (Vec<Block>, usize) {
    let mut blocks = Vec::new();
    let mut end_offset = 0;
    loop {
        if matches!(cursor.peek(), Some((Event::End(actual), _)) if *actual == end) {
            if let Some((_, range)) = cursor.next() {
                end_offset = range.end;
            }
            break;
        }
        if let Some((event, range)) = cursor.peek()
            && starts_inline_run(event)
        {
            let start = range.start;
            let (children, inline_end) = parse_inline_run(cursor, source, content, diagnostics);
            blocks.push(Block::Paragraph {
                children,
                layout: LayoutHint::default(),
                source: Some(source.span(&(start..inline_end))),
            });
            continue;
        }
        let Some(block) = parse_block(cursor, source, content, diagnostics) else {
            if cursor.peek().is_none() {
                break;
            }
            continue;
        };
        blocks.push(block);
    }
    (blocks, end_offset)
}

pub(super) fn parse_block(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Block> {
    let (event, range) = cursor.next()?;
    match event {
        Event::Start(Tag::Paragraph) => {
            let start = range.start;
            let (children, end) = parse_inlines(
                cursor,
                source,
                content,
                diagnostics,
                TagEnd::Paragraph,
                ContentRootKind::Body,
            );
            Some(Block::Paragraph {
                children,
                layout: LayoutHint::default(),
                source: Some(source.span(&(start..end))),
            })
        }
        Event::Start(Tag::CodeBlock(kind)) => {
            Some(parse_code_block(cursor, source, content, kind, range))
        }
        Event::Start(Tag::List(start)) => {
            if cursor.subtree_contains_task_marker() {
                let whole = cursor.consume_balanced(range);
                return Some(source.unsupported_block("task list", whole, diagnostics));
            }
            if !cursor.try_descend() {
                let whole = cursor.consume_balanced(range);
                return Some(source.unsupported_block("deeply nested list", whole, diagnostics));
            }
            let list = parse_list(cursor, source, content, diagnostics, start, range);
            cursor.ascend();
            Some(list)
        }
        Event::Start(Tag::Table(alignments)) => Some(parse_table(
            cursor,
            source,
            content,
            diagnostics,
            &alignments,
            range,
        )),
        Event::Rule => Some(Block::ThematicBreak {
            source: Some(source.span(&range)),
        }),
        Event::Start(tag) => {
            let name = unsupported_block_name(&tag);
            let whole = cursor.consume_balanced(range);
            Some(source.unsupported_block(name, whole, diagnostics))
        }
        Event::Text(value) | Event::Code(value) => {
            let span = source.span(&range);
            let mut root = content.root(ContentRootKind::Body, Some(span));
            Some(Block::Paragraph {
                children: vec![root.text(value.into_string(), Some(span))],
                layout: LayoutHint::default(),
                source: Some(span),
            })
        }
        Event::Html(_)
        | Event::InlineHtml(_)
        | Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::FootnoteReference(_)
        | Event::TaskListMarker(_) => {
            Some(source.unsupported_block("inline construct", range, diagnostics))
        }
        Event::SoftBreak | Event::HardBreak | Event::End(_) => None,
    }
}

fn parse_code_block(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    kind: CodeBlockKind<'_>,
    start_range: Range<usize>,
) -> Block {
    let mut value = String::new();
    let mut end = start_range.end;
    while let Some((event, range)) = cursor.next() {
        end = range.end;
        match event {
            Event::End(TagEnd::CodeBlock) => break,
            Event::Text(text) | Event::Code(text) => value.push_str(&text),
            Event::SoftBreak | Event::HardBreak => value.push('\n'),
            _ => value.push_str(source.raw(&range)),
        }
    }
    let language = match kind {
        CodeBlockKind::Indented => None,
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    };
    if value.ends_with('\n') {
        value.pop();
    }
    let span = source.span(&(start_range.start..end));
    let mut root = content.root(ContentRootKind::FixedBody, Some(span));
    Block::Preformatted {
        children: vec![root.text(value, Some(span))],
        language,
        layout: LayoutHint::default(),
        source: Some(span),
    }
}

fn parse_list(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    diagnostics: &mut Vec<Diagnostic>,
    start: Option<u64>,
    start_range: Range<usize>,
) -> Block {
    let mut items = Vec::new();
    let mut compact = true;
    let mut end = start_range.end;
    while let Some((event, range)) = cursor.next() {
        end = range.end;
        match event {
            Event::Start(Tag::Item) => {
                compact &= !cursor.item_has_direct_paragraph();
                let (blocks, item_end) =
                    parse_blocks_until(cursor, source, content, diagnostics, TagEnd::Item);
                end = item_end;
                items.push(ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: Some(source.span(&(range.start..item_end))),
                    entry: None,
                    blocks,
                });
            }
            Event::End(TagEnd::List(_)) => break,
            Event::Start(tag) => {
                let whole = cursor.consume_balanced(range);
                items.push(ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                    blocks: vec![source.unsupported_block(
                        unsupported_block_name(&tag),
                        whole,
                        diagnostics,
                    )],
                });
            }
            _ => {}
        }
    }
    let whole = start_range.start..end;
    Block::List {
        kind: if start.is_some() {
            ListKind::Ordered { start }
        } else {
            ListKind::Bullet
        },
        compact,
        items,
        layout: LayoutHint::default(),
        source: Some(source.span(&whole)),
    }
}

fn parse_table(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    diagnostics: &mut Vec<Diagnostic>,
    alignments: &[Alignment],
    start_range: Range<usize>,
) -> Block {
    let mut rows = Vec::new();
    let mut end = start_range.end;
    while let Some((event, range)) = cursor.next() {
        end = range.end;
        match event {
            Event::Start(Tag::TableHead) => {
                let (row, row_end) = parse_table_row(
                    cursor,
                    source,
                    content,
                    diagnostics,
                    alignments,
                    TagEnd::TableHead,
                );
                end = row_end;
                rows.push(row);
            }
            Event::Start(Tag::TableRow) => {
                let (row, row_end) = parse_table_row(
                    cursor,
                    source,
                    content,
                    diagnostics,
                    alignments,
                    TagEnd::TableRow,
                );
                end = row_end;
                rows.push(row);
            }
            Event::End(TagEnd::Table) => break,
            _ => {}
        }
    }
    Block::Table {
        rows,
        fixed_view: None,
        layout: LayoutHint::default(),
        source: Some(source.span(&(start_range.start..end))),
    }
}

fn parse_table_row(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    diagnostics: &mut Vec<Diagnostic>,
    alignments: &[Alignment],
    end_tag: TagEnd,
) -> (TableRow, usize) {
    let mut cells = Vec::new();
    let mut end = 0;
    while let Some((event, range)) = cursor.next() {
        end = range.end;
        match event {
            Event::Start(Tag::TableCell) => {
                let start = range.start;
                let (children, cell_end) = parse_inlines(
                    cursor,
                    source,
                    content,
                    diagnostics,
                    TagEnd::TableCell,
                    ContentRootKind::Cell,
                );
                end = cell_end;
                let blocks = if children.is_empty() {
                    Vec::new()
                } else {
                    vec![Block::Paragraph {
                        children,
                        layout: LayoutHint::default(),
                        source: Some(source.span(&(start..cell_end))),
                    }]
                };
                cells.push(TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks,
                    point: None,
                    column_span: 1,
                    row_span: 1,
                    alignment: alignments
                        .get(cells.len())
                        .and_then(|alignment| table_alignment(*alignment)),
                    source: Some(source.span(&(start..cell_end))),
                });
            }
            Event::End(actual) if actual == end_tag => break,
            Event::Start(tag) => {
                let whole = cursor.consume_balanced(range);
                let cell_source = source.span(&whole);
                cells.push(TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![source.unsupported_block(
                        unsupported_block_name(&tag),
                        whole,
                        diagnostics,
                    )],
                    point: None,
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: Some(cell_source),
                });
            }
            _ => {}
        }
    }
    (
        TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells,
        },
        end,
    )
}

fn table_alignment(alignment: Alignment) -> Option<TableAlignment> {
    match alignment {
        Alignment::None => None,
        Alignment::Left => Some(TableAlignment::Left),
        Alignment::Center => Some(TableAlignment::Center),
        Alignment::Right => Some(TableAlignment::Right),
    }
}

fn unsupported_block_name(tag: &Tag<'_>) -> &'static str {
    match tag {
        Tag::BlockQuote(_) => "block quote",
        Tag::HtmlBlock => "HTML block",
        Tag::FootnoteDefinition(_) => "footnote definition",
        Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
            "definition list"
        }
        Tag::MetadataBlock(_) => "metadata block",
        Tag::Heading { .. } => "nested heading",
        Tag::Image { .. } => "image",
        Tag::Strikethrough => "strikethrough",
        Tag::Superscript => "superscript",
        Tag::Subscript => "subscript",
        Tag::Link { .. } => "link",
        Tag::Paragraph
        | Tag::CodeBlock(_)
        | Tag::List(_)
        | Tag::Item
        | Tag::Table(_)
        | Tag::TableHead
        | Tag::TableRow
        | Tag::TableCell
        | Tag::Emphasis
        | Tag::Strong => "Markdown construct",
    }
}
