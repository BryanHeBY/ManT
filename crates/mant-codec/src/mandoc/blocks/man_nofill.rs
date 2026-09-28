//! Man no-fill words retain executed empty rows, not formatter operands.
use super::{
    Inline, Node, NodeKind, append_inline_node_with_next, ends_with_line_continuation,
    first_part_children, participates_in_inline_flow, source_span, targets,
};
use crate::mandoc::controls::{FormatterBoundary, formatter_control};
use crate::mandoc::inline::lower_no_fill_fragment_with_formatter;

struct LoweredNoFillLine {
    nodes: Vec<Inline>,
    source: Option<mant_ir::SourceSpan>,
    continues_line: bool,
    starts_line: bool,
    occupies_row: bool,
}

#[derive(Clone, Copy)]
struct NoFillSource<'a> {
    node: &'a Node,
    next: Option<&'a Node>,
    source_line_entered: bool,
    single_line_literal: bool,
    default_name: Option<&'a str>,
    macro_set: libmandoc_rs::MacroSet,
}

pub(super) fn is_no_fill_payload(node: &Node, single_line_literal: bool) -> bool {
    (node.flags.no_fill || single_line_literal)
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
pub(super) fn no_fill_boundary(node: &Node, single_line_literal: bool) -> FormatterBoundary {
    if let Some(control) = formatter_control(node.macro_name.as_deref()) {
        return control.boundary;
    }
    // mdoc_term.c::termp_rs_pre() has no row break in DESCRIPTION. Rs enters
    // its BODY through the same source-line dispatcher as adjacent text.
    if is_no_fill_payload(node, single_line_literal) || node.macro_name.as_deref() == Some("Rs") {
        FormatterBoundary::None
    } else {
        FormatterBoundary::Line
    }
}

fn lower_no_fill_lines(
    source: NoFillSource<'_>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Option<Vec<LoweredNoFillLine>> {
    let NoFillSource {
        node,
        next,
        source_line_entered,
        single_line_literal,
        default_name,
        macro_set,
    } = source;
    if is_no_fill_payload(node, single_line_literal) {
        let (mut nodes, continues_line) = lower_no_fill_fragment_with_formatter(
            formatter,
            ends_with_line_continuation(node),
            false,
            |builder| {
                append_inline_node_with_next(builder, node, next, default_name);
            },
        );
        let mut occupies_row = !nodes.is_empty();
        if nodes.is_empty() {
            let blank_rows = empty_word_rows(node, macro_set == libmandoc_rs::MacroSet::Mdoc);
            occupies_row = blank_rows > 0;
            // An empty native TEXT calls term_vspace() at this source node.
            // Settle its formatter cell now; otherwise the next request
            // would count the same authored blank row a second time.
            if node.kind == NodeKind::Text && node.decoder_text().unwrap_or_default().is_empty() {
                nodes.extend(
                    formatter
                        .no_fill_inline
                        .take_settled_row(&mut formatter.execution),
                );
            }
            for index in usize::from(!nodes.is_empty())..blank_rows {
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
            starts_line: node.flags.line_start && !source_line_entered,
            occupies_row,
        }]);
    }
    None
}

/// `man_term` renders every empty TEXT as vertical space, including a B/I
/// operand; `ESCAPE_IGNORE` instead buffers an invisible glyph on the current
/// row. Pure font escapes do neither. Inspect typed decoded events only when
/// ordinary lowering returned no visible payload.
fn empty_word_rows(node: &Node, is_mdoc: bool) -> usize {
    // Alternating font macros call term_word on operands themselves instead
    // of visiting man TEXT nodes, so their empty parameters are not vspace.
    // mdoc_term.c::print_mdoc_node() only interprets an empty TEXT as a blank
    // input row under NODE_LINE; an empty macro argument merely calls
    // term_word(""). man_term.c interprets every visited empty TEXT as vspace.
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
            let text = node.decoder_text().unwrap_or_default();
            if text.is_empty() && empty_text_is_row && (!is_mdoc || node.flags.line_start) {
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
    pub(super) fn push_no_fill_lines(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        source_line_entered: bool,
        single_line_literal: bool,
    ) -> bool {
        if !is_no_fill_payload(node, single_line_literal) {
            return false;
        }
        self.resume_no_fill_row();
        if !self.state.paragraph_is_empty() {
            self.state.flush_paragraph();
        }
        if node.flags.line_start
            && !source_line_entered
            && !self.state.formatter.no_fill_inline.continues_source_line()
        {
            self.settle_no_fill_inline();
        }
        // `nf`/`fi` split presentation buffers, not the native formatter.
        // Move a surviving bare BACKAFTER request into the no-fill executor;
        // an occupied cell was already settled by the mode boundary.
        let formatter = &mut self.state.formatter;
        let Some(lines) = lower_no_fill_lines(
            NoFillSource {
                node,
                next,
                source_line_entered,
                single_line_literal,
                default_name: self.context.default_name,
                macro_set: self.context.macro_set,
            },
            formatter,
        ) else {
            unreachable!("a no-fill payload must lower as a no-fill row");
        };
        // Every accepted no-fill payload represents a real `term_word()` and
        // consequently clears formatter-global negative `.sp` debt.
        self.state.clear_formatter_word_debt();
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
        let nodes = super::synopsis::execute_synopsis_head(
            node,
            self.context,
            self.state.spacing_enabled(),
            &mut self.state.formatter,
        );
        if !nodes.is_empty() {
            self.state.push_preformatted(
                nodes,
                source_span(node),
                head.last().is_some_and(ends_with_line_continuation),
                true,
                true,
            );
        }
        self.state
            .formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular); // BODY pre
        // SY is a scope, not a promise that its whole body is no-fill.
        // Execute each child through normal block dispatch so fi/nf, spacing
        // and structural children cannot become flattened pseudo-text.
        self.push_nodes(body);
        // man_term.c::post_SY() calls term_newln() for the BODY at .YS.
        // Settle its active no-fill row inside the synopsis output owner.
        self.settle_no_fill_inline();
        self.state.flush_paragraph();
        self.state.flush_preformatted();
        self.state
            .formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular); // BODY post
        self.state
            .formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular); // BLOCK post
        true
    }
}
