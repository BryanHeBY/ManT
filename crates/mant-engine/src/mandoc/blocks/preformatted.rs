//! Preserves no-fill and literal display content as preformatted blocks.

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Block, Inline};

use super::super::{
    LoweringContext, first_part_children,
    inline::{InlineBuilder, append_inline_node_with_next},
    layout::{layout, vertical_distance_lines},
    source_span,
};
use super::tables::{TableEmbeddingPlan, append_table_row};

pub(super) fn preformatted_blocks(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    let mut flow = DisplayFlow {
        output: Vec::new(),
        line: InlineBuilder::with_spacing(spacing_enabled),
        source: None,
        context,
        indent_columns,
        formatter: *formatter,
    };
    flow.line.font = formatter.font;
    flow.line.track_source_lines(context.no_fill_source_rows());
    let body_index = node
        .children
        .iter()
        .position(|child| child.kind == NodeKind::Body);
    let children = body_index.map_or(node.children.as_slice(), |index| {
        node.children[index].children.as_slice()
    });
    flow.append_nodes(children);
    if let Some(body_index) = body_index {
        for child in node.children[body_index + 1..]
            .iter()
            .take_while(|child| child.line == node.line && child.flags.delimiter_close)
        {
            flow.append_inline(child, None);
        }
    }
    flow.flush();
    formatter.font = flow.line.font;
    formatter.spacing = flow.line.spacing_enabled();
    flow.output
}

/// No-fill controls line geometry, not which AST payloads are reachable.
/// Transparent font scopes retain the same cursor; tables/lists re-enter the
/// structural lowerer after flushing the current inline run.
struct DisplayFlow<'a, 'source> {
    output: Vec<Block>,
    line: InlineBuilder,
    source: Option<mant_ir::SourceSpan>,
    context: &'a LoweringContext<'source>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    formatter: crate::mandoc::formatter::FormatterState,
}

impl DisplayFlow<'_, '_> {
    fn append_container(&mut self, node: &Node) -> bool {
        let mut saved_font = None;
        let mut started = false;
        crate::mandoc::containers::walk(node, |event| {
            use crate::mandoc::containers::Event;
            if !started {
                for target in super::targets::structural_targets(node) {
                    self.line
                        .append(vec![Inline::anchor_at(target, source_span(node))]);
                }
                started = true;
            }
            match event {
                Event::Children(nodes) => self.append_nodes(nodes),
                Event::Glyph(value) => {
                    self.line.begin_source_line(node.line);
                    self.line.append_text(&value);
                }
                Event::Tight => self.line.tighten_next_boundary(),
                Event::Release => self.line.release_next_boundary(),
                Event::EmptyWord => self.line.append_word(Vec::new()),
                Event::EnterFont(font) => saved_font = Some(self.line.font.push_scope(font)),
                Event::ExitFont => {
                    if let Some(saved) = saved_font.take() {
                        self.line.font.pop_scope(saved);
                    }
                }
            }
        })
    }

    fn flush(&mut self) {
        let mut next = InlineBuilder::with_spacing(self.line.spacing_enabled());
        next.font = self.line.font;
        self.line.transfer_source_cursor(&mut next);
        let children = std::mem::replace(&mut self.line, next).finish();
        if !children.is_empty() {
            self.output.push(Block::Preformatted {
                children,
                language: None,
                layout: layout(self.indent_columns),
                source: self.source.take(),
            });
        }
    }

    fn append_inline(&mut self, node: &Node, next: Option<&Node>) {
        if self.source.is_none() {
            self.source = source_span(node);
        }
        match node.macro_name.as_deref() {
            Some("sp") => {
                self.line.begin_source_line(node.line);
                self.line
                    .blank_rows(vertical_distance_lines(node).unwrap_or(0));
            }
            Some("br") => {
                self.line.begin_source_line(node.line);
                self.line.hard_break();
            }
            _ => {
                append_inline_node_with_next(&mut self.line, node, next, self.context.default_name);
            }
        }
    }

    fn append_nodes(&mut self, nodes: &[Node]) {
        let plan = TableEmbeddingPlan::new(nodes, self.context);
        for (index, node) in nodes.iter().enumerate() {
            if plan.consumes(index) {
                continue;
            }
            if self.append_container(node) {
                continue;
            }
            if node.kind == NodeKind::Block
                && matches!(node.macro_name.as_deref(), Some("Bd" | "D1" | "Dl"))
            {
                for target in super::targets::structural_targets(node) {
                    self.line
                        .append(vec![Inline::anchor_at(target, source_span(node))]);
                }
                self.append_nodes(first_part_children(node, NodeKind::Body));
            } else if node.kind == NodeKind::Table
                || (node.kind == NodeKind::Block
                    && matches!(node.macro_name.as_deref(), Some("Bl" | "Rs")))
            {
                self.flush();
                self.formatter.font = self.line.font;
                self.formatter.spacing = self.line.spacing_enabled();
                if node.kind == NodeKind::Table {
                    append_table_row(
                        &mut self.output,
                        node,
                        self.context,
                        self.indent_columns,
                        plan.embedding(index),
                        &mut self.formatter,
                    );
                } else {
                    self.output.extend(super::lower_blocks_with_spacing(
                        std::slice::from_ref(node),
                        self.context,
                        self.indent_columns,
                        &mut 1,
                        self.line.spacing_enabled(),
                        &mut self.formatter,
                    ));
                }
                self.line.font = self.formatter.font;
                self.line.inherit_spacing(self.formatter.spacing);
                self.line.reset_source_cursor();
                self.source = None;
            } else if node.kind != NodeKind::Text && node.macro_name.is_none() {
                self.append_nodes(&node.children);
            } else {
                self.append_inline(node, nodes.get(index + 1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::Inline;

    #[test]
    fn font_styling_preserves_line_boundaries() {
        let mut builder = super::InlineBuilder::new();
        builder.with_font_scope(crate::mandoc::roff_escape::RoffFont::Strong, |builder| {
            builder.append_text("first");
            builder.hard_break();
            builder.append_text("second");
        });
        let styled = builder.finish();

        assert!(matches!(
            styled.as_slice(),
            [
                Inline::Strong { children: first },
                Inline::LineBreak,
                Inline::Strong { children: second },
            ] if crate::inline::plain_text(first) == "first" && crate::inline::plain_text(second) == "second"
        ));
    }
}
