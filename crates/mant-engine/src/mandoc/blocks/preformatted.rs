//! Preserves no-fill and literal display content as preformatted blocks.

use libmandoc_rs::{Node, NodeKind, NormalizedFont};
use mant_ir::{Block, Inline};

use super::super::{
    LoweringContext, first_part_children,
    inline::{InlineBuilder, append_inline_node, lower_inline_nodes, plain_text},
    layout::layout,
    source_span,
};
use super::{
    participates_in_inline_flow,
    tables::{append_table_row, table_embeddings},
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
    let (table_embeddings, embedded_nodes) = table_embeddings(children, context);
    let mut output = Vec::new();
    let mut inline_run = Vec::new();
    for (index, child) in children.iter().enumerate() {
        if embedded_nodes[index] {
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
                table_embeddings[index].as_ref(),
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

fn preformatted_inlines(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    spacing_enabled: bool,
) -> (Vec<Inline>, bool) {
    let nodes = nodes.iter().collect::<Vec<_>>();
    preformatted_inlines_refs(&nodes, context, spacing_enabled)
}

/// Assemble a no-fill run into visible rows.
///
/// libmandoc represents physical blank input lines as empty text nodes.  The
/// terminal formatter collapses consecutive raw blank lines to one separator,
/// while an explicit `.sp` request can ask for more.  Work from the adjacent
/// visible source lines so the AST's empty placeholders do not create a
/// growing stack of `LineBreak`s.
fn preformatted_inlines_refs(
    nodes: &[&Node],
    context: &LoweringContext<'_>,
    spacing_enabled: bool,
) -> (Vec<Inline>, bool) {
    let mut output = Vec::new();
    let mut line = InlineBuilder::with_spacing(spacing_enabled);
    let mut previous_visible_line = None;
    for node in nodes {
        if node.kind == NodeKind::Comment || node.flags.no_print {
            continue;
        }

        // Do not give every empty AST placeholder its own rendered row.  Its
        // source-line distance is accounted for when the next printable node
        // is appended below.
        if node.kind == NodeKind::Text && node.text.as_deref().is_some_and(str::is_empty) {
            continue;
        }

        if previous_visible_line.is_some_and(|previous| node.line > previous) {
            let spacing_enabled = line.spacing_enabled();
            output.extend(
                std::mem::replace(&mut line, InlineBuilder::with_spacing(spacing_enabled)).finish(),
            );
        }
        if let Some(previous) = previous_visible_line.filter(|previous| node.line > *previous)
            && !output.is_empty()
        {
            output.push(Inline::LineBreak);
            let extra_rows = context.no_fill_blank_rows_between(Some(previous), Some(node.line));
            output.extend(std::iter::repeat_n(
                Inline::LineBreak,
                usize::from(extra_rows),
            ));
        }
        if node.macro_name.as_deref() == Some("Bf") {
            let body = first_part_children(node, NodeKind::Body)
                .iter()
                .collect::<Vec<_>>();
            let (nested, final_spacing) =
                preformatted_inlines_refs(&body, context, line.spacing_enabled());
            line.append(if let Some(font) = node.font {
                style_preformatted_inlines(nested, font)
            } else {
                nested
            });
            line.inherit_spacing(final_spacing);
        } else if node.kind == NodeKind::Block
            && matches!(node.macro_name.as_deref(), Some("Bd" | "D1" | "Dl"))
        {
            // Malformed but deployed mdoc sometimes opens another literal
            // display before closing the current one.  libmandoc retains the
            // nested container; treating it as an inline macro collapses all
            // of its physical rows.  A preformatted parent can safely make
            // the nested display transparent while preserving its row
            // boundaries.
            let body = first_part_children(node, NodeKind::Body)
                .iter()
                .collect::<Vec<_>>();
            let (nested, final_spacing) =
                preformatted_inlines_refs(&body, context, line.spacing_enabled());
            line.append(nested);
            line.inherit_spacing(final_spacing);
        } else if node.kind == NodeKind::Text || node.macro_name.is_some() {
            append_inline_node(&mut line, node, context.default_name);
        } else {
            let (nested, final_spacing) =
                preformatted_inlines(&node.children, context, line.spacing_enabled());
            line.append(nested);
            line.inherit_spacing(final_spacing);
        }
        previous_visible_line = Some(node.line);
    }
    let final_spacing = line.spacing_enabled();
    output.extend(line.finish());
    (output, final_spacing)
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
