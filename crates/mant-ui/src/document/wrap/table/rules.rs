//! Table rules use the same column preferences and output work bounds.
use super::super::{Line, LogicalTableLayout, RowCopyMap, Span, WrappedLine};
use super::content::table_column_widths;

pub(super) fn render_layout_rule(
    indent: usize,
    rules: &[mant_ir::TableRuleCellKind],
    layout: &LogicalTableLayout,
    width: usize,
) -> Vec<WrappedLine> {
    let indent = super::super::readable_origins(indent, indent, width).0;
    let available = width.saturating_sub(indent).max(1);
    let gap = usize::from(layout.column_preferences.gap_columns);
    if gap.saturating_mul(rules.len().saturating_sub(1)) > mant_ir::geometry::MAX_COLUMN_PADDING {
        return rules
            .iter()
            .map(|rule| WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::raw(match rule {
                        mant_ir::TableRuleCellKind::Horizontal => "─",
                        mant_ir::TableRuleCellKind::DoubleHorizontal => "═",
                    }),
                ]),
                links: Vec::new(),
                search_cells: Vec::new(),
                copy_map: RowCopyMap::default(),
            })
            .collect();
    }
    let widths = table_column_widths(&layout.preferred_widths, available, gap)
        .filter(|widths| widths.len() == rules.len())
        .unwrap_or_else(|| {
            let base = available.saturating_sub(rules.len().saturating_sub(1) * gap);
            vec![(base / rules.len().max(1)).max(1); rules.len()]
        });
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    for (index, (rule, width)) in rules.iter().zip(widths).enumerate() {
        if index != 0 {
            spans.push(Span::raw(" ".repeat(gap)));
        }
        let glyph = match rule {
            mant_ir::TableRuleCellKind::Horizontal => '─',
            mant_ir::TableRuleCellKind::DoubleHorizontal => '═',
        };
        spans.push(Span::raw(glyph.to_string().repeat(width)));
    }
    vec![WrappedLine {
        source_end: None,
        anchors: Vec::new(),
        line: Line::from(spans),
        links: Vec::new(),
        search_cells: Vec::new(),
        copy_map: RowCopyMap::default(),
    }]
}
