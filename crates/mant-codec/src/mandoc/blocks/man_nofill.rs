//! Man no-fill words retain executed empty rows, not formatter operands.
use super::{
    FontState, Inline, Node, NodeKind, ends_with_line_continuation, first_part_children,
    lower_inline_nodes_with_font_state, lower_no_fill_line_with_font_state,
    participates_in_inline_flow, source_span, targets,
};

struct LoweredNoFillLine {
    nodes: Vec<Inline>,
    source: Option<mant_ir::SourceSpan>,
    continues_line: bool,
    starts_line: bool,
    occupies_row: bool,
}

pub(super) fn is_no_fill_payload(node: &Node) -> bool {
    node.flags.no_fill
        && participates_in_inline_flow(node)
        && !matches!(
            node.macro_name.as_deref(),
            Some("PD" | "nf" | "fi" | "EX" | "EE" | "An" | "Sm" | "ft" | "in" | "sp" | "br" | "Pp")
        )
}

/// Whether this node ends the current no-fill execution row.
///
/// This is deliberately independent from [`is_no_fill_payload`].  Native
/// requests such as `ft`, `PD`, and the presentation-only width/tab controls
/// execute without calling `term_newln()` or `term_flushln()`, so pending
/// `\c`, `\p`, and `\z` state crosses them.  Requests that establish a real
/// line boundary are settled before their normal block dispatch executes.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum NoFillBoundary {
    None,
    Line,
    NoBreak,
}

pub(super) fn no_fill_boundary(node: &Node) -> NoFillBoundary {
    // `mc` flushes with TERMP_NOBREAK: it commits the current cells but keeps
    // the next formatter word on the same visual row.  Device-specific margin
    // geometry is omitted, but the source-neutral projection retains one
    // ordinary boundary rather than inventing a newline.
    if node.macro_name.as_deref() == Some("mc") {
        return NoFillBoundary::NoBreak;
    }
    // These requests call term_newln()/term_flushln() in the pinned CVS
    // terminal executor.  Check them before payload classification because
    // roff control nodes can retain NODE_NOFILL and inline-flow children.
    if matches!(
        node.macro_name.as_deref(),
        Some("br" | "ce" | "rj" | "fi" | "nf" | "EX" | "EE" | "sp" | "in" | "ti" | "Pp")
    ) {
        return NoFillBoundary::Line;
    }
    if is_no_fill_payload(node) {
        return NoFillBoundary::None;
    }
    if matches!(
        node.macro_name.as_deref(),
        Some(
            "ft" | "PD"
                | "Sm"
                | "Tg"
                | "ad"
                | "na"
                | "hy"
                | "nh"
                | "ne"
                | "nr"
                | "ta"
                | "DT"
                | "ll"
                | "po"
        )
    ) {
        NoFillBoundary::None
    } else {
        NoFillBoundary::Line
    }
}

fn lower_no_fill_lines(
    node: &Node,
    default_name: Option<&str>,
    font: &mut FontState,
    inline_state: &mut super::NoFillInlineState,
) -> Option<Vec<LoweredNoFillLine>> {
    if is_no_fill_payload(node) {
        let (mut nodes, continues_line) = lower_no_fill_line_with_font_state(
            std::slice::from_ref(node),
            default_name,
            true,
            font,
            inline_state,
            ends_with_line_continuation(node),
        );
        let mut occupies_row = !nodes.is_empty();
        if nodes.is_empty() {
            let blank_rows = empty_word_rows(node);
            occupies_row = blank_rows > 0;
            for index in 0..blank_rows {
                if index > 0 {
                    nodes.push(Inline::LineBreak);
                }
                nodes.push(Inline::Text {
                    value: String::new(),
                });
            }
        }
        return Some(vec![LoweredNoFillLine {
            nodes,
            source: source_span(node),
            continues_line,
            starts_line: node.flags.line_start,
            occupies_row,
        }]);
    }
    None
}

/// `man_term` renders every empty TEXT as vertical space, including a B/I
/// operand; `ESCAPE_IGNORE` instead buffers an invisible glyph on the current
/// row. Pure font escapes do neither. Inspect typed decoded events only when
/// ordinary lowering returned no visible payload.
fn empty_word_rows(node: &Node) -> usize {
    // Alternating font macros call term_word on operands themselves instead
    // of visiting man TEXT nodes, so their empty parameters are not vspace.
    let empty_text_is_row =
        crate::mandoc::inline::alternating_font_pair(node.macro_name.as_deref()).is_none();
    let mut stack = vec![node];
    let mut blanks = 0usize;
    let mut zero_width_glyph = false;
    while let Some(node) = stack.pop() {
        if node.flags.no_print || node.kind == NodeKind::Comment {
            continue;
        }
        if node.kind == NodeKind::Text {
            let text = node.text.as_deref().unwrap_or_default();
            if text.is_empty() && empty_text_is_row {
                blanks = blanks.saturating_add(1);
            } else {
                zero_width_glyph |= crate::mandoc::roff_escape::decode(text)
                    .iter()
                    .any(|event| {
                        matches!(
                            event,
                            crate::mandoc::roff_escape::RoffInlineEvent::ZeroWidthGlyph
                        )
                    });
            }
        } else {
            stack.extend(node.children.iter().rev());
        }
    }
    blanks.max(usize::from(zero_width_glyph))
}

impl super::BlockLowerer<'_, '_> {
    pub(super) fn push_no_fill_lines(&mut self, node: &Node) -> bool {
        let Some(lines) = lower_no_fill_lines(
            node,
            self.context.default_name,
            &mut self.formatter.font,
            &mut self.no_fill_inline,
        ) else {
            return false;
        };
        for line in lines {
            self.state.push_preformatted(
                line.nodes,
                line.source,
                line.continues_line,
                line.starts_line,
                line.occupies_row,
            );
        }
        true
    }

    pub(super) fn push_no_fill_synopsis(&mut self, node: &Node) -> bool {
        let body = first_part_children(node, NodeKind::Body);
        if node.macro_name.as_deref() != Some("SY") || !body.iter().any(|child| child.flags.no_fill)
        {
            return false;
        }
        self.state.flush_paragraph();
        self.state.flush_preformatted();
        self.state
            .queue_targets(targets::structural_targets(node), source_span(node));
        let head = first_part_children(node, NodeKind::Head);
        let saved = self
            .formatter
            .font
            .push_scope(crate::mandoc::roff_escape::RoffFont::Strong);
        let nodes = lower_inline_nodes_with_font_state(
            head,
            self.context.default_name,
            self.state.spacing_enabled(),
            &mut self.formatter.font,
        );
        self.formatter.font.pop_scope(saved);
        if !nodes.is_empty() {
            self.state.push_preformatted(
                nodes,
                source_span(node),
                head.last().is_some_and(ends_with_line_continuation),
                true,
                true,
            );
        }
        // SY is a scope, not a promise that its whole body is no-fill.
        // Execute each child through normal block dispatch so fi/nf, spacing
        // and structural children cannot become flattened pseudo-text.
        self.push_nodes(body);
        self.state.flush_paragraph();
        self.state.flush_preformatted();
        self.formatter.font = FontState::new();
        true
    }
}
