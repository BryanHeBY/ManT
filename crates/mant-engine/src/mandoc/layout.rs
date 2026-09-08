//! Maps roff spacing and indentation measurements onto renderer-neutral layout hints.
//!
//! The block lowerer owns structural decisions.  This module owns the small,
//! shared rules that turn mandoc's display offsets and paragraph distances into
//! the `LayoutHint` values consumed by every output format.

use libmandoc_rs::Node;
use mant_ir::{Block, LayoutHint};

use crate::block::{block_layout, block_layout_mut};

mod definition;
mod distance;
pub(super) use definition::{DefinitionGeometry, TermPlacement};
pub(super) use distance::Distance;

/// Independent source formatter position, IR parent origin and man macro base.
/// Entering an owned description changes the IR parent, not the macro base;
/// only an RS scope establishes a new base for argument-less `in` restoration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SourceIndent {
    source: Distance,
    parent: Distance,
    // man_term's mt->offset is not the current output column or IR owner.
    // Entering a definition body must not change where argument-less in
    // restores; only an RS scope establishes a new macro base.
    macro_base: Distance,
}

impl SourceIndent {
    pub(super) fn absolute(self, distance: Distance) -> Self {
        Self {
            source: distance.add(Distance::cells(-5)).0.at_page_floor(),
            parent: self.parent,
            macro_base: self.macro_base,
        }
    }
    pub(super) fn relative_columns(self) -> i32 {
        self.source
            .position_columns()
            .saturating_sub(self.parent.position_columns())
    }

    pub(super) fn content_origin(self) -> Self {
        Self {
            source: self.source,
            parent: self.source,
            macro_base: self.macro_base,
        }
    }

    pub(super) fn macro_origin(self) -> Self {
        Self {
            source: self.macro_base,
            ..self
        }
    }

    pub(super) fn offset_from(self, other: Self) -> i32 {
        self.source
            .position_columns()
            .saturating_sub(other.source.position_columns())
    }
}

#[cfg(test)]
impl From<i32> for SourceIndent {
    fn from(columns: i32) -> Self {
        Self {
            source: Distance::cells(columns),
            parent: Distance::default(),
            macro_base: Distance::cells(columns),
        }
    }
}

impl super::LoweringContext<'_> {
    pub(super) fn check_gap_bounds(&self, blocks: &[Block]) {
        if mant_protocol::geometry::has_bounded_gap(blocks) {
            let mut diagnostics = self.diagnostics.borrow_mut();
            if !diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("manual.vertical-spacing-limit"))
            {
                diagnostics.push(mant_ir::Diagnostic {
                        level: mant_ir::DiagnosticLevel::Warning,
                        code: Some("manual.vertical-spacing-limit".into()),
                        message: "vertical spacing exceeds the 4096-row boundary limit; presentation is bounded".into(),
                        source: blocks.first().and_then(crate::block::block_source),
                    });
            }
        }
    }
    pub(super) fn offset_indent(
        &self,
        node: &Node,
        parent: SourceIndent,
        extra: Distance,
    ) -> SourceIndent {
        let (sum, bounded) = parent.source.add(extra);
        if bounded {
            self.warn_indent(node);
        }
        SourceIndent {
            source: sum.at_page_floor(),
            parent: parent.parent,
            macro_base: parent.macro_base,
        }
    }

    pub(super) fn distance_or(&self, node: &Node, argument: &str, fallback: Distance) -> Distance {
        self.checked_distance(node, argument).unwrap_or(fallback)
    }

    pub(super) fn checked_distance(&self, node: &Node, argument: &str) -> Option<Distance> {
        let distance = Distance::parse(argument);
        if distance.is_none() {
            self.warn_indent(node);
        }
        distance
    }

    pub(super) fn measured_mdoc_distance(
        &self,
        node: &Node,
        text: &str,
        fallback: Distance,
    ) -> Distance {
        // Unlike man distances, mdoc requires a unit; a bare number is a
        // printable width sample. Do not mistake digit-leading samples for
        // malformed numeric distances.
        if let Some((number, unit)) = text
            .trim()
            .split_at_checked(text.trim().len().saturating_sub(1))
            && matches!(
                unit,
                "n" | "m" | "u" | "c" | "f" | "i" | "M" | "P" | "v" | "p"
            )
            && number.parse::<f64>().is_ok()
        {
            return self.distance_or(node, text, fallback);
        }
        let visible = super::inline::plain_text(&super::inline::parse_roff_text(text));
        Distance::cells(mant_protocol::geometry::coordinate(
            mant_protocol::geometry::text_width(&visible),
        ))
    }

    pub(super) fn man_relative_indent(
        &self,
        node: &Node,
        parent: SourceIndent,
        prevailing: Distance,
    ) -> SourceIndent {
        let distance = first_part_argument(node).map_or(prevailing, |argument| {
            self.distance_or(node, argument, prevailing)
        });
        let mut scope = self.offset_indent(node, parent.macro_origin(), distance);
        scope.macro_base = scope.source;
        scope
    }

    pub(super) fn display_offset(&self, node: &Node) -> Distance {
        if matches!(node.macro_name.as_deref(), Some("D1" | "Dl")) {
            return Distance::cells(6);
        }
        match node.offset.as_deref() {
            None | Some("left") => Distance::default(),
            Some("indent") => Distance::cells(6),
            Some("indent-two") => Distance::cells(12),
            Some(offset) => self.measured_mdoc_distance(node, offset, Distance::default()),
        }
    }

    fn warn_indent(&self, node: &Node) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        if diagnostics
            .iter()
            .any(|item| item.code.as_deref() == Some("manual.indentation-limit"))
        {
            return;
        }
        diagnostics.push(mant_ir::Diagnostic {
            level: mant_ir::DiagnosticLevel::Warning,
            code: Some("manual.indentation-limit".into()),
            message: "unsupported or excessive indentation was bounded; invalid offsets use the default and cumulative indentation is limited to 4096 columns".into(),
            source: super::source_span(node),
        });
    }
}

