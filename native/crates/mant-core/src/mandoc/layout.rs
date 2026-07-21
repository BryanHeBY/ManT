//! Maps roff spacing and indentation measurements onto renderer-neutral layout hints.
//!
//! The block lowerer owns structural decisions.  This module owns the small,
//! shared rules that turn mandoc's display offsets and paragraph distances into
//! the `LayoutHint` values consumed by every output format.

use libmandoc_rs::Node;
use mant_ast::{Block, LayoutHint};

/// Update the current man(7) paragraph distance after a `.PD` request.
pub(super) fn update_paragraph_distance(node: &Node, paragraph_distance: &mut u16) {
    if node.macro_name.as_deref() == Some("PD")
        && let Some(lines) = paragraph_distance_lines(node)
    {
        *paragraph_distance = lines;
    }
}

/// Determine the visible spacing before a section heading.
pub(super) fn section_spacing(
    node: &Node,
    is_first: bool,
    has_preceding_content: bool,
    paragraph_distance: u16,
) -> u16 {
    match node.macro_name.as_deref() {
        // man(7) uses the current `.PD` value, except before the first heading
        // at a level and after an empty peer section.
        Some("SH" | "SS") => {
            if has_preceding_content {
                paragraph_distance
            } else {
                0
            }
        }
        // mdoc(7) gives top-level sections one row even before the first Sh;
        // Ss only receives it when visible content precedes the heading.
        Some("Sh") => u16::from(is_first || has_preceding_content),
        Some("Ss") => u16::from(has_preceding_content),
        _ => 0,
    }
}

/// Preserve leading space by attaching it to the first visible nested block.
pub(super) fn add_leading_spacing(blocks: &mut [Block], lines: u16) {
    if lines == 0 {
        return;
    }
    let Some(first) = blocks.first_mut() else {
        return;
    };
    set_block_spacing(first, lines);
}

/// Increase the leading spacing on a semantic block without losing an
/// existing explicit vertical-space node.
pub(super) fn set_block_spacing(block: &mut Block, lines: u16) {
    if let Block::VerticalSpace {
        lines: existing, ..
    } = block
    {
        *existing = (*existing).max(lines);
    } else if let Some(layout) = block_layout_mut(block) {
        layout.spacing_before_lines = layout.spacing_before_lines.max(lines);
    }
}

/// Return a block's layout when it has one.
pub(super) fn block_layout_mut(block: &mut Block) -> Option<&mut LayoutHint> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout),
        Block::VerticalSpace { .. } => None,
    }
}

/// Return a block's indentation when it has one.
pub(super) fn block_indent(block: &Block) -> Option<u16> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout.indent_columns),
        Block::VerticalSpace { .. } => None,
    }
}

/// Convert a `.PD` measurement to terminal rows using mandoc's unit ratios.
/// Missing arguments restore man(7)'s one-row default; invalid values retain
/// the previous state.
pub(super) fn paragraph_distance_lines(node: &Node) -> Option<u16> {
    let Some(argument) = first_text(node) else {
        return Some(1);
    };
    distance_lines(argument)
}

/// Convert an explicit vertical-space request to terminal rows.
pub(super) fn vertical_distance_lines(node: &Node) -> Option<u16> {
    first_text(node).map_or(Some(1), distance_lines)
}

fn distance_lines(argument: &str) -> Option<u16> {
    let argument = argument.trim();
    let number_end = argument
        .find(|character: char| character.is_ascii_alphabetic())
        .unwrap_or(argument.len());
    let scale = argument[..number_end].parse::<f64>().ok()?;
    if !scale.is_finite() {
        return None;
    }
    let unit = argument[number_end..].trim();
    let vertical_rows = match unit {
        "u" => scale / 40.0,
        "c" => scale * 6.0 / 2.54,
        "f" => scale * 65_536.0 / 40.0,
        "i" => scale * 6.0,
        "M" => scale * 0.006,
        "p" => scale / 12.0,
        "m" | "n" => scale * 0.6,
        // `P`, `v`, no suffix, and unknown suffixes retain the vertical scale.
        _ => scale,
    };

    // Equivalent to mandoc's positive rounding in term_vspan(), without a
    // lossy float cast, including its fallback for unusually large values.
    for lines in 0_u16..66 {
        if vertical_rows < f64::from(lines) + 0.5005 {
            return Some(lines);
        }
    }
    Some(1)
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == libmandoc_rs::NodeKind::Text {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

/// Construct a zero-spacing layout at a semantic indentation level.
pub(super) const fn layout(indent_columns: u16) -> LayoutHint {
    LayoutHint {
        indent_columns,
        spacing_before_lines: 0,
    }
}

/// Construct a layout that preserves an explicit leading vertical distance.
pub(super) const fn layout_with_spacing(
    indent_columns: u16,
    spacing_before_lines: u16,
) -> LayoutHint {
    LayoutHint {
        indent_columns,
        spacing_before_lines,
    }
}

/// Translate mandoc display offsets to terminal columns.
pub(super) fn display_indent(node: &Node) -> u16 {
    let Some(offset) = node.offset.as_deref() else {
        return 4;
    };
    if offset == "left" {
        return 0;
    }
    if offset == "indent" {
        return 4;
    }
    offset
        .trim_end_matches(|character: char| character.is_ascii_alphabetic())
        .parse()
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use libmandoc_rs::{Node, NodeFlags, NodeKind};

    use super::{display_indent, layout, paragraph_distance_lines, vertical_distance_lines};

    fn node(kind: NodeKind, text: Option<&str>, offset: Option<&str>) -> Node {
        Node {
            kind,
            macro_name: None,
            text: text.map(ToOwned::to_owned),
            tag: None,
            line: 0,
            column: 0,
            flags: NodeFlags::default(),
            list_kind: None,
            display_kind: None,
            compact: false,
            offset: offset.map(ToOwned::to_owned),
            table_cells: Vec::new(),
            equation: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn converts_mandoc_vertical_units_to_terminal_rows() {
        let empty = node(NodeKind::Root, None, None);
        assert_eq!(paragraph_distance_lines(&empty), Some(1));
        assert_eq!(
            vertical_distance_lines(&node(NodeKind::Text, Some("2v"), None)),
            Some(2)
        );
        assert_eq!(
            vertical_distance_lines(&node(NodeKind::Text, Some("1i"), None)),
            Some(6)
        );
        assert_eq!(
            vertical_distance_lines(&node(NodeKind::Text, Some("not-a-number"), None)),
            None
        );
    }

    #[test]
    fn normalizes_display_offsets_and_layout_hints() {
        assert_eq!(display_indent(&node(NodeKind::Root, None, None)), 4);
        assert_eq!(display_indent(&node(NodeKind::Root, None, Some("left"))), 0);
        assert_eq!(display_indent(&node(NodeKind::Root, None, Some("8n"))), 8);
        assert_eq!(layout(3).indent_columns, 3);
    }
}
