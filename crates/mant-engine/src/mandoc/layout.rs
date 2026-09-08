//! Source layout policy shared by every roff lowering consumer.
//!
//! Source coordinates and man scope restoration, mdoc width interpretation,
//! and vertical boundary requests have independent implementations. This
//! facade preserves the private lowering API and constructs neutral IR hints.

use libmandoc_rs::Node;
use mant_ir::{Block, LayoutHint};

use crate::block::block_layout;

mod definition;
mod distance;
mod mdoc;
mod source_indent;
mod spacing;
pub(super) use definition::{DefinitionGeometry, TermPlacement};
pub(super) use distance::Distance;
pub(super) use source_indent::SourceIndent;
pub(super) use spacing::{
    add_leading_spacing, man_paragraph_spacing, paragraph_distance_lines, section_spacing,
    set_block_spacing, update_paragraph_distance, vertical_distance_lines,
};

pub(super) fn first_part_argument(node: &Node) -> Option<&str> {
    node.children
        .iter()
        .find(|child| child.kind == libmandoc_rs::NodeKind::Head)
        .and_then(first_text)
}

/// Return a block's indentation when it has one.
pub(super) fn block_indent(block: &Block) -> Option<i32> {
    block_layout(block).map(|layout| layout.indent_columns)
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == libmandoc_rs::NodeKind::Text {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

/// Convert a horizontal roff measurement to terminal character columns.
///
/// mandoc's character device treats em and en units as one column. Physical
/// units use the same 10-characters-per-inch ratios as its ASCII renderer.
/// This intentionally accepts only absolute finite values: relative widths
/// depend on formatter state and should retain the caller's previous value.
#[cfg(test)]
fn horizontal_distance_columns(argument: &str) -> Option<usize> {
    let argument = argument.trim();
    if argument.starts_with(['+', '-']) {
        return None;
    }
    usize::try_from(Distance::parse(argument)?.columns()).ok()
}

/// Construct a zero-spacing layout at a semantic indentation level.
pub(super) fn layout(indent_columns: SourceIndent) -> LayoutHint {
    LayoutHint {
        indent_columns: indent_columns.relative_columns(),
        spacing_before_lines: 0,
        ..Default::default()
    }
}

/// Construct a layout that preserves an explicit leading vertical distance.
pub(super) fn layout_with_spacing(
    indent_columns: SourceIndent,
    spacing_before_lines: u16,
) -> LayoutHint {
    LayoutHint {
        indent_columns: indent_columns.relative_columns(),
        spacing_before_lines,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
