//! Lowers Markdown block containers into the source-neutral document model.

use std::ops::Range;

use mant_ast::{
    Block, Diagnostic, Inline, LayoutHint, ListItem, ListKind, TableAlignment, TableCell, TableRow,
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Tag, TagEnd};

use super::{
    EventCursor,
    inline::{parse_inline_run, parse_inlines, starts_inline_run},
    source::MarkdownSource,
};

pub(super) fn parse_blocks_until(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
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
            let (children, inline_end) = parse_inline_run(cursor, source, diagnostics);
            blocks.push(Block::Paragraph {
                children,
                layout: LayoutHint::default(),
                source: Some(source.span(&(start..inline_end))),
            });
            continue;
        }
        let Some(block) = parse_block(cursor, source, diagnostics) else {
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
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Block> {
    let (event, range) = cursor.next()?;
    match event {
        Event::Start(Tag::Paragraph) => {
            let start = range.start;
            let (children, end) = parse_inlines(cursor, source, diagnostics, TagEnd::Paragraph);
            Some(Block::Paragraph {
                children,
                layout: LayoutHint::default(),
                source: Some(source.span(&(start..end))),
            })
        }
        Event::Start(Tag::CodeBlock(kind)) => Some(parse_code_block(cursor, source, kind, range)),
        Event::Start(Tag::List(start)) => {
            if cursor.subtree_contains_task_marker() {
                let whole = cursor.consume_balanced(range);
                return Some(source.unsupported_block("task list", whole, diagnostics));
            }
            Some(parse_list(cursor, source, diagnostics, start, range))
        }
        Event::Start(Tag::Table(alignments)) => {
            Some(parse_table(cursor, source, diagnostics, &alignments, range))
        }
        Event::Rule => Some(Block::ThematicBreak {
            source: Some(source.span(&range)),
        }),
        Event::Start(tag) => {
            let name = unsupported_block_name(&tag);
            let whole = cursor.consume_balanced(range);
            Some(source.unsupported_block(name, whole, diagnostics))
        }
        Event::Text(value) | Event::Code(value) => Some(Block::Paragraph {
            children: vec![Inline::Text {
                value: value.into_string(),
            }],
            layout: LayoutHint::default(),
            source: Some(source.span(&range)),
        }),
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
    Block::Preformatted {
        children: vec![Inline::Text { value }],
        language,
        layout: LayoutHint::default(),
        source: Some(source.span(&(start_range.start..end))),
    }
}

fn parse_list(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    start: Option<u64>,
    start_range: Range<usize>,
) -> Block {
    let mut items = Vec::new();
    let mut end = start_range.end;
    while let Some((event, range)) = cursor.next() {
        end = range.end;
        match event {
            Event::Start(Tag::Item) => {
                let (blocks, item_end) =
                    parse_blocks_until(cursor, source, diagnostics, TagEnd::Item);
                end = item_end;
                items.push(ListItem { blocks });
            }
            Event::End(TagEnd::List(_)) => break,
            Event::Start(tag) => {
                let whole = cursor.consume_balanced(range);
                items.push(ListItem {
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
            ListKind::Ordered
        } else {
            ListKind::Bullet
        },
        start,
        compact: !source.raw(&whole).contains("\n\n"),
        items,
        layout: LayoutHint::default(),
        source: Some(source.span(&whole)),
    }
}

fn parse_table(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
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
                let (row, row_end) =
                    parse_table_row(cursor, source, diagnostics, alignments, TagEnd::TableHead);
                end = row_end;
                rows.push(row);
            }
            Event::Start(Tag::TableRow) => {
                let (row, row_end) =
                    parse_table_row(cursor, source, diagnostics, alignments, TagEnd::TableRow);
                end = row_end;
                rows.push(row);
            }
            Event::End(TagEnd::Table) => break,
            _ => {}
        }
    }
    Block::Table {
        rows,
        layout: LayoutHint::default(),
        source: Some(source.span(&(start_range.start..end))),
    }
}

fn parse_table_row(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
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
                let (children, cell_end) =
                    parse_inlines(cursor, source, diagnostics, TagEnd::TableCell);
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
                    blocks,
                    column_span: 1,
                    row_span: 1,
                    alignment: alignments
                        .get(cells.len())
                        .and_then(|alignment| table_alignment(*alignment)),
                });
            }
            Event::End(actual) if actual == end_tag => break,
            Event::Start(tag) => {
                let whole = cursor.consume_balanced(range);
                cells.push(TableCell {
                    blocks: vec![source.unsupported_block(
                        unsupported_block_name(&tag),
                        whole,
                        diagnostics,
                    )],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                });
            }
            _ => {}
        }
    }
    (TableRow { cells }, end)
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
