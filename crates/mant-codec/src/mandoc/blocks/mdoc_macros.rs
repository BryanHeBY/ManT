//! mdoc(7)-specific macro handling, the seed of the analog of upstream
//! `mdoc_html.c`'s `mdoc_html_acts` dispatch family.

use libmandoc_rs::{MacroToken::Mdoc, MdocMacro, Node, NodeKind};

use super::{super::source_span, Block, BlockLowerer};

impl BlockLowerer<'_, '_> {
    /// CVS `mdoc_term.c` bibliography wrapper: `termp_rs_pre()` calls
    /// `term_vspace` only for a non-first `Rs` in SEE ALSO; the wrapper
    /// itself owns no post text or formatter row break.
    pub(super) fn push_bibliography(&mut self, node: &Node) {
        // CVS termp_rs_pre() calls term_vspace only for a non-first Rs in
        // SEE ALSO. This is an executed pre boundary, before BODY fields.
        if node.section == libmandoc_rs::NormalizedSection::SeeAlso && self.paragraph_predecessor {
            self.settle_no_fill_inline();
            self.state.flush_preformatted();
            let lines = self.state.resolve_vertical_space(1);
            self.state.flush_paragraph_for_line_request();
            self.state.output.push(Block::VerticalSpace {
                lines,
                source: source_span(node),
            });
        }
        let Some(body) = node
            .children
            .iter()
            .find(|child| child.kind == NodeKind::Body)
        else {
            return;
        };
        let posts = self.context.scope_posts.clone();
        posts.enter_body(body.id, self.state.formatter.font.checkpoint());
        self.push_nodes_with_reference_posts(&body.children, true);
        if let Some(saved) = posts.exit_body(body.id) {
            self.state.formatter.font.pop_scope(saved);
        }
    }
}

fn is_reference_field(node: &Node) -> bool {
    matches!(
        node.macro_token.as_ref(),
        Some(Mdoc(
            MdocMacro::PercentA
                | MdocMacro::PercentB
                | MdocMacro::PercentC
                | MdocMacro::PercentD
                | MdocMacro::PercentI
                | MdocMacro::PercentJ
                | MdocMacro::PercentN
                | MdocMacro::PercentO
                | MdocMacro::PercentP
                | MdocMacro::PercentQ
                | MdocMacro::PercentR
                | MdocMacro::PercentT
                | MdocMacro::PercentU
                | MdocMacro::PercentV
        ))
    )
}

/// `roff.c::roff_node_prev()/next()` skip comments, NOPRT nodes and the
/// formatter's transparent requests before bibliography pre/post handlers
/// inspect neighboring fields.
fn next_reference_sibling(nodes: &[Node], index: usize) -> Option<usize> {
    ((index + 1)..nodes.len())
        .find(|&next| super::super::adjacency::is_logical_sibling(&nodes[next]))
}

fn previous_reference_sibling(nodes: &[Node], index: usize) -> Option<usize> {
    (0..index)
        .rev()
        .find(|&previous| super::super::adjacency::is_logical_sibling(&nodes[previous]))
}

/// CVS `mdoc_term.c::termp__a_pre()` adds `and` before the final author.
pub(super) fn reference_author_conjunction(nodes: &[Node], index: usize) -> bool {
    nodes[index].macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
        && previous_reference_sibling(nodes, index).is_some_and(|previous| {
            nodes[previous].macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
        })
        && next_reference_sibling(nodes, index)
            .is_none_or(|next| nodes[next].macro_token.as_ref() != Some(&Mdoc(MdocMacro::PercentA)))
}

/// CVS `mdoc_term.c::termp____post()` omits the first comma for exactly two
/// adjacent authors, then uses a period only after the final Rs field.
pub(super) fn reference_field_post(nodes: &[Node], index: usize) -> Option<&'static str> {
    let node = &nodes[index];
    if !is_reference_field(node) {
        return None;
    }
    let next = next_reference_sibling(nodes, index);
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
        && next.is_some_and(|next| {
            nodes[next].macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
        })
        && next
            .and_then(|next| next_reference_sibling(nodes, next))
            .is_none_or(|after| {
                nodes[after].macro_token.as_ref() != Some(&Mdoc(MdocMacro::PercentA))
            })
        && previous_reference_sibling(nodes, index).is_none_or(|previous| {
            nodes[previous].macro_token.as_ref() != Some(&Mdoc(MdocMacro::PercentA))
        })
    {
        return None;
    }
    Some(if next.is_none() { "." } else { "," })
}
