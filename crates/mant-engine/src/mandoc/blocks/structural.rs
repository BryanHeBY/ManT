//! Structural payload dispatch; state and output remain caller-owned.
use super::{
    Block, DEFAULT_MAN_TAG_WIDTH, DisplayKind, Inline, LoweringContext, ManDefinitionState,
    ManListState, Node, NodeKind, TableEmbedding, add_leading_spacing,
    append_relative_continuation, append_table_row, equation_block, extend_blocks_with_spacing,
    first_part_children, layout_with_spacing, lower_blocks_onto, lower_blocks_with_spacing,
    lower_inline_nodes, lower_man_definition_block, lower_mdoc_list, lower_synopsis_head,
    part_child_groups, plain_text, preformatted_blocks, set_block_spacing, source_span,
};

pub(super) struct StructuralLowerer<'a, 'source, 'state> {
    pub(super) context: &'a LoweringContext<'source>,
    pub(super) indent_columns: crate::mandoc::layout::SourceIndent,
    pub(super) paragraph_distance: &'state mut u16,
    pub(super) output: &'state mut Vec<Block>,
    pub(super) definition_hanging_width: &'state mut crate::mandoc::layout::Distance,
    pub(super) man_list_state: &'state mut ManListState,
    pub(super) spacing_enabled: bool,
    pub(super) formatter: &'state mut crate::mandoc::formatter::FormatterState,
}

impl StructuralLowerer<'_, '_, '_> {
    fn lower_man_definition(&mut self, node: &Node) {
        lower_man_definition_block(
            node,
            self.context,
            self.indent_columns,
            ManDefinitionState {
                paragraph_distance: self.paragraph_distance,
                output: self.output,
                definition_hanging_width: self.definition_hanging_width,
                list_state: self.man_list_state,
            },
            self.spacing_enabled,
            self.formatter,
        );
    }

    pub(super) fn push(&mut self, node: &Node, table_embedding: Option<&TableEmbedding<'_>>) {
        let continues_ip_item =
            node.macro_name.as_deref() == Some("RS") && self.man_list_state.is_active();
        if !matches!(node.macro_name.as_deref(), Some("IP" | "TP")) && !continues_ip_item {
            *self.man_list_state = ManListState::None;
        }
        if self.lower_transparent_container(node) {
            return;
        }
        match node.macro_name.as_deref() {
            Some("TP" | "IP" | "TQ") => self.lower_man_definition(node),
            Some("Bl") => {
                let mut block = lower_mdoc_list(
                    node,
                    self.context,
                    self.indent_columns,
                    self.paragraph_distance,
                    self.spacing_enabled,
                    self.formatter,
                );
                if !self.output.is_empty() && !node.compact {
                    set_block_spacing(&mut block, 1);
                }
                self.output.push(block);
            }
            Some("Bd" | "D1" | "Dl") => {
                let mut nested = preformatted_blocks(
                    node,
                    self.context,
                    self.context.offset_indent(
                        node,
                        self.indent_columns,
                        self.context.display_offset(node),
                    ),
                    self.spacing_enabled,
                    self.formatter,
                );
                if node.macro_name.as_deref() == Some("Bd")
                    && !self.output.is_empty()
                    && !node.compact
                {
                    add_leading_spacing(&mut nested, 1);
                }
                self.output.extend(nested);
            }
            Some("Rs") => {
                let mut children = self.context.lower_inline_with_spacing(
                    first_part_children(node, NodeKind::Body),
                    self.spacing_enabled,
                    self.formatter,
                );
                if !children.is_empty() {
                    append_bibliography_period(&mut children);
                    self.output.push(Block::Paragraph {
                        children,
                        layout: layout_with_spacing(
                            self.indent_columns,
                            u16::from(!self.output.is_empty()),
                        ),
                        source: source_span(node),
                    });
                }
            }
            Some("SY" | "Nm") => lower_synopsis_head(
                self.output,
                node,
                self.context,
                self.indent_columns,
                self.paragraph_distance,
                self.spacing_enabled,
                self.formatter,
            ),
            _ if node.kind == NodeKind::Table => append_table_row(
                self.output,
                node,
                self.context,
                self.indent_columns,
                table_embedding,
                self.formatter,
            ),
            _ if node.kind == NodeKind::Equation => {
                self.output.push(equation_block(node, self.indent_columns));
            }
            _ => lower_structural_fallback(
                self.output,
                node,
                self.context,
                self.indent_columns,
                self.paragraph_distance,
                self.spacing_enabled,
                self.formatter,
            ),
        }
    }

    fn lower_man_paragraph(&mut self, node: &Node) {
        let children = first_part_children(node, NodeKind::Body);
        let hanging = node.macro_name.as_deref() == Some("HP");
        if !hanging {
            *self.definition_hanging_width =
                crate::mandoc::layout::Distance::cells(DEFAULT_MAN_TAG_WIDTH);
        } else if !children.is_empty()
            && let Some(argument) = crate::mandoc::layout::first_part_argument(node)
        {
            *self.definition_hanging_width =
                self.context
                    .distance_or(node, argument, *self.definition_hanging_width);
        }
        let spacing = if self.output.is_empty() {
            0
        } else {
            *self.paragraph_distance
        };
        let mut lowerer = super::BlockLowerer::new(
            self.context,
            self.indent_columns,
            self.paragraph_distance,
            self.spacing_enabled,
            Vec::new(),
            *self.formatter,
        );
        if hanging && !children.is_empty() {
            lowerer.state.start_hanging(self.context.offset_indent(
                node,
                self.indent_columns,
                *self.definition_hanging_width,
            ));
        }
        lowerer.push_nodes(children);
        lowerer.formatter.spacing = lowerer.state.spacing_enabled();
        *self.formatter = lowerer.formatter;
        extend_blocks_with_spacing(self.output, lowerer.finish(), spacing);
    }

    fn lower_transparent_container(&mut self, node: &Node) -> bool {
        match node.macro_name.as_deref() {
            Some("PP" | "P" | "LP" | "HP") => self.lower_man_paragraph(node),
            Some("Bd") if node.display_kind == Some(DisplayKind::Filled) => {
                let spacing_before = u16::from(!self.output.is_empty() && !node.compact);
                let nested = lower_blocks_with_spacing(
                    first_part_children(node, NodeKind::Body),
                    self.context,
                    self.context.offset_indent(
                        node,
                        self.indent_columns,
                        self.context.display_offset(node),
                    ),
                    self.paragraph_distance,
                    self.spacing_enabled,
                    self.formatter,
                );
                extend_blocks_with_spacing(self.output, nested, spacing_before);
            }
            Some("RS") => {
                if self.man_list_state.is_active() {
                    let mut nested = lower_blocks_with_spacing(
                        first_part_children(node, NodeKind::Body),
                        self.context,
                        self.context.man_relative_indent(
                            node,
                            self.indent_columns,
                            *self.definition_hanging_width,
                        ),
                        self.paragraph_distance,
                        self.spacing_enabled,
                        self.formatter,
                    );
                    if append_relative_continuation(
                        self.output,
                        &mut nested,
                        self.indent_columns,
                        *self.man_list_state,
                    ) {
                        return true;
                    }
                    *self.man_list_state = ManListState::None;
                    self.output.append(&mut nested);
                    return true;
                }
                let output = std::mem::take(self.output);
                *self.output = lower_blocks_onto(
                    first_part_children(node, NodeKind::Body),
                    self.context,
                    self.context.man_relative_indent(
                        node,
                        self.indent_columns,
                        *self.definition_hanging_width,
                    ),
                    self.paragraph_distance,
                    self.spacing_enabled,
                    output,
                    self.formatter,
                );
            }
            _ => return false,
        }
        true
    }
}

