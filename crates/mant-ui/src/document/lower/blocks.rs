//! Dispatch source blocks while retaining direct cell-tail boundaries.
use super::super::{Block, LineSurface, LogicalLine, WrapMode, theme};
use super::DocumentBuilder;
use mant_ir::geometry::{compose_origin, padding};

impl DocumentBuilder<'_> {
    pub(in crate::document) fn blocks(&mut self, blocks: &[Block], base_indent: i32) {
        self.blocks_with_cell_context(blocks, base_indent, false);
    }

    pub(in crate::document) fn table_cell_blocks(&mut self, blocks: &[Block], base_indent: i32) {
        self.blocks_with_cell_context(blocks, base_indent, true);
    }

    fn blocks_with_cell_context(&mut self, blocks: &[Block], base_indent: i32, cell: bool) {
        let mut gap = mant_ir::geometry::GapPlan::default();
        for (index, block) in blocks.iter().enumerate() {
            gap.append_resolved(mant_ir::geometry::block_gap(block));
            if matches!(block, Block::VerticalSpace { .. }) {
                continue;
            }
            self.spacing(gap.rows(0));
            gap = mant_ir::geometry::GapPlan::default();
            if let Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            } = block
            {
                // A following block closes the paragraph terminator before
                // its own completed gap. Only the final direct Paragraph
                // gives a cell an open tail, as in the shared CLI cell flow.
                // Literal rows always retain their authored delimiters.
                let origin = compose_origin(base_indent, layout.indent_columns);
                self.inline_lines_with_geometry_tail(
                    mant_ir::InlineContentRef {
                        content: children,
                        layout: inline_layout,
                    },
                    origin,
                    compose_origin(origin, layout.continuation_indent_columns),
                    theme::style(theme::StyleRole::Text),
                    LineSurface::Normal,
                    !(cell && index + 1 == blocks.len()),
                );
            } else {
                self.block(block, base_indent);
            }
        }
        self.spacing(gap.rows(0));
    }

    pub(in crate::document) fn block(&mut self, block: &Block, base_indent: i32) {
        match block {
            Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            } => {
                let origin = compose_origin(base_indent, layout.indent_columns);
                self.inline_lines_with_geometry_tail(
                    mant_ir::InlineContentRef {
                        content: children,
                        layout: inline_layout,
                    },
                    origin,
                    compose_origin(origin, layout.continuation_indent_columns),
                    theme::style(theme::StyleRole::Text),
                    LineSurface::Normal,
                    true,
                );
            }
            Block::Preformatted {
                children,
                inline_layout,
                layout,
                ..
            } => {
                let origin = compose_origin(base_indent, layout.indent_columns);
                self.inline_lines_with_geometry(
                    mant_ir::InlineContentRef {
                        content: children,
                        layout: inline_layout,
                    },
                    origin,
                    compose_origin(origin, layout.continuation_indent_columns),
                    theme::style(theme::StyleRole::Text),
                    LineSurface::Code,
                );
            }
            Block::List {
                kind,
                compact,
                items,
                layout,
                ..
            } => {
                self.list(
                    *kind,
                    *compact,
                    items,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                self.definitions(
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
                self.table(
                    rows,
                    column_preferences,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::Equation { value, layout, .. } => {
                self.plain_block_lines(
                    value,
                    compose_origin(base_indent, layout.indent_columns),
                    theme::style(theme::StyleRole::Equation),
                    WrapMode::Character,
                );
            }
            Block::VerticalSpace { lines, .. } => self.spacing(*lines),
            Block::ThematicBreak { .. } => self.push(LogicalLine::rule(padding(base_indent))),
            Block::Unsupported { text, layout, .. } => {
                // mandoc mdoc_term.c::print_mdoc_node() breaks each authored
                // NODE_LINE in no-fill mode before terminal cell handling.
                self.plain_block_lines(
                    text,
                    compose_origin(base_indent, layout.indent_columns),
                    theme::style(theme::StyleRole::Unsupported),
                    WrapMode::Word,
                );
            }
        }
    }
}
