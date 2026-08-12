//! Normalizes Markdown source spacing into the layout model shared with roff.
//!
//! `CommonMark` parsers retain a framing newline in fenced code and otherwise
//! leave blank-line presentation to HTML/CSS. `ManT` has no CSS layer, so this
//! pass makes those semantics explicit before any renderer sees the document.

use mant_ast::{Block, Inline, LayoutHint, Section, SourceSpan};

use super::source::MarkdownSource;

/// Apply source-derived block spacing to the normalized document.
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
        | Inline::DocumentReference { .. }
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