pub(super) fn first_part_argument(node: &Node) -> Option<&str> {
    node.children
        .iter()
        .find(|child| child.kind == libmandoc_rs::NodeKind::Head)
        .and_then(first_text)
}

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
        *existing = existing.saturating_add(lines);
    } else if let Some(layout) = block_layout_mut(block) {
        layout.spacing_before_lines = layout.spacing_before_lines.saturating_add(lines);
    }
}

/// Return a block's indentation when it has one.
pub(super) fn block_indent(block: &Block) -> Option<i32> {
    block_layout(block).map(|layout| layout.indent_columns)
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
mod tests {
    use libmandoc_rs::{Node, NodeFlags, NodeKind};

    use super::{
        horizontal_distance_columns, layout, layout_with_spacing, paragraph_distance_lines,
        vertical_distance_lines,
    };
    use mant_ir::Block;

    fn node(kind: NodeKind, text: Option<&str>, offset: Option<&str>) -> Node {
        Node {
            kind,
            macro_name: None,
            text: text.map(ToOwned::to_owned),
            tag: None,
            line: 0,
            column: 0,
            flow_epoch: 0,
            flags: NodeFlags::default(),
            list_kind: None,
            definition_list_style: None,
            display_kind: None,
            font: None,
            author_mode: None,
            enclosure: None,
            compact: false,
            offset: offset.map(ToOwned::to_owned),
            width: None,
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
    fn normalizes_layout_hints() {
        assert_eq!(layout(3.into()).indent_columns, 3);
    }

    #[test]
    fn independent_paragraph_and_vertical_space_requests_are_not_erased() {
        let mut blocks = [
            Block::VerticalSpace {
                lines: 1,
                source: None,
            },
            Block::Paragraph {
                children: Vec::new(),
                layout: layout_with_spacing(4.into(), 1),
                source: None,
            },
        ];

        super::set_block_spacing(&mut blocks[0], 2);

        let Block::Paragraph { layout, .. } = &blocks[1] else {
            panic!("expected paragraph after explicit vertical space");
        };
        assert_eq!(layout.indent_columns, 4);
        assert_eq!(layout.spacing_before_lines, 1);
        assert!(matches!(blocks[0], Block::VerticalSpace { lines: 3, .. }));
    }

    #[test]
    fn converts_horizontal_roff_widths_to_terminal_columns() {
        assert_eq!(horizontal_distance_columns("20"), Some(20));
        assert_eq!(horizontal_distance_columns("8n"), Some(8));
        assert_eq!(horizontal_distance_columns("1i"), Some(10));
        assert_eq!(horizontal_distance_columns("24u"), Some(1));
        assert_eq!(horizontal_distance_columns("+2n"), None);
        assert_eq!(horizontal_distance_columns("wide"), None);
    }
}
