//! Man no-fill words retain executed empty rows, not formatter operands.
use super::{
    Node, NodeKind, append_inline_node_with_next, ends_with_line_continuation, first_part_children,
    participates_in_inline_flow, source_span, targets,
};
use crate::mandoc::controls::{FormatterBoundary, formatter_control};
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, RoffMacro,
};

pub(super) fn is_no_fill_payload(node: &Node, single_line_literal: bool) -> bool {
    (node.flags.no_fill || single_line_literal)
        && participates_in_inline_flow(node)
        && !matches!(
            node.macro_token.as_ref(),
            Some(
                Man(ManMacro::Pd | ManMacro::Ex | ManMacro::Ee | ManMacro::In)
                    | Roff(
                        RoffMacro::Nf
                            | RoffMacro::Fi
                            | RoffMacro::Ft
                            | RoffMacro::Sp
                            | RoffMacro::Br
                    )
                    | Mdoc(MdocMacro::Sm | MdocMacro::Pp)
            )
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
    if let Some(control) = formatter_control(node.macro_token.as_ref()) {
        return control.boundary;
    }
    // These block scopes do not execute term_newln() on entry. The common
    // NODE_NOFILL/NODE_LINE rule decides whether the source row ends; a
    // preceding \c can carry it into their BODY or generated post text.
    // See man_term.c::pre_UR() and mdoc_term.c::termp_rs_pre().
    // Payload ownership is separate from row execution. A state-only inline
    // macro such as `An -split` emits no word, but its entry still follows
    // NODE_LINE and a preceding \c continuation.
    if ((node.flags.no_fill || single_line_literal) && participates_in_inline_flow(node))
        // Bf/Bk and transparent Xo share the surrounding word buffer.
        // Their pre handlers do not call term_newln(); only the common
        // NODE_LINE gate can close it (mdoc_term.c:314-318,1799,1921).
        || crate::mandoc::containers::is_container(node)
        || matches!(node.macro_token.as_ref(), Some(Mdoc(MdocMacro::Rs) | Man(ManMacro::Ur | ManMacro::Mt)))
    {
        FormatterBoundary::None
    } else {
        FormatterBoundary::Line
    }
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
        let default_name = self.context.default_name;
        self.state.execute_no_fill_fragment(
            source_span(node),
            ends_with_line_continuation(node),
            false,
            |builder| append_inline_node_with_next(builder, node, next, default_name),
        );
        true
    }

    pub(super) fn push_no_fill_synopsis(&mut self, node: &Node) -> bool {
        let body = first_part_children(node, NodeKind::Body);
        if node.macro_token.as_ref() != Some(&Man(ManMacro::Sy))
            || !body.iter().any(|child| child.flags.no_fill)
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
