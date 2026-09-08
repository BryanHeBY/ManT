//! Structural payload dispatch; state and output remain caller-owned.
use super::{
    Block, DEFAULT_MAN_TAG_WIDTH, DisplayKind, Inline, LoweringContext, ManAliasState,
    ManDefinitionState, ManListState, Node, NodeKind, TableEmbedding, add_leading_spacing,
    append_relative_continuation, append_table_row, equation_block, extend_blocks_with_spacing,
    first_part_children, layout_with_spacing, lower_blocks_onto, lower_blocks_with_spacing,
    lower_inline_nodes, lower_man_definition_block, lower_mdoc_list, lower_synopsis_head,
    part_child_groups, plain_text, preformatted_blocks, set_block_spacing, source_span,
};

pub(super) struct StructuralLowerer<'a, 'source, 'state> {
    pub(super) context: &'a LoweringContext<'source>,
    pub(super) indent_columns: u16,
    pub(super) paragraph_distance: &'state mut u16,
    pub(super) output: &'state mut Vec<Block>,
    pub(super) definition_hanging_width: &'state mut usize,
    pub(super) man_alias_state: &'state mut ManAliasState,
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
                alias_state: self.man_alias_state,
                list_state: self.man_list_state,
            },
            self.spacing_enabled,
            self.formatter,
        );
    }

    pub(super) fn push(&mut self, node: &Node, table_embedding: Option<&TableEmbedding<'_>>) {
        if !matches!(node.macro_name.as_deref(), Some("TP" | "IP" | "TQ")) {
            *self.man_alias_state = ManAliasState::None;
        }
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
                    self.context.nested_indent(
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

    fn lower_transparent_container(&mut self, node: &Node) -> bool {
        match node.macro_name.as_deref() {
            Some("PP" | "P" | "LP" | "HP") => {
                // man(7) ordinary paragraphs restore the prevailing tag
                // width; HP is a hanging paragraph, not that reset boundary.
                if matches!(node.macro_name.as_deref(), Some("PP" | "P" | "LP")) {
                    *self.definition_hanging_width = DEFAULT_MAN_TAG_WIDTH;
                }
                let spacing_before = if self.output.is_empty() {
                    0
                } else {
                    *self.paragraph_distance
                };
                let nested = lower_blocks_with_spacing(
                    first_part_children(node, NodeKind::Body),
                    self.context,
                    self.indent_columns,
                    self.paragraph_distance,
                    self.spacing_enabled,
                    self.formatter,
                );
                extend_blocks_with_spacing(self.output, nested, spacing_before);
            }
            Some("Bd") if node.display_kind == Some(DisplayKind::Filled) => {
                let spacing_before = u16::from(!self.output.is_empty() && !node.compact);
                let nested = lower_blocks_with_spacing(
                    first_part_children(node, NodeKind::Body),
                    self.context,
                    self.context.nested_indent(
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
                        self.context.nested_indent(node, self.indent_columns, 4),
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
                    self.context.nested_indent(node, self.indent_columns, 4),
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
    indent_columns: u16,
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
