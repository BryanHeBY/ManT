//! State-only requests execute before printable fallback and never leak operands.
use super::{
    Block, BlockState, LoweringContext, Node, NodeKind, is_section, lower_inline_nodes,
    lower_inline_nodes_with_font_state, plain_text, source_span, update_paragraph_distance,
    vertical_space_delta,
};
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, RoffMacro,
};

/// A read-only request classification, not a replayable formatter effect.
#[derive(Clone, Copy)]
enum BlockControl {
    ParagraphDistance,
    LiteralBoundary,
    PreBreak,
    Spacing,
}

fn classify_control(node: &Node, dialect: libmandoc_rs::MacroSet) -> Option<BlockControl> {
    match node.macro_token.as_ref()? {
        Man(ManMacro::Pd) => Some(BlockControl::ParagraphDistance),
        Roff(RoffMacro::Nf | RoffMacro::Fi) => Some(BlockControl::LiteralBoundary),
        // ti executes pre_br but does not select another output container.
        Roff(RoffMacro::Ti) => Some(BlockControl::PreBreak),
        Man(ManMacro::Ex | ManMacro::Ee) if dialect == libmandoc_rs::MacroSet::Man => {
            Some(BlockControl::LiteralBoundary)
        }
        Mdoc(MdocMacro::Sm) => Some(BlockControl::Spacing),
        _ => None,
    }
}

impl super::BlockLowerer<'_, '_> {
    /// Consume execution requests before the no-fill word fallback can
    /// mistake numeric operands for printable text.
    pub(super) fn push_executed_spacing(&mut self, node: &Node) -> bool {
        let space = node.macro_token.as_ref() == Some(&Roff(RoffMacro::Sp));
        if !(space || node.flags.no_fill && node.macro_token.as_ref() == Some(&Roff(RoffMacro::Br)))
        {
            return false;
        }
        self.state.flush_preformatted();
        let lines = self
            .state
            .resolve_vertical_space(vertical_space_delta(node));
        self.state.flush_paragraph_for_line_request();
        self.state.consume_hanging_first_line();
        if space {
            self.state.output.push(Block::VerticalSpace {
                lines,
                source: source_span(node),
            });
        }
        true
    }

    pub(super) fn consume_control_or_empty_block(&mut self, node: &Node) -> bool {
        if node.macro_token.as_ref() == Some(&Man(ManMacro::In))
            && self.context.macro_set == libmandoc_rs::MacroSet::Man
        {
            self.state.flush_preformatted();
            self.state.flush_paragraph();
            let current = self.state.source_indent();
            let next = node
                .children
                .first()
                .and_then(|node| node.text.as_deref())
                .map_or(self.indent_columns.macro_origin(), |argument| {
                    let Some(distance) = self.context.checked_distance(node, argument) else {
                        return current;
                    };
                    if argument.starts_with(['+', '-']) {
                        self.context.offset_indent(node, current, distance)
                    } else {
                        current.absolute(distance)
                    }
                });
            self.state.set_source_indent(next);
            return true;
        }
        // Classification itself must not consume a font, boundary or request.
        // Execute once, before the later no-print/operand filters, as in source.
        let control_consumed = classify_control(node, self.context.macro_set)
            .map(|request| {
                execute_block_control(
                    request,
                    node,
                    self.context,
                    &mut self.state,
                    self.paragraph_distance,
                );
            })
            .is_some();
        if control_consumed
            || node.flags.no_print
            || node.kind == NodeKind::Comment
            || is_section(node, false)
            || crate::mandoc::controls::operand_control(node.macro_token.as_ref()).is_some()
        {
            return true;
        }
        // A configuration-only `.EQ delim XX .EN` produces an empty equation
        // syntax node. It changes parser state but owns no printable block.
        if node.kind == NodeKind::Equation
            && node
                .equation
                .as_ref()
                .is_none_or(|value| value.readable_text().trim().is_empty())
        {
            return true;
        }
        if node.kind == NodeKind::Text
            && node.text.as_deref().is_some_and(str::is_empty)
            && !node.flags.no_fill
        {
            // The filled source-line path also visits an empty TEXT through
            // term_vspace(), so negative .sp debt is consumed here instead
            // of turning the blank into an unconditional IR gap.
            let lines = self.state.resolve_vertical_space(1);
            self.state.flush_paragraph_for_line_request();
            if lines > 0 {
                self.state.output.push(Block::VerticalSpace {
                    lines,
                    source: source_span(node),
                });
            }
            return true;
        }
        false
    }

    /// `.ft` selects fonts without printable block output of its own; it is
    /// the roff-request analog of upstream `roff_html.c::roff_html_pre_ft`.
    pub(super) fn consume_font_request(&mut self, node: &Node) -> bool {
        if node.macro_token.as_ref() != Some(&Roff(RoffMacro::Ft)) {
            return false;
        }
        lower_inline_nodes_with_font_state(
            std::slice::from_ref(node),
            self.context.default_name,
            self.state.spacing_enabled(),
            &mut self.state.formatter.font,
        );
        true
    }
}

/// Execute exactly one classified request before printable block lowering.
fn execute_block_control(
    request: BlockControl,
    node: &Node,
    context: &LoweringContext<'_>,
    state: &mut BlockState,
    paragraph_distance: &mut u16,
) {
    match request {
        BlockControl::ParagraphDistance => update_paragraph_distance(node, paragraph_distance),
        BlockControl::LiteralBoundary => {
            // roff_term_pre_br and man pre_literal end the current line and
            // switch HP from its temporary first-line origin to its permanent
            // body origin, including when no text preceded the request.
            state.literal_mode_boundary();
        }
        BlockControl::PreBreak => state.pre_break_request(),
        BlockControl::Spacing => {
            let setting = plain_text(&lower_inline_nodes(&node.children, context.default_name));
            state.set_spacing(setting.trim());
        }
    }
}
