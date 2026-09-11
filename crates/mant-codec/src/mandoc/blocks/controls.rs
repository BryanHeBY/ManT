//! State-only requests execute before printable fallback and never leak operands.
use super::{
    AuthorMode, Block, BlockState, LoweringContext, Node, NodeKind, is_section, lower_inline_nodes,
    plain_text, source_span, update_paragraph_distance, vertical_distance_lines,
};

/// A read-only request classification, not a replayable formatter effect.
#[derive(Clone, Copy)]
enum BlockControl {
    ParagraphDistance,
    LiteralBoundary,
    Author(Option<AuthorMode>),
    Spacing,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ControlOutcome {
    Consumed,
    // An author name may execute a split boundary and still own inline text.
    ContinueInline,
}

fn classify_control(node: &Node, dialect: libmandoc_rs::MacroSet) -> Option<BlockControl> {
    match node.macro_name.as_deref()? {
        "PD" => Some(BlockControl::ParagraphDistance),
        "nf" | "fi" => Some(BlockControl::LiteralBoundary),
        "EX" | "EE" if dialect == libmandoc_rs::MacroSet::Man => {
            Some(BlockControl::LiteralBoundary)
        }
        "An" => Some(BlockControl::Author(node.author_mode)),
        "Sm" => Some(BlockControl::Spacing),
        _ => None,
    }
}

impl super::BlockLowerer<'_, '_> {
    /// Consume execution requests before the no-fill word fallback can
    /// mistake numeric operands for printable text.
    pub(super) fn push_executed_spacing(&mut self, node: &Node) -> bool {
        let space = node.macro_name.as_deref() == Some("sp");
        if !(space || node.flags.no_fill && node.macro_name.as_deref() == Some("br")) {
            return false;
        }
        self.state.flush_preformatted();
        self.state.flush_paragraph();
        self.state.consume_hanging_first_line();
        if space && let Some(lines) = vertical_distance_lines(node) {
            self.state.output.push(Block::VerticalSpace {
                lines,
                source: source_span(node),
            });
        }
        true
    }

    pub(super) fn consume_control_or_empty_block(&mut self, node: &Node) -> bool {
        if node.macro_name.as_deref() == Some("in")
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
        let control_consumed =
            classify_control(node, self.context.macro_set).is_some_and(|request| {
                execute_block_control(
                    request,
                    node,
                    self.context,
                    &mut self.state,
                    self.paragraph_distance,
                    &mut self.split_authors,
                ) == ControlOutcome::Consumed
            });
        if control_consumed
            || node.flags.no_print
            || node.kind == NodeKind::Comment
            || is_section(node, false)
            || crate::mandoc::controls::operand_control(node.macro_name.as_deref()).is_some()
        {
            return true;
        }
        // A configuration-only `.EQ delim XX .EN` produces an empty equation
        // syntax node. It changes parser state but owns no printable block.
        if node.kind == NodeKind::Equation
            && node
                .equation
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return true;
        }
        if node.kind == NodeKind::Text
            && node.text.as_deref().is_some_and(str::is_empty)
            && !node.flags.no_fill
        {
            self.state.flush_paragraph();
            self.state.output.push(Block::VerticalSpace {
                lines: 1,
                source: source_span(node),
            });
            return true;
        }
        false
    }
}

/// Execute exactly one classified request before printable block lowering.
fn execute_block_control(
    request: BlockControl,
    node: &Node,
    context: &LoweringContext<'_>,
    state: &mut BlockState,
    paragraph_distance: &mut u16,
    split_authors: &mut bool,
) -> ControlOutcome {
    match request {
        BlockControl::ParagraphDistance => update_paragraph_distance(node, paragraph_distance),
        BlockControl::LiteralBoundary => {
            // roff_term_pre_br and man pre_literal end the current line and
            // switch HP from its temporary first-line origin to its permanent
            // body origin, including when no text preceded the request.
            state.literal_mode_boundary();
        }
        BlockControl::Author(mode) => match mode {
            Some(AuthorMode::Split) => *split_authors = true,
            Some(AuthorMode::NoSplit) => *split_authors = false,
            None if *split_authors => {
                state.hard_break();
                return ControlOutcome::ContinueInline;
            }
            None => return ControlOutcome::ContinueInline,
        },
        BlockControl::Spacing => {
            let setting = plain_text(&lower_inline_nodes(&node.children, context.default_name));
            state.set_spacing(setting.trim());
        }
    }
    ControlOutcome::Consumed
}
