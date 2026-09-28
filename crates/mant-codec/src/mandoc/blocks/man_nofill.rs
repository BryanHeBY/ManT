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
    vertical_assertion: VerticalAssertion,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum VerticalAssertion {
    None,
    Asserted,
}

#[derive(Clone, Copy)]
struct NoFillSource<'a> {
    node: &'a Node,
    next: Option<&'a Node>,
    source_line_entered: bool,
    single_line_literal: bool,
    default_name: Option<&'a str>,
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
    // These block scopes do not execute term_newln() on entry. The common
    // NODE_NOFILL/NODE_LINE rule decides whether the source row ends; a
    // preceding \c can carry it into their BODY or generated post text.
    // See man_term.c::pre_UR() and mdoc_term.c::termp_rs_pre().
    if is_no_fill_payload(node, single_line_literal)
        || matches!(node.macro_name.as_deref(), Some("Rs" | "UR" | "MT"))
    {
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
    } = source;
    if is_no_fill_payload(node, single_line_literal) {
        let (mut nodes, continues_line, asserted_vertical_row) =
            lower_no_fill_fragment_with_formatter(
                formatter,
                ends_with_line_continuation(node),
                false,
                |builder| {
                    append_inline_node_with_next(builder, node, next, default_name);
                },
            );
        let mut occupies_row = !nodes.is_empty();
        if nodes.is_empty() {
            occupies_row = has_invisible_word_cell(node);
            if occupies_row {
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
            vertical_assertion: if asserted_vertical_row {
                VerticalAssertion::Asserted
            } else {
                VerticalAssertion::None
            },
        }]);
    }
    None
}

/// A zero-width formatter word occupies a row without a printable glyph.
/// Empty TEXT is executed as `term_vspace()` during the actual inline walk;
/// reconstructing its rows after the fact would lose skipvsp and \c state.
fn has_invisible_word_cell(node: &Node) -> bool {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if node.flags.no_print || node.kind == NodeKind::Comment {
            continue;
        }
        if node.kind == NodeKind::Text {
            let text = node.decoder_text().unwrap_or_default();
            if crate::mandoc::roff_escape::decode(text)
                .iter()
                .any(|event| {
                    matches!(
                        event,
                        crate::mandoc::roff_escape::RoffInlineEvent::ZeroWidthGlyph
                    )
                })
            {
                return true;
            }
        } else {
            stack.extend(node.children.iter().rev());
        }
    }
    false
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
            },
            formatter,
        ) else {
            unreachable!("a no-fill payload must lower as a no-fill row");
        };
        // Actual term_word() calls clear skipvsp in begin_word_projection().
        // A visited empty TEXT instead calls term_vspace(), possibly consuming
        // that debt without producing a visible row.
        for line in lines {
            self.state.push_preformatted(
                line.nodes,
                line.source,
                line.continues_line,
                line.starts_line,
                line.occupies_row,
            );
            if line.vertical_assertion == VerticalAssertion::Asserted {
                self.state.mark_literal_vertical_row();
            }
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
