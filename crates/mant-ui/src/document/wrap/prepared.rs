//! Project shared source units onto theme and link cells exactly once.

use mant_render::line_layout::{PreparedLine, UnitKind};

use super::{LogicalLine, Style, cells::StyledCell, links::LinkCursor};

pub(super) struct StyledUnits {
    pub(super) cells: Vec<StyledCell>,
    /// Scalar-cell boundaries for every source display unit, plus final end.
    pub(super) offsets: Vec<usize>,
}

pub(super) fn styled_units(line: &LogicalLine, prepared: &PreparedLine<'_>) -> StyledUnits {
    let mut cells = Vec::new();
    let mut offsets = Vec::with_capacity(prepared.units().len() + 1);
    let mut links = LinkCursor::new(&line.links);
    let mut spans = line.spans.iter();
    let mut span = spans.next();
    let mut span_end = span.map_or(0, |span| span.content.len());
    for unit in prepared.units() {
        #[cfg(test)]
        super::workload::visit(1);
        offsets.push(cells.len());
        while unit.bytes.start >= span_end {
            span = spans.next();
            span_end += span.map_or(0, |span| span.content.len());
            if span.is_none() {
                break;
            }
        }
        let style = span.map_or_else(Style::default, |span| span.style);
        let link_index = links.grapheme(unit.scalars.start, unit.scalars.len());
        let copyable = |source_index| {
            !line
                .layout_scalars
                .iter()
                .any(|range| range.contains(&source_index))
        };
        if matches!(unit.kind, UnitKind::Tab { .. }) {
            cells.push(StyledCell {
                source_index: unit.scalars.start,
                character: '\t',
                display_character: Some(' '),
                grapheme_start: true,
                width: unit.columns,
                style,
                link_index,
                copyable: copyable(unit.scalars.start),
            });
            continue;
        }
        // Whole-row segmentation allows a glyph to cross style boundaries.
        // Its first scalar owns the style and its complete width; all source
        // scalars still receive search coordinates for that same glyph.
        for (index, character) in prepared.source()[unit.bytes.clone()].chars().enumerate() {
            let character = if character.is_control() {
                '\u{fffd}'
            } else {
                character
            };
            let source_index = unit.scalars.start + index;
            cells.push(StyledCell {
                source_index,
                character,
                display_character: Some(character),
                grapheme_start: index == 0,
                width: if index == 0 { unit.columns } else { 0 },
                style,
                link_index,
                copyable: copyable(source_index),
            });
        }
    }
    offsets.push(cells.len());
    StyledUnits { cells, offsets }
}
