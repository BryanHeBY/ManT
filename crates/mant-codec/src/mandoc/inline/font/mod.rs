//! Stateful roff font decoding shared by prose, macro operands and table cells.
use super::flow::TrailingOutput;
use super::{
    Font, FontState, Inline, InlineBuilder, Node, RoffInlineEvent, append_inline_nodes, decode,
    is_formatter_word_blank,
};

mod no_fill;
mod style;
mod text_execution;
mod zero_advance;

pub(in crate::mandoc) use no_fill::{NoFillInlineState, lower_no_fill_line_with_font_state};
pub(super) use style::coalesce_font_runs;
pub(in crate::mandoc) use text_execution::{
    FormatterWordPart, parse_formatter_word_parts_with_zero_advance, parse_roff_text_with_state,
    parse_roff_text_with_zero_advance,
};
pub(in crate::mandoc) use zero_advance::ZeroAdvanceState;

pub(in crate::mandoc) fn parse_roff_text(source: &str) -> Vec<Inline> {
    parse_roff_text_with_font(source, Font::Regular, true)
}

/// Decode one roff text run using the font selected by its enclosing macro.
/// Explicit `\\f` escapes change `font` while the run is scanned, so a reset
/// to regular text remains visible even inside an alternating `.BI` argument.
pub(super) fn parse_roff_text_with_font(
    source: &str,
    initial_font: Font,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut state = FontState::new();
    state.select(initial_font);
    parse_roff_text_with_state(source, &mut state, recognize_generated_references)
}

pub(super) fn lower_man_font_scope(
    output: &mut InlineBuilder,
    node: &Node,
    default_name: Option<&str>,
) {
    output.font.select(Font::Regular);
    if let Some((first, second)) = super::alternating_font_pair(node.macro_name.as_deref()) {
        for (index, child) in node.children.iter().enumerate() {
            output
                .font
                .select(if index % 2 == 0 { first } else { second });
            // Alternating man macro operands are emitted as one formatter
            // word.  This is also the scope in which CVS preserves the
            // TERMP_BACK* state from one argument to the next.
            if index > 0 {
                output.tighten_next_boundary();
            }
            // Pinned CVS man_term.c::pre_alternate() visits each child as an
            // operand; term.c::term_word() may switch font *within* one child.
            // Preserve the native boundary before flattening style runs.
            output.mark_native_operand(child, false);
            super::append_inline_node_with_next(
                output,
                child,
                node.children.get(index + 1),
                default_name,
            );
            output.mark_native_operand(child, true);
        }
        output.font.select(Font::Regular);
        return;
    }
    if node.macro_name.as_deref() == Some("OP") {
        // CVS man_term.c::pre_OP() emits both brackets with term_word().
        // Keep them in the caller's formatter stream so pending `\z`/`\p`
        // state crosses authored operands and generated punctuation in order.
        output.append_text("[");
        output.tighten_next_boundary();
        output.enter_keep_words();
        for (index, child) in node.children.iter().enumerate() {
            output.font.select(if index == 0 {
                Font::Strong
            } else {
                Font::Emphasis
            });
            super::append_inline_node_with_next(
                output,
                child,
                node.children.get(index + 1),
                default_name,
            );
        }
        // OP resets for its closing bracket, then the man macro scope resets
        // again. Consequently a following fP selects regular, not its operand.
        output.font.select(Font::Regular);
        output.exit_keep_words();
        output.tighten_next_boundary();
        output.append_text("]");
        output.font.select(Font::Regular);
        return;
    }
    match node.macro_name.as_deref() {
        Some("B" | "SB") => output.font.select(Font::Strong),
        Some("I") => output.font.select(Font::Emphasis),
        _ => {}
    }
    // pre_B() selects the initial font, then ordinary child words execute
    // inside this one macro instance. Retain its complete native HEAD span
    // so a numeric short option can be admitted only at the actual bold
    // declaration start; an inline \fB later in its argument is not a new
    // operand.
    let bold_head = matches!(node.macro_name.as_deref(), Some("B" | "SB"));
    if bold_head {
        output.mark_native_operand(node, false);
    }
    super::append_inline_nodes(output, &node.children, default_name);
    if bold_head {
        output.mark_native_operand(node, true);
    }
    output.font.select(Font::Regular);
}

fn builder_with_zero_advance(
    spacing: bool,
    state: FontState,
    zero_advance: &mut ZeroAdvanceState,
) -> InlineBuilder {
    let mut builder = InlineBuilder::with_spacing(spacing);
    builder.font = state;
    builder.zero_advance = std::mem::take(zero_advance);
    builder
}

fn finish_with_zero_advance(
    builder: InlineBuilder,
    _zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, super::flow::PreservedInlineState) {
    builder.finish_preserving_execution()
}

pub(in crate::mandoc) fn lower_inline_nodes_with_font_state(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
) -> Vec<Inline> {
    let mut zero_advance = ZeroAdvanceState::new();
    let (mut output, execution) = lower_inline_nodes_with_font_state_and_zero_advance(
        nodes,
        default_name,
        spacing,
        state,
        &mut zero_advance,
    );
    if execution.word_end_break {
        output.push(Inline::LineBreak);
    }
    zero_advance = execution.zero_advance;
    zero_advance.finish_into(&mut output);
    output
}

fn lower_inline_nodes_with_font_state_and_zero_advance(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
    zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, super::flow::PreservedInlineState) {
    let mut builder = builder_with_zero_advance(spacing, *state, zero_advance);
    append_inline_nodes(&mut builder, nodes, default_name);
    *state = builder.font;
    finish_with_zero_advance(builder, zero_advance)
}
