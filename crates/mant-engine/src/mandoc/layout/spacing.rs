//! Vertical source distances, paragraph requests and explicit boundary gaps.
use libmandoc_rs::Node;
use mant_ir::Block;

use super::first_text;
use mant_ir::geometry::block_layout_mut;

/// Resolve man(7)'s `print_bvspace` prerequisite before materializing IR.
/// Transparent RS scopes may carry a predecessor outside a detached output
/// buffer. Conversely a first tagged paragraph has none, even when it will
/// become an ordered list. Rendered container emptiness cannot decide this.
pub(in crate::mandoc) const fn man_paragraph_spacing(
    paragraph_distance: u16,
    has_predecessor: bool,
) -> u16 {
    if has_predecessor {
        paragraph_distance
    } else {
        0
    }
}

impl crate::mandoc::LoweringContext<'_> {
    pub(in crate::mandoc) fn check_gap_bounds(&self, blocks: &[Block]) {
        if mant_ir::geometry::has_bounded_gap(blocks) {
            let mut diagnostics = self.diagnostics.borrow_mut();
            if !diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("manual.vertical-spacing-limit"))
            {
                diagnostics.push(mant_ir::Diagnostic {
                        impact: mant_ir::DiagnosticImpact::None,
                        level: mant_ir::DiagnosticLevel::Warning,
                        code: Some("manual.vertical-spacing-limit".into()),
                        message: "vertical spacing exceeds the 4096-row boundary limit; presentation is bounded".into(),
                        source: blocks.first().and_then(mant_ir::geometry::block_source),
                    });
            }
        }
    }
}

/// Update the current man(7) paragraph distance after a `.PD` request.
pub(in crate::mandoc) fn update_paragraph_distance(node: &Node, paragraph_distance: &mut u16) {
    if node.macro_name.as_deref() == Some("PD")
        && let Some(lines) = paragraph_distance_lines(node)
    {
        *paragraph_distance = lines;
    }
}

/// Determine the visible spacing before a section heading.
pub(in crate::mandoc) fn section_spacing(
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
pub(in crate::mandoc) fn add_leading_spacing(blocks: &mut [Block], lines: u16) {
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
pub(in crate::mandoc) fn set_block_spacing(block: &mut Block, lines: u16) {
    if let Block::VerticalSpace {
        lines: existing, ..
    } = block
    {
        *existing = existing.saturating_add(lines);
    } else if let Some(layout) = block_layout_mut(block) {
        layout.spacing_before_lines = layout.spacing_before_lines.saturating_add(lines);
    }
}

/// Convert a `.PD` measurement to terminal rows using mandoc's unit ratios.
/// Missing arguments restore man(7)'s one-row default; invalid values retain
/// the previous state.
pub(in crate::mandoc) fn paragraph_distance_lines(node: &Node) -> Option<u16> {
    let Some(argument) = first_text(node) else {
        return Some(1);
    };
    distance_lines(argument)
}

/// Convert an explicit vertical-space request to terminal rows.
pub(in crate::mandoc) fn vertical_distance_lines(node: &Node) -> Option<u16> {
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
