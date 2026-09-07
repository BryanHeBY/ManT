//! Preserves no-fill and literal display content as preformatted blocks.

use libmandoc_rs::{Node, NodeKind, NormalizedFont};
use mant_ir::{Block, Inline};

use super::super::{
    LoweringContext, first_part_children,
    inline::{InlineBuilder, append_inline_node, lower_inline_nodes, plain_text},
    layout::{layout, vertical_distance_lines},
    source_span,
};
use super::{
    ends_with_line_continuation, participates_in_inline_flow,
    tables::{TableEmbeddingPlan, append_table_row},
};

pub(super) fn preformatted_blocks(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    mut spacing_enabled: bool,
) -> Vec<Block> {
    let body_index = node
        .children
        .iter()
        .position(|child| child.kind == NodeKind::Body);
    let children = body_index.map_or_else(
        || node.children.as_slice(),
        |index| node.children[index].children.as_slice(),
    );
    let table_plan = TableEmbeddingPlan::new(children, context);
    let mut output = Vec::new();
    let mut inline_run = Vec::new();
    for (index, child) in children.iter().enumerate() {
        if table_plan.consumes(index) {
            continue;
        }
        if child.kind == NodeKind::Table {
            push_preformatted_inline_run(
                &mut output,
                &mut inline_run,
                context,
                indent_columns,
                &mut spacing_enabled,
            );
            append_table_row(
                &mut output,
                child,
                context,
                indent_columns,
                table_plan.embedding(index),
            );
        } else {
            inline_run.push(child);
        }
    }
    let (mut inlines, _) = preformatted_inlines_refs(&inline_run, context, spacing_enabled);

    // mdoc validation can move a closing delimiter out of the display body
    // while leaving it as a direct child of the display block.  It still
    // belongs to the same rendered line (`.Dl return [ exitstatus ]`).
    if let Some(body_index) = body_index {
        let tail = &node.children[body_index + 1..];
        let tail_len = tail
            .iter()
            .take_while(|child| child.line == node.line && participates_in_inline_flow(child))
            .count();
        if tail
            .first()
            .is_some_and(|child| child.flags.delimiter_close)
        {
            inlines.extend(lower_inline_nodes(&tail[..tail_len], context.default_name));
        }
    }
    if !inlines.is_empty() {
        output.push(Block::Preformatted {
            children: inlines,
            language: None,
            layout: layout(indent_columns),
            source: source_span(node),
        });
    }
    output
}

fn push_preformatted_inline_run(
    output: &mut Vec<Block>,
    nodes: &mut Vec<&Node>,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    spacing_enabled: &mut bool,
) {
    if nodes.is_empty() {
        return;
    }
    let (children, final_spacing) = preformatted_inlines_refs(nodes, context, *spacing_enabled);
    *spacing_enabled = final_spacing;
    let source = nodes.first().and_then(|node| source_span(node));
    nodes.clear();
    if !children.is_empty() {
        output.push(Block::Preformatted {
            children,
            language: None,
            layout: layout(indent_columns),
            source,
        });
    }
}

/// Assemble a no-fill run using one physical-line cursor and inline state.
/// Styling and transparent AST containers do not themselves consume a line.
fn preformatted_inlines_refs(
    nodes: &[&Node],
    context: &LoweringContext<'_>,
    spacing_enabled: bool,
) -> (Vec<Inline>, bool) {
    let mut line = InlineBuilder::with_spacing(spacing_enabled);
    NoFillFlow::default().append(nodes.iter().copied(), &mut line, context);
    let final_spacing = line.spacing_enabled();
    (line.finish(), final_spacing)
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
        for node in nodes {
            self.push(node, line, context);
        }
    }

    fn push(&mut self, node: &Node, line: &mut InlineBuilder, context: &LoweringContext<'_>) {
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
            line.append_scope(
                |line| self.append(first_part_children(node, NodeKind::Body), line, context),
                |nodes| {
                    if let Some(font) = node.font {
                        style_preformatted_inlines(nodes, font)
                    } else {
                        nodes
                    }
                },
            );
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
                append_inline_node(line, node, context.default_name);
                return;
            }
            _ => {}
        }
        if let Some(previous) = self.previous_line.filter(|previous| node.line > *previous)
            && !self.continues_line
        {
            line.blank_rows(context.no_fill_blank_rows_between(Some(previous), Some(node.line)));
        }
        append_inline_node(line, node, context.default_name);
        self.previous_line = Some(node.line);
        self.continues_line = ends_with_line_continuation(node);
    }
}

pub(super) fn style_preformatted_inlines(nodes: Vec<Inline>, font: NormalizedFont) -> Vec<Inline> {
    let mut output = Vec::new();
    let mut line = Vec::new();
    for node in nodes {
        if node == Inline::LineBreak {
            append_styled_preformatted_line(&mut output, &mut line, font);
            output.push(Inline::LineBreak);
        } else {
            line.push(node);
        }
    }
    append_styled_preformatted_line(&mut output, &mut line, font);
    output
}

fn append_styled_preformatted_line(
    output: &mut Vec<Inline>,
    line: &mut Vec<Inline>,
    font: NormalizedFont,
) {
    let content = std::mem::take(line);
    if content.is_empty() {
        return;
    }
    output.push(match font {
        NormalizedFont::Emphasis => Inline::Emphasis { children: content },
        NormalizedFont::Literal => Inline::Code {
            value: plain_text(&content),
        },
        NormalizedFont::Symbolic => Inline::Strong { children: content },
    });
}

#[cfg(test)]
mod tests {
    use libmandoc_rs::NormalizedFont;
    use mant_ir::Inline;

    #[test]
    fn font_styling_preserves_line_boundaries() {
        let styled = super::style_preformatted_inlines(
            vec![
                Inline::Text {
                    value: "first".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "second".to_owned(),
                },
            ],
            NormalizedFont::Symbolic,
        );

        assert!(matches!(
            styled.as_slice(),
            [
                Inline::Strong { children: first },
                Inline::LineBreak,
                Inline::Strong { children: second },
            ] if super::plain_text(first) == "first" && super::plain_text(second) == "second"
        ));
    }
}
