//! Outside decoration of already resolved sequential fields. Declared fields
//! are not reinterpreted as a parallel tbl grid by an optional outside frame.
use super::{Flow, LayoutText, Part};
use mant_ir::{TableAlignment, TableBorderStyle, geometry};

impl Flow {
    pub(in crate::output::text) fn frame_resolved_table(
        self,
        stroke: Option<TableBorderStyle>,
        alignment: Option<TableAlignment>,
        viewport: Option<usize>,
        origin: i32,
    ) -> Self {
        if !self.has_physical_rows() {
            return self;
        }
        let Some(stroke) = stroke else {
            return self.position_table(alignment, viewport, origin, None);
        };
        let parent = self
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) | Part::CompletedText(text) | Part::Literal(text) => text
                    .split(false)
                    .iter()
                    .filter(|row| !row.is_empty())
                    .map(LayoutText::leading_layout_padding)
                    .min(),
                Part::TableRule(_) | Part::Gap(_) | Part::CompleteGap => None,
            })
            .min()
            .unwrap_or_else(|| geometry::padding(origin))
            .min(geometry::padding(origin));
        let frame_origin = geometry::coordinate(parent);
        let width = self
            .parts
            .iter()
            .map(|part| match part {
                Part::Text(text) | Part::CompletedText(text) | Part::Literal(text) => text
                    .visible
                    .lines()
                    .map(|line| geometry::text_width(line).saturating_sub(parent))
                    .max()
                    .unwrap_or(0),
                Part::TableRule(rule) => geometry::text_width(&rule.render_default().visible),
                Part::Gap(_) | Part::CompleteGap => 0,
            })
            .max()
            .unwrap_or(0);
        if parent.saturating_add(width).saturating_add(4) > geometry::MAX_COLUMN_PADDING
            || viewport
                .is_some_and(|viewport| parent.saturating_add(width).saturating_add(4) > viewport)
        {
            return self.position_table(None, viewport, origin, None);
        }
        let (text, completed) = self.finish_cell();
        let (horizontal, vertical, corners) = match stroke {
            TableBorderStyle::Single => ('─', '│', ['┌', '┐', '└', '┘']),
            TableBorderStyle::Double => ('═', '║', ['╔', '╗', '╚', '╝']),
        };
        let edge = horizontal.to_string().repeat(width.saturating_add(2));
        let mut rows = vec![LayoutText::from(format!(
            "{}{edge}{}",
            corners[0], corners[1]
        ))];
        let parent_prefix = " ".repeat(parent);
        for row in text.split(completed) {
            let mut row = row.strip_prefix(&parent_prefix);
            let used = geometry::text_width(&row.visible);
            row = row.prefixed(&format!("{vertical} "));
            row.push_plain(&format!(
                "{} {vertical}",
                " ".repeat(width.saturating_sub(used))
            ));
            rows.push(row);
        }
        rows.push(LayoutText::from(format!(
            "{}{edge}{}",
            corners[2], corners[3]
        )));
        Self::text(LayoutText::join(rows, "\n").indented(parent)).position_table(
            alignment,
            viewport,
            frame_origin,
            None,
        )
    }
}