/// Restore the terminal punctuation generated by the mdoc `Rs` formatter.
///
/// Bibliography fields do not carry their final full stop in the syntax tree;
/// mandoc adds it while presenting the complete reference. Retaining that
/// formatter-owned character keeps phrases and citations faithful without
/// attaching punctuation to the individual `%A`/`%T` semantic nodes.
fn append_bibliography_period(children: &mut Vec<Inline>) {
    let text = plain_text(children);
    if !text.trim_end().ends_with(['.', '!', '?']) {
        children.push(Inline::Text { value: ".".into() });
    }
}

fn lower_structural_fallback(
    output: &mut Vec<Block>,
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    let heads = part_child_groups(node, NodeKind::Head).collect::<Vec<_>>();
    let bodies = part_child_groups(node, NodeKind::Body).collect::<Vec<_>>();
    let tails = part_child_groups(node, NodeKind::Tail).collect::<Vec<_>>();
    if heads
        .iter()
        .chain(&tails)
        .any(|part| parts_have_visible_text(part, context.default_name))
        || bodies.len() > 1
    {
        context.warn_unhandled_structural_parts(node);
    }
    if bodies.is_empty() {
        output.extend(lower_blocks_with_spacing(
            node.children.as_slice(),
            context,
            indent_columns,
            paragraph_distance,
            spacing_enabled,
            formatter,
        ));
    } else {
        for body in bodies {
            output.extend(lower_blocks_with_spacing(
                body,
                context,
                indent_columns,
                paragraph_distance,
                spacing_enabled,
                formatter,
            ));
        }
    }
}

pub(super) fn parts_have_visible_text(nodes: &[Node], default_name: Option<&str>) -> bool {
    !plain_text(&lower_inline_nodes(nodes, default_name))
        .trim()
        .is_empty()
}
