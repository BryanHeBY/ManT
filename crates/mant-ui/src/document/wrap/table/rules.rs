//! Rules paint the already selected table region, including stacked fallback.
use super::super::{Line, RowCopyMap, Span, WrappedLine};

pub(super) fn render_rules(
    indent: usize,
    whole: Option<bool>,
    rules: &[mant_ir::TableRuleCellKind],
    widths: Option<&[usize]>,
    gap: usize,
    extent: usize,
) -> Vec<WrappedLine> {
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    if let Some(double) = whole {
        spans.push(rule_span(if double { '═' } else { '─' }, extent));
    } else if let Some(widths) = widths.filter(|widths| !widths.is_empty()) {
        for (index, width) in widths.iter().enumerate() {
            if index != 0 {
                spans.push(Span::raw(" ".repeat(gap)));
            }
            if let Some(rule) = rules.get(index) {
                spans.push(rule_span(glyph(*rule), *width));
            } else {
                // An unoccupied declared column has no authored rule cell.
                spans.push(Span::raw(" ".repeat(*width)));
            }
        }
    } else {
        // No rectangular grid survives a stack. Preserve every actual rule's
        // strength in source order, with bounded total work, even for more
        // rule cells than fit on one row. Never amplify N rules by N columns.
        return rules
            .chunks(extent.max(1))
            .map(|chunk| {
                let mut spans = vec![Span::raw(" ".repeat(indent))];
                for (index, rule) in chunk.iter().enumerate() {
                    let width = extent / chunk.len() + usize::from(index < extent % chunk.len());
                    spans.push(rule_span(glyph(*rule), width));
                }
                row(spans)
            })
            .collect();
    }
    vec![row(spans)]
}

fn row(spans: Vec<Span<'static>>) -> WrappedLine {
    WrappedLine {
        source_end: None,
        anchors: Vec::new(),
        line: Line::from(spans),
        links: Vec::new(),
        search_cells: Vec::new(),
        copy_map: RowCopyMap::default(),
    }
}

fn rule_span(glyph: char, width: usize) -> Span<'static> {
    Span::styled(
        glyph.to_string().repeat(width),
        crate::theme::style(crate::theme::StyleRole::Rule),
    )
}

fn glyph(kind: mant_ir::TableRuleCellKind) -> char {
    match kind {
        mant_ir::TableRuleCellKind::Horizontal => '─',
        mant_ir::TableRuleCellKind::DoubleHorizontal => '═',
    }
}
