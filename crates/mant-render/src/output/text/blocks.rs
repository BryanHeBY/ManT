//! One source-aware block layout for plain and decorated text.
//! Decorators must preserve visible content and boundary whitespace.
use super::flow::Flow;
use super::indent_lines;
use super::layout::LayoutText;
use crate::presentation::{
    EntryStyleMap, InlinePresentation, TextPresentation, TextRole, visit_inline_text,
};
use mant_ir::geometry::{compose_origin, coordinate, marker_run_in_gap, padding, text_width};
use mant_ir::{Block, DefinitionItem, Inline, ListItem, ListKind, Section, TableCell};

#[cfg(test)]
mod cell_layout_tests;
mod lists;
mod tables;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod visits;

pub(super) struct BlockRenderer<'a> {
    pub(super) names: Option<EntryStyleMap<'a>>,
    pub(super) decorate: &'a dyn Fn(TextPresentation, &str) -> String,
    pub(super) locations: Option<&'a super::super::styles::LocatedStyles<'a>>,
}

impl BlockRenderer<'_> {
    pub(super) fn paint(&self, role: TextRole, text: &str) -> String {
        (self.decorate)(role.into(), text)
    }

    /// Rows of a term with each row's request-relative indent. A
    /// [`LineBreak`](Inline::LineBreak) closes its row and carries the next
    /// row's indent (a cleared-BRIND request moved the upstream offset,
    /// roff_term.c:73-75); wrapped text rows inherit the current indent.
    pub(super) fn inline_rows(
        &self,
        children: &[Inline],
        role: TextRole,
    ) -> Vec<(LayoutText, u16)> {
        let mut rows = vec![(LayoutText::default(), 0)];
        let mut next_indent = 0_u16;
        let names = self
            .names
            .as_ref()
            .map_or(&[][..], |map| map.ranges(children));
        let mut append = |presentation: TextPresentation, value: &str| {
            #[cfg(test)]
            visits::inline();
            let decorated = LayoutText::decorated(value, (self.decorate)(presentation, value));
            if let InlinePresentation {
                line_break_indent: Some(indent),
                ..
            } = presentation.inline
            {
                next_indent = indent;
            }
            // Decoration is applied once. Measurements retain the original
            // fragments and are composed before measuring a complete row.
            for (index, piece) in decorated.split(false).into_iter().enumerate() {
                if index > 0 {
                    let indent = next_indent;
                    next_indent = 0;
                    rows.push((LayoutText::default(), indent));
                }
                rows.last_mut().expect("open row").0.append(&piece);
            }
        };
        if let Some(locations) = self.locations {
            locations.visit_inline(children, role, append);
        } else {
            // Generic links consume only IR visible content; mdoc `Lk`
            // terminal expansion (generated colon and address) is executed
            // in the shared lowering, not appended here.
            visit_inline_text(children, names, |inline, _, value| {
                append(
                    TextPresentation {
                        role,
                        inline,
                        matched: false,
                    },
                    value,
                );
            });
        }
        rows
    }

    pub(super) fn inline_text(&self, children: &[Inline], role: TextRole) -> String {
        self.inline_layout(children, role).rendered
    }

    pub(super) fn inline_layout(&self, children: &[Inline], role: TextRole) -> LayoutText {
        LayoutText::join(
            self.inline_rows(children, role)
                .into_iter()
                .map(|(row, indent)| row.indented(padding(i32::from(indent)))),
            "\n",
        )
    }

    pub(super) fn sections_flow(&self, sections: &[Section], depth: usize) -> Flow {
        let mut output = Flow::default();
        for section in sections {
            output.extend(self.section_flow(section, depth));
        }
        output
    }

    pub(super) fn render_section(&self, section: &Section, depth: usize) -> String {
        self.section_flow(section, depth).finish(false)
    }

    fn section_flow(&self, section: &Section, depth: usize) -> Flow {
        let heading_indent = "  ".repeat(depth);
        let mut output = Flow::default();
        output.gap(section.spacing_before_lines);
        output.push_text(
            self.inline_layout(&section.heading.content, TextRole::Heading)
                .prefixed(&heading_indent),
        );
        output.extend(self.block_flow(&section.blocks, coordinate(depth.saturating_mul(2))));
        output.extend(self.sections_flow(&section.children, depth + 1));
        output
    }

    pub(super) fn render_blocks(&self, blocks: &[Block], base_indent: i32) -> String {
        self.render_block_sequence(blocks, base_indent, None)
    }

    pub(super) fn render_block_sequence(
        &self,
        blocks: &[Block],
        base_indent: i32,
        leading_gap: Option<usize>,
    ) -> String {
        self.block_flow(blocks, base_indent)
            .finish(leading_gap.is_some())
    }

    pub(super) fn block_flow(&self, blocks: &[Block], base_indent: i32) -> Flow {
        let mut output = Flow::default();
        for block in blocks {
            output.gap(mant_ir::geometry::block_gap(block));
            output.extend(self.render_block(block, base_indent));
        }
        output
    }

    fn render_block(&self, block: &Block, base_indent: i32) -> Flow {
        #[cfg(test)]
        visits::block();
        let (value, layout_indent) = match block {
            Block::Paragraph {
                children, layout, ..
            } => return self.paragraph_flow(children, layout, base_indent),
            Block::Preformatted {
                children, layout, ..
            } => return self.preformatted_flow(children, layout, base_indent),
            Block::List {
                kind,
                items,
                compact,
                layout,
                ..
            } => {
                return self.render_list(
                    *kind,
                    items,
                    *compact,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                return self.render_definitions(
                    items,
                    *compact,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::Table {
                rows,
                column_widths,
                layout,
                ..
            } => {
                let origin = compose_origin(base_indent, layout.indent_columns);
                return self.table_flow(rows, column_widths, origin);
            }
            Block::Equation { value, layout, .. }
            | Block::Unsupported {
                text: value,
                layout,
                ..
            } => (
                LayoutText::decorated(
                    value,
                    self.locations.map_or_else(
                        || self.paint(TextRole::Body, value),
                        |locations| locations.text(value, self.decorate),
                    ),
                ),
                layout.indent_columns,
            ),
            // Vertical space is handled as an inter-block separator in
            // `render_blocks`, never as a standalone rendered block.
            Block::VerticalSpace { .. } => return Flow::default(),
            Block::ThematicBreak { .. } => ("---".into(), 0),
        };
        Self::nonliteral_leaf(&value, compose_origin(base_indent, layout_indent))
    }

    fn preformatted_flow(
        &self,
        children: &[mant_ir::Inline],
        layout: &mant_ir::LayoutHint,
        base_indent: i32,
    ) -> Flow {
        // Literal newlines and whitespace are content, not layout requests.
        // They survive even when the entire block contains only blanks.
        if !mant_ir::geometry::has_literal_rows(children) {
            return Flow::default();
        }
        let origin = compose_origin(base_indent, layout.indent_columns);
        Flow::literal(LayoutText::join(
            self.inline_rows(children, TextRole::Body)
                .into_iter()
                .map(|(row, indent)| {
                    row.indented(padding(compose_origin(origin, i32::from(indent))))
                }),
            "\n",
        ))
    }

    fn paragraph_flow(
        &self,
        children: &[mant_ir::Inline],
        layout: &mant_ir::LayoutHint,
        base_indent: i32,
    ) -> Flow {
        let mut rows = self.inline_rows(children, TextRole::Body);
        if rows.iter().all(|(row, _)| row.visible.trim().is_empty()) {
            return Flow::default();
        }
        while rows.last().is_some_and(|(row, _)| row.is_empty()) {
            let (tail, _) = rows.pop().expect("trailing row");
            if let Some((row, _)) = rows.last_mut() {
                row.append(&tail);
            }
        }
        let first_origin = compose_origin(base_indent, layout.indent_columns);
        Flow::text(LayoutText::join(
            rows.into_iter().enumerate().map(|(index, (line, indent))| {
                let origin = if index == 0 {
                    first_origin
                } else {
                    compose_origin(first_origin, layout.continuation_indent_columns)
                };
                line.prefixed(&" ".repeat(padding(compose_origin(origin, i32::from(indent)))))
            }),
            "\n",
        ))
    }

    fn nonliteral_leaf(value: &LayoutText, origin: i32) -> Flow {
        let value = value.trim_newlines();
        if value.visible.trim().is_empty() {
            Flow::default()
        } else {
            Flow::text(value.indented(padding(origin)))
        }
    }
}
