//! Resolve executed definition rows into source-neutral head/body layout.

use super::head::definition_head_anchor;
use super::{Block, DefinitionFlow, DefinitionHeadFlow, Inline, Node, source_span};

pub(super) fn apply_executed_definition_body_layout(
    geometry: &mut crate::mandoc::layout::DefinitionGeometry,
    flow: DefinitionFlow,
    closed_head_row: bool,
    definition_field_exited: bool,
    observed: &crate::mandoc::formatter::DefinitionBodyObservation,
    head_source_continues: bool,
    description: &[Block],
) {
    if flow.shares_pending_term_row && observed.placement_breaks() {
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    // NOSPACE describes the next word, not the relation to a previous
    // physical row. A closed HEAD stays separate even if pre_br left that
    // register armed (roff_term.c:69-78, mdoc_term.c::termp_it_post()).
    if !closed_head_row && !definition_field_exited && observed.first_word_flushed_at_body() {
        // The cleared field filled its capacity (term.c:250-253 with
        // 205-207): the body shares the head's row starting at the
        // description column, with no separator cell to count.
        geometry.relation_override = Some((
            mant_ir::HeadBodyRelation::joined(),
            mant_ir::DefinitionBodyAlignment::Indented,
        ));
        geometry.gap = 0;
    } else if !closed_head_row && !definition_field_exited && observed.first_word_concatenated() {
        // TERMP_NOSPACE at the body's first word leaves no separator cell:
        // the body column starts at the head's end (roff_term.c:75-78),
        // so the layout carries no minimum gap.
        geometry.relation_override = Some((
            mant_ir::HeadBodyRelation::joined(),
            mant_ir::DefinitionBodyAlignment::AfterTerm,
        ));
        geometry.gap = 0;
    }
    if flow.head.generated_cells().is_some_and(|count| count > 0)
        && (observed.materialized_run_in_boundary() || observed.joined_run_in_word())
        && !matches!(
            geometry.placement,
            crate::mandoc::layout::TermPlacement::Stacked
        )
        && !closed_head_row
        && !definition_field_exited
        && !observed.placement_breaks()
    {
        // The actual generated BODY word already owns this accepted scalar
        // seam. Adding a second generic word separator would duplicate it.
        geometry.relation_override = Some((
            mant_ir::HeadBodyRelation::joined(),
            mant_ir::DefinitionBodyAlignment::AfterTerm,
        ));
    }
    if matches!(description.first(), Some(Block::Preformatted { .. }))
        && !observed
            .source_continues_after_run_in()
            .unwrap_or(head_source_continues)
    {
        // A literal BODY can run in only when CVS kept the source row open
        // with \\c. Ordinary no-fill NODE_LINE starts a fresh physical row.
        // That executed close also supersedes a provisional relation from
        // the first word's NOSPACE/column state: a word register cannot join
        // a row that print_mdoc_node() already ended (mdoc_term.c:314-318).
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
        geometry.relation_override = None;
    }
}

pub(super) fn normalize_executed_definition_head(
    term: Vec<Inline>,
    term_breaks: &[usize],
    node: &Node,
    flow: DefinitionFlow,
    geometry: &mut crate::mandoc::layout::DefinitionGeometry,
) -> (Vec<Vec<Inline>>, bool) {
    let mut terms = split_definition_terms(term, term_breaks);
    if matches!(flow.head, DefinitionHeadFlow::Detached { .. }) {
        for term in &mut terms {
            // CVS term.c::term_fill() drops a field made solely of ordinary
            // breakable spaces. Such a HEAD owns no printed row, so an early
            // BODY .sp must not make its IR term appear as another blank row.
            // Fixed blanks (\~ and \0) remain real cells in the UTF-8 device.
            if term.iter().all(only_breakable_head_padding) {
                clear_breakable_head_padding(term);
            }
        }
    }
    if let Some(id) = definition_head_anchor(node) {
        if terms.is_empty() {
            terms.push(Vec::new());
        }
        terms[0].insert(0, Inline::anchor_at(id, source_span(node)));
    }
    // Only an executed line boundary can close the HEAD row. An empty
    // operand without NODE_LINE still runs term_word(), not term_vspace()
    // (mdoc_term.c:354-378); later BODY words consume that same raw field.
    let closed_head_row = take_closed_head_row(&mut terms);
    if closed_head_row {
        // The last explicit HEAD break becomes the term/BODY separation.
        // Earlier breaks, including extra sp rows, remain inside the term.
        // Heads whose in-word `\p` or armed trailing marker closed the
        // native row follow the same term_flushln() tail rule
        // (term.c:250-252).
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if flow.head.generated_cells().is_some() {
        // The native generated cells execute inside the shared stream below.
        // Their surviving projection is carried by the description itself;
        // adding the static geometry gap as well would count it twice.
        geometry.gap = 0;
    }
    (terms, closed_head_row)
}

fn only_breakable_head_padding(inline: &Inline) -> bool {
    match inline {
        Inline::Anchor { .. } | Inline::LineBreak { .. } => true,
        Inline::Text { value } | Inline::Code { value } => value.chars().all(|ch| ch == ' '),
        Inline::Emphasis { children }
        | Inline::Strong { children }
        | Inline::Link { children, .. } => children.iter().all(only_breakable_head_padding),
        // Equations and other semantic nodes are not ordinary term_fill()
        // padding cells.
        Inline::Equation { .. } => false,
    }
}

fn clear_breakable_head_padding(term: &mut Vec<Inline>) {
    term.retain_mut(|inline| match inline {
        Inline::Anchor { .. } | Inline::LineBreak { .. } => true,
        Inline::Link { children, .. } => {
            // Preserve the typed destination while removing its unprinted
            // whitespace label. Link resolution may later unwrap an unknown
            // section target; an empty child cannot revive the field row.
            clear_breakable_head_padding(children);
            true
        }
        Inline::Emphasis { children } | Inline::Strong { children } => {
            clear_breakable_head_padding(children);
            !children.is_empty()
        }
        Inline::Text { .. } | Inline::Code { .. } => false,
        Inline::Equation { .. } => unreachable!("padding predicate excludes equations"),
    });
}

fn take_closed_head_row(terms: &mut [Vec<Inline>]) -> bool {
    // Semantic wrappers cannot change term_newln()/endline() output. Move
    // exactly one final executed boundary into the structural HEAD/BODY
    // relation, preserving earlier blank rows and every authored target.
    // Pinned mdoc_term.c::termp_it_post(), term.c::term_flushln().
    fn take_boundary(nodes: &mut Vec<Inline>) -> Option<bool> {
        let mut index = nodes.len();
        while index > 0 {
            index -= 1;
            match &mut nodes[index] {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => {
                    nodes.remove(index);
                    return Some(true);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if let Some(closed) = take_boundary(children) {
                        return Some(closed);
                    }
                }
                // Even an empty Text can represent a newly occupied native
                // cell. It is not identity metadata to skip over.
                _ => return Some(false),
            }
        }
        None
    }

    let Some(term) = terms.last_mut() else {
        return false;
    };
    take_boundary(term) == Some(true)
}

fn invisible_closed_head_row(terms: &[Vec<Inline>]) -> bool {
    terms.last().is_some_and(|term| {
        !term.iter().any(|inline| {
            matches!(inline, Inline::LineBreak { .. })
                || mant_ir::has_printable_character(std::slice::from_ref(inline))
        })
    })
}

pub(super) fn transfer_completed_empty_head_rows(terms: &mut [Vec<Inline>], rows: u16) -> u16 {
    if rows == 0
        || terms
            .iter()
            .any(|term| mant_ir::has_printable_character(term))
    {
        return 0;
    }
    // term_vspace() already executed each endline (term.c:489-497). An
    // otherwise empty term has no glyph row to render their last boundary.
    // Deliver the receipt once to BODY layout; neither an implicit term row
    // nor its retained LineBreak projection may account for the same rows.
    // Keep every authored identity even when its HEAD has no readable label.
    for term in terms {
        crate::mandoc::inline::retain_inline_identities(term);
    }
    rows
}

pub(super) fn carry_completed_head_rows(
    node: &Node,
    terms: &[Vec<Inline>],
    closed_head_row: bool,
    completed_empty_rows: u16,
    description: &mut Vec<Block>,
) {
    let rows = if completed_empty_rows > 0 {
        completed_empty_rows
    } else {
        u16::from(closed_head_row && invisible_closed_head_row(terms))
    };
    if rows > 0 {
        // term_newln() flushed a HEAD cell, but an invisible term has no IR
        // renderer. Transfer its completed physical row to the BODY owner.
        description.insert(
            0,
            Block::VerticalSpace {
                lines: rows,
                source: source_span(node),
            },
        );
    }
}

/// Split alternatives embedded in one extended mdoc definition head.
///
/// libmandoc retains `.Pp` inside `It Xo ... Xc` as an inline child. In that
/// position it separates equivalent term spellings rather than starting a
/// new description paragraph. The IR already models such aliases as several
/// terms on one definition item, so preserve that structure explicitly.
pub(in crate::mandoc::blocks::lists) fn split_definition_terms(
    term: Vec<Inline>,
    alternative_breaks: &[usize],
) -> Vec<Vec<Inline>> {
    let mut terms = Vec::new();
    let mut current = Vec::new();
    let mut alternatives = alternative_breaks.iter().copied().peekable();
    for (index, node) in term.into_iter().enumerate() {
        if node == Inline::line_break() && alternatives.peek() == Some(&index) {
            alternatives.next();
            if mant_ir::has_printable_character(&current) {
                let origin = crate::mandoc::inline::split_row_origin(&mut current);
                terms.push(std::mem::take(&mut current));
                current.extend(origin);
            } else {
                // An executed but invisible author word (`\&`, or a
                // control-only `\p`) still owns a physical formatter row.
                // Keep that row attached to the next visible spelling rather
                // than manufacturing an empty semantic alternative that
                // renderers are required to ignore.
                current
                    .retain(|inline| !matches!(inline, Inline::Text { value } if value.is_empty()));
                current.push(Inline::line_break());
            }
        } else {
            current.push(node);
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}
