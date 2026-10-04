//! Structural payload dispatch; state and output remain caller-owned.
use super::{
    Block, DisplayKind, LoweringContext, ManDefinitionState, ManListState, Node, NodeKind,
    ScopeFlow, TableEmbedding, add_leading_spacing, append_relative_continuation, append_table_row,
    first_part_children, layout, lower_inline_nodes, lower_man_definition_block, lower_mdoc_list,
    lower_scope, lower_synopsis_head, part_child_groups, plain_text, preformatted_blocks,
    set_block_spacing, source_span,
};
use libmandoc_rs::{
    MacroToken::{Man, Mdoc},
    ManMacro, MdocMacro,
};

pub(super) struct StructuralLowerer<'a, 'source, 'state> {
    pub(super) context: &'a LoweringContext<'source>,
    pub(super) indent_columns: crate::mandoc::layout::SourceIndent,
    pub(super) paragraph_distance: &'state mut u16,
    pub(super) output: &'state mut Vec<Block>,
    pub(super) paragraph_predecessor: bool,
    pub(super) man_source_predecessor: bool,
    pub(super) definition_hanging_width: &'state mut crate::mandoc::layout::Distance,
    pub(super) man_list_state: &'state mut ManListState,
    pub(super) ip_run: Option<super::lists::man::IpRun>,
    pub(super) spacing_enabled: bool,
    pub(super) formatter: &'state mut crate::mandoc::formatter::FormatterState,
}

