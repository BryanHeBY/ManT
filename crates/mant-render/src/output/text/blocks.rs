//! One source-aware block layout for plain and decorated text.
//! Decorators must preserve visible content and boundary whitespace.
use super::flow::Flow;
use super::indent_lines;
use super::layout::LayoutText;
use crate::presentation::{EntryStyleMap, TextPresentation, TextRole, visit_inline_text};
use mant_ir::geometry::{compose_origin, coordinate, marker_run_in_gap, padding};
use mant_ir::{
    Block, DefinitionItem, InlineContentRef, ListItem, ListKind, Section, resolve_row_origins,
};
#[cfg(test)]
use mant_ir::{Inline, TableCell};

#[cfg(test)]
mod cell_layout_tests;
mod lists;
mod tables;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod visits;

pub(in crate::output) struct BlockRenderer<'a> {
    pub(in crate::output) names: Option<EntryStyleMap<'a>>,
    pub(in crate::output) decorate: &'a dyn Fn(TextPresentation, &str) -> String,
    pub(in crate::output) locations: Option<&'a super::super::styles::LocatedStyles<'a>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ParagraphTail {
    BlockBoundary,
    OpenCellRow,
}

impl BlockRenderer<'_> {
    pub(super) fn paint(&self, role: TextRole, text: &str) -> String {
        (self.decorate)(role.into(), text)
    }

    /// Decorate authoritative text once, then attach the complete owner's
    /// signed correction to every logical hard row. Generated padding never
    /// enters the scalar domain used by names and matches.
    pub(super) fn inline_rows(
        &self,
        content: InlineContentRef<'_>,
        role: TextRole,
    ) -> Vec<(LayoutText, i32)> {
        let children = content.content;
        let mut rows = vec![LayoutText::default()];
        let names = self
            .names
            .as_ref()
            .map_or(&[][..], |map| map.ranges(children));
        let mut append = |presentation: TextPresentation, value: &str| {
            #[cfg(test)]
            visits::inline();
            let decorated = LayoutText::decorated(value, (self.decorate)(presentation, value));
            // Decoration is applied once. Measurements retain the original
            // fragments and are composed before measuring a complete row.
            for (index, piece) in decorated.split(false).into_iter().enumerate() {
                if index > 0 {
                    rows.push(LayoutText::default());
                }
                rows.last_mut().expect("open row").append(&piece);
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
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| (row, content.layout.row_indent(index)))
            .collect()
    }

    pub(in crate::output) fn inline_text(
        &self,
        content: InlineContentRef<'_>,
        role: TextRole,
    ) -> String {
        self.inline_layout(content, role).rendered
    }

    pub(super) fn inline_layout(
        &self,
        content: InlineContentRef<'_>,
        role: TextRole,
    ) -> LayoutText {
        self.inline_layout_at(content, role, 0)
    }

    fn inline_layout_at(
        &self,
        content: InlineContentRef<'_>,
        role: TextRole,
        origin: i32,
    ) -> LayoutText {
        LayoutText::join(
            self.inline_rows(content, role)
                .into_iter()
                .map(|(row, indent)| {
                    row.indented(padding(
                        resolve_row_origins(origin, origin, indent).first_visual_origin,
                    ))
                }),
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
        let heading_origin = coordinate(depth.saturating_mul(2));
        let mut output = Flow::default();
        output.gap(section.spacing_before_lines);
        output.push_text(self.inline_layout_at(
            InlineContentRef {
                content: &section.heading.content,
                layout: &section.heading.inline_layout,
            },
            TextRole::Heading,
            heading_origin,
        ));
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
        self.block_flow_with_tail(blocks, base_indent, ParagraphTail::BlockBoundary)
    }

    /// A table cell's final hard break opens a row for the next field.
    /// Ordinary block composition closes that same tail at its block join;
    /// completed vertical rows continue to belong to the gap flow.
    fn cell_block_flow(&self, blocks: &[Block], base_indent: i32) -> Flow {
        self.block_flow_with_tail(blocks, base_indent, ParagraphTail::OpenCellRow)
    }

    fn block_flow_with_tail(
        &self,
        blocks: &[Block],
        base_indent: i32,
        tail: ParagraphTail,
    ) -> Flow {
        let mut output = Flow::default();
        for (index, block) in blocks.iter().enumerate() {
            output.gap(mant_ir::geometry::block_gap(block));
            let tail = if index + 1 == blocks.len() {
                tail
            } else {
                ParagraphTail::BlockBoundary
            };
            output.extend(self.render_block_with_tail(block, base_indent, tail));
        }
        output
    }

    fn render_block(&self, block: &Block, base_indent: i32) -> Flow {
        self.render_block_with_tail(block, base_indent, ParagraphTail::BlockBoundary)
    }

    fn render_block_with_tail(&self, block: &Block, base_indent: i32, tail: ParagraphTail) -> Flow {
        #[cfg(test)]
        visits::block();
        let (value, layout_indent) = match block {
            Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            } => {
                return self.paragraph_flow(
                    InlineContentRef {
                        content: children,
                        layout: inline_layout,
                    },
                    layout,
                    base_indent,
                    tail,
                );
            }
            Block::Preformatted {
                children,
                inline_layout,
                layout,
                ..
            } => {
                return self.preformatted_flow(
                    InlineContentRef {
                        content: children,
                        layout: inline_layout,
                    },
                    layout,
                    base_indent,
                );
            }
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
                column_preferences,
                layout,
                ..
            } => {
                let origin = compose_origin(base_indent, layout.indent_columns);
                return self.table_flow(rows, column_preferences, origin);
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
        content: InlineContentRef<'_>,
        layout: &mant_ir::LayoutHint,
        base_indent: i32,
    ) -> Flow {
        // Literal newlines and whitespace are content, not layout requests.
        // They survive even when the entire block contains only blanks.
        if !mant_ir::geometry::has_literal_rows(content.content) {
            return Flow::default();
        }
        let origin = compose_origin(base_indent, layout.indent_columns);
        let continuation_origin = compose_origin(origin, layout.continuation_indent_columns);
        Flow::literal(LayoutText::join(
            self.inline_rows(content, TextRole::Body)
                .into_iter()
                .enumerate()
                .map(|(index, (row, indent))| {
                    let first = if index == 0 {
                        origin
                    } else {
                        continuation_origin
                    };
                    row.indented(padding(
                        resolve_row_origins(first, continuation_origin, indent).first_visual_origin,
                    ))
                }),
            "\n",
        ))
    }

    fn paragraph_flow(
        &self,
        content: InlineContentRef<'_>,
        layout: &mant_ir::LayoutHint,
        base_indent: i32,
        tail: ParagraphTail,
    ) -> Flow {
        let mut rows = self.inline_rows(content, TextRole::Body);
        if rows.len() == 1 && rows[0].0.visible.is_empty() {
            return Flow::default();
        }
        let completed_empty_tail = Self::close_paragraph_rows(&mut rows, tail);
        let first_origin = compose_origin(base_indent, layout.indent_columns);
        let continuation_origin = compose_origin(first_origin, layout.continuation_indent_columns);
        let value = LayoutText::join(
            rows.into_iter().enumerate().map(|(index, (line, indent))| {
                let origin = if index == 0 {
                    first_origin
                } else {
                    continuation_origin
                };
                // An empty physical row has no device advance. Preserve its
                // opaque decoration without turning the row origin into
                // authored blank cells (term_ascii.c::ascii_endline()).
                line.indented(padding(
                    resolve_row_origins(origin, continuation_origin, indent).first_visual_origin,
                ))
            }),
            "\n",
        );
        if completed_empty_tail {
            Flow::completed_text(value)
        } else {
            Flow::text(value)
        }
    }

    fn close_paragraph_rows(rows: &mut Vec<(LayoutText, i32)>, tail: ParagraphTail) -> bool {
        if tail == ParagraphTail::BlockBoundary
            && rows.len() > 1
            && rows.last().is_some_and(|(row, _)| row.is_empty())
        {
            let (tail, _) = rows.pop().expect("open trailing row");
            if let Some((row, _)) = rows.last_mut() {
                row.append(&tail);
            }
            return rows.last().is_some_and(|(row, _)| row.is_empty());
        }
        false
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
