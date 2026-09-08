//! Preserves no-fill and literal display content as preformatted blocks.

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Block, Inline};

use super::super::{
    LoweringContext, first_part_children,
    inline::{InlineBuilder, append_inline_node_with_next},
    layout::{layout, vertical_distance_lines},
    source_span,
};
use super::{
    ends_with_line_continuation,
    tables::{TableEmbeddingPlan, append_table_row},
};

pub(super) fn preformatted_blocks(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    let mut flow = DisplayFlow {
        output: Vec::new(),
        line: InlineBuilder::with_spacing(spacing_enabled),
        cursor: NoFillFlow::default(),
        source: None,
        context,
        indent_columns,
        formatter: *formatter,
    };
    flow.line.font = formatter.font;
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
    cursor: NoFillFlow,
    source: Option<mant_ir::SourceSpan>,
    context: &'a LoweringContext<'source>,
    indent_columns: u16,
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
                Event::Glyph(value) => self.line.append_text(&value),
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
        self.cursor.push(node, next, &mut self.line, self.context);
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
                self.cursor = NoFillFlow::default();
                self.source = None;
            } else if node.kind != NodeKind::Text && node.macro_name.is_none() {
                self.append_nodes(&node.children);
            } else {
                self.append_inline(node, nodes.get(index + 1));
            }
        }
    }
}

/// Source-line policy is independent of the inline tree used for styling.
/// Only printable leaves or materialized vertical requests advance this cursor.
#[derive(Default)]
struct NoFillFlow {
    previous_line: Option<u32>,
    continues_line: bool,
}

impl NoFillFlow {
    fn append<'a>(
        &mut self,
        nodes: impl IntoIterator<Item = &'a Node>,
        line: &mut InlineBuilder,
        context: &LoweringContext<'_>,
    ) {
        let mut nodes = nodes.into_iter().peekable();
        while let Some(node) = nodes.next() {
            self.push(node, nodes.peek().copied(), line, context);
        }
    }

    fn push(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        line: &mut InlineBuilder,
        context: &LoweringContext<'_>,
    ) {
        if node.macro_name.as_deref() == Some("Tg") {
            append_inline_node_with_next(line, node, next, context.default_name);
            return;
        }
        if node.kind == NodeKind::Comment || node.flags.no_print {
            return;
        }
        if node.kind == NodeKind::Text && node.text.as_deref().is_some_and(str::is_empty) {
            return;
        }
        // Containers carry scope, not an extra source-visible row. Recurse
        // through the same cursor/builder so controls and continuation at the
        // end of a nested body remain active for the next outside leaf.
        if node.macro_name.as_deref() == Some("Bf") {
            let saved = node.font.map(|font| line.font.push_scope(font.into()));
            self.append(first_part_children(node, NodeKind::Body), line, context);
            if let Some(saved) = saved {
                line.font.pop_scope(saved);
            }
            return;
        }
        if node.kind == NodeKind::Block
            && matches!(node.macro_name.as_deref(), Some("Bd" | "D1" | "Dl"))
        {
            self.append(first_part_children(node, NodeKind::Body), line, context);
            return;
        }
        if node.kind != NodeKind::Text && node.macro_name.is_none() {
            self.append(&node.children, line, context);
            return;
        }
        match node.macro_name.as_deref() {
            Some("sp") => {
                line.blank_rows(vertical_distance_lines(node).unwrap_or(0));
                self.previous_line = Some(node.line);
                self.continues_line = false;
                return;
            }
            Some("br") => {
                line.hard_break();
                self.continues_line = false;
                return;
            }
            Some("Sm" | "ft" | "Ns") => {
                append_inline_node_with_next(line, node, next, context.default_name);
                return;
            }
            _ => {}
        }
        if let Some(previous) = self.previous_line.filter(|previous| node.line > *previous)
            && !self.continues_line
        {
            line.blank_rows(context.no_fill_blank_rows_between(Some(previous), Some(node.line)));
        }
        append_inline_node_with_next(line, node, next, context.default_name);
        self.previous_line = Some(node.line);
        self.continues_line = ends_with_line_continuation(node);
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