impl StructuralLowerer<'_, '_, '_> {
    fn has_paragraph_predecessor(&self) -> bool {
        self.paragraph_predecessor || !self.output.is_empty()
    }

    fn lower_man_definition(&mut self, node: &Node) {
        // man_term.c::pre_IP/pre_TP use print_bvspace() at BLOCK entry;
        // TQ has zero leading distance. Neither uses emitted IR as evidence
        // of a preceding source sibling inside a first-child RS chain.
        let has_predecessor = self.man_source_predecessor;
        lower_man_definition_block(
            node,
            self.context,
            self.indent_columns,
            ManDefinitionState {
                paragraph_distance: self.paragraph_distance,
                output: self.output,
                definition_hanging_width: self.definition_hanging_width,
                list_state: self.man_list_state,
                ip_run: self.ip_run,
                has_predecessor,
            },
            self.spacing_enabled,
            self.formatter,
        );
    }

    pub(super) fn push(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        table_embedding: Option<&TableEmbedding>,
    ) {
        let scoped_body = if matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(MdocMacro::Bl | MdocMacro::Rs))
        ) || (node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))
            && node.display_kind == Some(DisplayKind::Filled))
        {
            node.children
                .iter()
                .find(|part| part.kind == NodeKind::Body)
        } else {
            None
        };
        if let Some(body) = scoped_body {
            self.context
                .scope_posts
                .enter_body(body.id, self.formatter.font.checkpoint());
        }
        self.push_scoped(node, next, table_embedding);
        if let Some(body) = scoped_body
            && let Some(saved) = self.context.scope_posts.exit_body(body.id)
        {
            self.formatter.font.pop_scope(saved);
        }
    }

    fn push_scoped(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        table_embedding: Option<&TableEmbedding>,
    ) {
        let continues_ip_item = node.macro_token.as_ref() == Some(&Man(ManMacro::Rs))
            && self.man_list_state.is_active();
        if !matches!(
            node.macro_token.as_ref(),
            Some(Man(ManMacro::Ip | ManMacro::Tp))
        ) && !continues_ip_item
        {
            self.man_list_state.reset();
        }
        if self.lower_transparent_container(node, next) {
            return;
        }
        match node.macro_token.as_ref() {
            Some(Man(ManMacro::Tp | ManMacro::Ip | ManMacro::Tq)) => {
                self.lower_man_definition(node);
            }
            Some(Mdoc(MdocMacro::Bl)) => {
                let mut block = lower_mdoc_list(
                    node,
                    self.context,
                    self.indent_columns,
                    self.paragraph_distance,
                    self.spacing_enabled,
                    self.has_paragraph_predecessor(),
                    self.formatter,
                );
                // Bl BLOCK pre/post call only term_newln(). The vertical
                // request belongs to It pre's print_bvspace(), so an empty
                // normalized list cannot acquire another paragraph gap.
                // Target attachment may create a zero-width IR carrier; it
                // does not create an It BLOCK in the native execution tree.
                if super::lists::has_native_mdoc_list_items(node)
                    && self.has_paragraph_predecessor()
                    && !node.compact
                {
                    set_block_spacing(&mut block, 1);
                }
                self.output.push(block);
            }
            Some(Mdoc(MdocMacro::Bd | MdocMacro::D1 | MdocMacro::Dl)) => {
                let has_predecessor = self.has_paragraph_predecessor();
                let mut nested = preformatted_blocks(
                    node,
                    self.context,
                    self.context.offset_indent(
                        node,
                        self.indent_columns,
                        self.context.display_offset(node),
                    ),
                    self.spacing_enabled,
                    has_predecessor,
                    self.formatter,
                );
                if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))
                    && has_predecessor
                    && !node.compact
                    // The shared column driver already executed native
                    // print_bvspace against the live field, including its
                    // completed-row receipt. IR spacing is not a second
                    // execution of that request.
                    && !self.formatter.execution.has_column_output_scope()
                {
                    if nested.is_empty() {
                        // Even an empty display executes its own vertical
                        // request. Do not lose it merely because no literal
                        // leaf exists to carry a layout hint.
                        nested.push(Block::VerticalSpace {
                            lines: 1,
                            source: source_span(node),
                        });
                    } else {
                        add_leading_spacing(&mut nested, 1);
                    }
                }
                self.output.extend(nested);
            }
            Some(Man(ManMacro::Sy) | Mdoc(MdocMacro::Nm)) => lower_synopsis_head(
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

    fn lower_man_hanging_paragraph(&mut self, node: &Node, next: Option<&Node>) {
        let output_start = self.output.len();
        let children = first_part_children(node, NodeKind::Body);
        if !children.is_empty()
            && let Some(argument) = crate::mandoc::layout::first_part_argument(node)
        {
            *self.definition_hanging_width =
                self.context
                    .distance_or(node, argument, *self.definition_hanging_width);
        }
        let spacing = crate::mandoc::layout::execute_man_paragraph_spacing(
            self.formatter,
            *self.paragraph_distance,
            self.man_source_predecessor,
        );
        // pre_HP(HEAD) consumes no children, but print_man_node() still
        // replaces the font on HEAD and BODY entry and exit.
        self.formatter.font.man_text_boundary(); // HEAD pre
        self.formatter.font.man_text_boundary(); // HEAD post
        self.formatter.font.man_text_boundary(); // BODY pre
        let mut lowerer = super::BlockLowerer::new(
            self.context,
            self.indent_columns,
            self.paragraph_distance,
            self.spacing_enabled,
            Vec::new(),
            std::mem::take(self.formatter),
        );
        if !children.is_empty() {
            lowerer.state.start_hanging(self.context.offset_indent(
                node,
                self.indent_columns,
                *self.definition_hanging_width,
            ));
        }
        let next_owner = next.filter(|next| {
            next.kind == NodeKind::Block
                && next.macro_token.as_ref() == Some(&Man(ManMacro::Ip))
                && first_part_children(next, NodeKind::Head).is_empty()
                && native_exit_epoch(node) == next.flow_epoch
        });
        let operand_capture = next_owner.map(|_| {
            std::rc::Rc::new(std::cell::RefCell::new(
                crate::mandoc::inline::HeadOperandCapture::default(),
            ))
        });
        let inbound_capture = std::mem::replace(
            &mut lowerer.state.formatter.head_operand_capture,
            operand_capture.clone(),
        );
        lowerer.push_nodes(children);
        // man_term.c::post_HP() closes its BODY row. PP/P/LP have no post
        // handler and execute in the caller's live BlockState instead.
        let nested = lowerer.finish_into(self.formatter, super::FormatterRowBoundary::Settle);
        self.formatter.head_operand_capture = inbound_capture;
        self.formatter.font.man_text_boundary(); // BODY post
        self.formatter.font.man_text_boundary(); // BLOCK post
        extend_blocks_with_spacing(self.output, nested, spacing, node);
        // man_term.c::pre_HP/post_HP and pre_IP render separate native
        // owners. The real driver's adjacent empty IP can complete a semantic
        // pair later; it must not alter either owner's formatter geometry.
        if let Some(next) = next_owner
            && self.output.len() == output_start + 1
        {
            self.context.native_heads.borrow_mut().hanging.head(
                &mut self.output[output_start],
                std::ptr::from_ref(node) as usize,
                std::ptr::from_ref(next) as usize,
                operand_capture
                    .as_ref()
                    .expect("matched IP capture")
                    .borrow_mut()
                    .take(),
            );
        }
    }

    fn lower_transparent_container(&mut self, node: &Node, next: Option<&Node>) -> bool {
        match node.macro_token.as_ref() {
            Some(Man(ManMacro::Hp)) => self.lower_man_hanging_paragraph(node, next),
            Some(Mdoc(MdocMacro::Bd)) if node.display_kind == Some(DisplayKind::Filled) => {
                let has_predecessor = self.has_paragraph_predecessor();
                let spacing_before = u16::from(has_predecessor && !node.compact);
                let nested = super::lower_scope(
                    first_part_children(node, NodeKind::Body),
                    self.context,
                    self.paragraph_distance,
                    self.formatter,
                    super::ScopeFlow::body_post_row_end(
                        self.context.offset_indent(
                            node,
                            self.indent_columns,
                            self.context.display_offset(node),
                        ),
                        self.spacing_enabled,
                        has_predecessor,
                    ),
                );
                extend_blocks_with_spacing(self.output, nested, spacing_before, node);
            }
            Some(Man(ManMacro::Rs)) => {
                // mandoc's print_bvspace climbs first-child RS wrappers to
                // find a predecessor. A detached item-continuation buffer
                // must not erase that source fact: RS itself adds no gap,
                // while its first PP still applies the current PD distance.
                let paragraph_predecessor = self.has_paragraph_predecessor();
                self.formatter.font.man_text_boundary(); // HEAD pre
                self.formatter.font.man_text_boundary(); // HEAD post
                self.formatter.font.man_text_boundary(); // BODY pre
                let continues_item = self.man_list_state.is_active();
                let output = if continues_item {
                    Vec::new()
                } else {
                    std::mem::take(self.output)
                };
                let mut lowerer = super::BlockLowerer::new(
                    self.context,
                    self.context.man_relative_indent(
                        node,
                        self.indent_columns,
                        *self.definition_hanging_width,
                    ),
                    self.paragraph_distance,
                    self.spacing_enabled,
                    output,
                    std::mem::take(self.formatter),
                );
                lowerer.paragraph_predecessor = paragraph_predecessor;
                lowerer.man_source_predecessor = self.man_source_predecessor;
                lowerer.push_nodes(first_part_children(node, NodeKind::Body));
                let mut nested =
                    lowerer.finish_into(self.formatter, super::FormatterRowBoundary::Settle);
                self.formatter.font.man_text_boundary(); // BODY post
                self.formatter.font.man_text_boundary(); // BLOCK post
                if continues_item {
                    if append_relative_continuation(
                        self.output,
                        &mut nested,
                        self.indent_columns,
                        *self.man_list_state,
                    ) {
                        return true;
                    }
                    self.man_list_state.reset();
                    self.output.append(&mut nested);
                    return true;
                }
                *self.output = nested;
            }
            _ => return false,
        }
        true
    }
}

