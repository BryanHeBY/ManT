//! Normalizes Markdown source spacing into the layout model shared with roff.
//!
//! `CommonMark` parsers retain a framing newline in fenced code and otherwise
//! leave blank-line presentation to HTML/CSS. `ManT` has no CSS layer, so this
//! pass makes those semantics explicit before any renderer sees the document.

use mant_ast::{Block, Inline, LayoutHint, ListItem, ListKind, Section, SectionRole, SourceSpan};

use super::source::MarkdownSource;

/// Apply source-derived block spacing and quick-reference conventions.
pub(super) fn normalize_markdown_layout(
    source: &MarkdownSource<'_>,
    root_blocks: &mut [Block],
    sections: &mut [Section],
) {
    normalize_blocks(source, root_blocks);
    normalize_sections(source, sections);
}

fn normalize_sections(source: &MarkdownSource<'_>, sections: &mut [Section]) {
    for section in sections {
        normalize_blocks(source, &mut section.blocks);
        if section.role == Some(SectionRole::QuickReference) {
            normalize_quick_reference(&mut section.blocks);
        }
        normalize_sections(source, &mut section.children);
    }
}

/// Preserve one visible row for a source blank line, just as man(7) lowering
/// records paragraph distance in `LayoutHint::spacing_before_lines`.
fn normalize_blocks(source: &MarkdownSource<'_>, blocks: &mut [Block]) {
    let mut previous_source = None;
    for block in blocks {
        if let (Some(previous), Some(current)) = (previous_source, block_source(block))
            && source.has_blank_line_between(previous, current)
            && let Some(layout) = block_layout_mut(block)
        {
            layout.spacing_before_lines = layout.spacing_before_lines.max(1);
        }

        normalize_nested_blocks(source, block);
        previous_source = block_source(block);
    }
}

fn normalize_nested_blocks(source: &MarkdownSource<'_>, block: &mut Block) {
    match block {
        Block::Preformatted { children, .. } => {
            trim_code_framing_newline(children);
        }
        Block::List { items, .. } => {
            for item in items {
                normalize_blocks(source, &mut item.blocks);
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                normalize_blocks(source, &mut item.description);
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                normalize_blocks(source, &mut cell.blocks);
            }
        }
        Block::Paragraph { .. }
        | Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => {}
    }
}

/// Real tldr-pages files alternate a one-item description list with a
/// standalone code paragraph. Lower that dialect into one marker-free list so
/// every renderer and the search index sees explicit description/command rows.
fn normalize_quick_reference(blocks: &mut Vec<Block>) {
    if blocks.is_empty() || !blocks.len().is_multiple_of(2) {
        return;
    }

    let mut examples = Vec::with_capacity(blocks.len() / 2);
    for pair in blocks.chunks_exact(2) {
        let Some(example) = tldr_example(pair) else {
            return;
        };
        examples.push(example);
    }

    let layout = block_layout(blocks.first()).unwrap_or_default();
    let source = merge_spans(
        blocks.first().and_then(block_source),
        blocks.last().and_then(block_source),
    );
    *blocks = vec![Block::List {
        kind: ListKind::Plain,
        start: None,
        compact: false,
        items: examples,
        layout,
        source,
    }];
}

fn tldr_example(pair: &[Block]) -> Option<ListItem> {
    let [description_list, command] = pair else {
        return None;
    };
    let Block::List {
        kind: ListKind::Bullet,
        items,
        ..
    } = description_list
    else {
        return None;
    };
    let [description_item] = items.as_slice() else {
        return None;
    };
    let [
        Block::Paragraph {
            children: description,
            layout: description_layout,
            source: description_source,
        },
    ] = description_item.blocks.as_slice()
    else {
        return None;
    };
    let Block::Paragraph {
        children: command,
        layout: command_layout,
        source: command_source,
    } = command
    else {
        return None;
    };
    if !matches!(command.as_slice(), [Inline::Code { .. }]) {
        return None;
    }

    let mut description = description.clone();
    trim_tldr_description(&mut description);
    Some(ListItem {
        blocks: vec![
            Block::Paragraph {
                children: description,
                layout: *description_layout,
                source: *description_source,
            },
            Block::Paragraph {
                children: command.clone(),
                layout: LayoutHint {
                    spacing_before_lines: command_layout.spacing_before_lines.max(1),
                    ..*command_layout
                },
                source: *command_source,
            },
        ],
    })
}

fn trim_tldr_description(children: &mut [Inline]) {
    let Some(last) = children.last_mut() else {
        return;
    };
    match last {
        Inline::Text { value } | Inline::Code { value } => {
            let trimmed = value.trim_end();
            if let Some(description) = trimmed.strip_suffix(':') {
                *value = description.to_owned();
            }
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::ExternalLink { children, .. }
        | Inline::EmailLink { children, .. }
        | Inline::ManualReference { children, .. }
        | Inline::SectionReference { children, .. } => trim_tldr_description(children),
        Inline::LineBreak | Inline::Anchor { .. } => {}
    }
}

fn block_layout(block: Option<&Block>) -> Option<LayoutHint> {
    match block? {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(*layout),
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}

fn merge_spans(start: Option<SourceSpan>, end: Option<SourceSpan>) -> Option<SourceSpan> {
    let start = start?;
    let end = end?;
    Some(SourceSpan {
        line: start.line,
        column: start.column,
        end_line: end.end_line,
        end_column: end.end_column,
    })
}

/// pulldown-cmark includes the newline before a closing fence in its text
/// event. It delimits source syntax and must not become an empty painted row.
fn trim_code_framing_newline(children: &mut Vec<Inline>) {
    let Some(last) = children.last_mut() else {
        return;
    };
    match last {
        Inline::Text { value } | Inline::Code { value } => {
            if value.ends_with('\n') {
                value.pop();
            }
        }
        Inline::LineBreak => {
            children.pop();
        }
        Inline::Strong { .. }
        | Inline::Emphasis { .. }
        | Inline::ExternalLink { .. }
        | Inline::EmailLink { .. }
        | Inline::ManualReference { .. }
        | Inline::SectionReference { .. }
        | Inline::Anchor { .. } => {}
    }
}

fn block_source(block: &Block) -> Option<SourceSpan> {
    match block {
        Block::Paragraph { source, .. }
        | Block::Preformatted { source, .. }
        | Block::List { source, .. }
        | Block::DefinitionList { source, .. }
        | Block::Table { source, .. }
        | Block::Equation { source, .. }
        | Block::VerticalSpace { source, .. }
        | Block::ThematicBreak { source }
        | Block::Unsupported { source, .. } => *source,
    }
}

fn block_layout_mut(block: &mut Block) -> Option<&mut LayoutHint> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout),
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}