/// Empty paragraph validation can erase an intervening PP/P/LP. The executed
/// boundary stamp survives that erasure; inspect only the last descendant,
/// not source line numbers or a speculative traversal of the IP body.
fn native_exit_epoch(mut node: &Node) -> usize {
    while let Some(last) = node.children.last() {
        node = last;
    }
    node.flow_epoch
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
        output.extend(lower_scope(
            node.children.as_slice(),
            context,
            paragraph_distance,
            formatter,
            ScopeFlow::filled(indent_columns, spacing_enabled),
        ));
    } else {
        for body in bodies {
            output.extend(lower_scope(
                body,
                context,
                paragraph_distance,
                formatter,
                ScopeFlow::filled(indent_columns, spacing_enabled),
            ));
        }
    }
}

pub(super) fn parts_have_visible_text(nodes: &[Node], default_name: Option<&str>) -> bool {
    !plain_text(&lower_inline_nodes(nodes, default_name))
        .trim()
        .is_empty()
}

/// Retain display equation content with decoded glyphs and source geometry.
fn equation_block(node: &Node, indent_columns: crate::mandoc::layout::SourceIndent) -> Block {
    let expression = node
        .equation
        .as_ref()
        .map(crate::mandoc::equations::expression_from_ast);
    let value = expression
        .as_ref()
        .map_or_else(String::new, mant_ir::EquationExpression::readable_text);
    Block::Equation {
        // Equation boxes carry the same named-character escapes as ordinary
        // roff text, but they bypass inline-node lowering. Decode them here so
        // values such as `\[*p]` and `\[mi]` cannot leak into every output
        // projection.
        value,
        expression,
        display: true,
        layout: layout(indent_columns),
        source: source_span(node),
    }
}

/// Preserve a retained macro's already executed leading distance, even when
/// its body only changes formatter state. CVS `pre_PP`/`print_bvspace` and groff's
/// paragraph macros execute spacing before the following RS sibling exists.
/// The caller resolves predecessor/compact/PD policy; an empty body cannot
/// cancel that source event or transfer its provenance to later content.
fn extend_blocks_with_spacing(
    output: &mut Vec<Block>,
    mut nested: Vec<Block>,
    lines: u16,
    node: &Node,
) {
    if nested.is_empty() && lines > 0 {
        nested.push(Block::VerticalSpace {
            lines,
            source: source_span(node),
        });
    } else {
        add_leading_spacing(&mut nested, lines);
    }
    output.extend(nested);
}
