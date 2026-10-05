//! Assemble definition HEAD/BODY owners using one projected inline context.

mod body;
mod phrasing;
use body::definition_body;
use phrasing::DefinitionHead;
pub(super) use phrasing::{InlineRoot, render_roots};

use mant_ir::{Block, DefinitionItem, EntryOwner};

use super::super::inline::block_prefix_escape_position;
use super::super::mapped::{BlockSyntax, MappedText};
use super::super::{MarkdownInlineProjection, MarkdownOptions};
use super::{assembly, join_definition_items};

pub(super) fn render_definition_list(
    items: &[DefinitionItem],
    compact: bool,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let mut rendered = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let DefinitionContent {
            mut content,
            has_terms,
        } = definition_content(item, options, locations, track);
        content.contribution.before |=
            mant_ir::geometry::list_item_spacing(item.layout.spacing_before_lines, index, compact)
                > 0;
        let Some(mut content) = content.nonempty() else {
            continue;
        };
        if !content.contribution.rows {
            rendered.push((
                content.with_owner(EntryOwner::Definition(item), track),
                item.layout.spacing_before_lines,
            ));
            continue;
        }
        // Protect block syntax formed across the final HEAD/BODY seam.
        if has_terms && let Some(position) = block_prefix_escape_position(&content.text) {
            content.insert(position, "\\");
        }
        if let Some(content) = content
            .with_owner(EntryOwner::Definition(item), track)
            .prefix("- ")
        {
            rendered.push((content, item.layout.spacing_before_lines));
        }
    }
    join_definition_items(rendered, compact)
}

struct DefinitionContent {
    content: MappedText,
    has_terms: bool,
}

fn mapped_terms(terms: Vec<(Vec<usize>, String)>, roots: &[InlineRoot<'_>]) -> MappedText {
    let terms = terms.into_iter().map(|(indices, text)| {
        let last = indices
            .iter()
            .rev()
            .map(|&index| &roots[index])
            .find(|root| root.has_output);
        MappedText::from(text)
            .tail_grammar(
                BlockSyntax::Phrasing,
                last.is_some_and(|root| root.open_row),
            )
            .hard_rows(last.is_some_and(|root| root.hard_rows))
    });
    MappedText::join(terms, "  \n")
}

fn definition_content(
    item: &DefinitionItem,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> DefinitionContent {
    let mut head = DefinitionHead::project(&item.terms, options, locations);
    let has_terms = head.has_output;
    let mut terms = std::mem::take(&mut head.terms);
    let start = item.description_start();
    let shared_prose = start.is_some_and(|body| {
        body.can_share(item.head_body_relation) && matches!(body.block, Block::Paragraph { .. })
    });
    let mut body = definition_body(item, start, options, locations, track, &head);
    if has_terms
        && !body.navigation.is_empty()
        && let Some((indices, term)) = terms.last_mut()
    {
        // Navigation is phrasing on an existing row, never an independent
        // paragraph. Borrow original roots into the same inline context so
        // authored hard tails and adjacent delimiters retain their rules.
        // Prefix zero-width syntax before the occupied HEAD, so it cannot
        // make an authored open tail appear to contain a new word.
        *term = render_roots(
            body.navigation
                .iter()
                .chain(indices.iter().map(|&index| &head.roots[index])),
            options,
        );
    }
    if let (Some((indices, term)), Some(prose)) = (terms.last_mut(), &body.first_prose)
        && has_terms
        && shared_prose
        && item.head_body_relation.joins_without_separator()
    {
        // Source ownership stays split; one inline context selects delimiters
        // for the physical row. Independent encoded strings cannot be joined:
        // adjacent strong/emphasis/code delimiters may become literal text.
        let roots = body
            .navigation
            .iter()
            .chain(indices.iter().map(|&index| &head.roots[index]))
            .chain(prose.iter());
        let hard_rows = roots.clone().all(|root| !root.has_output || root.hard_rows);
        *term = render_roots(roots, options);
        let open_row = prose
            .iter()
            .rev()
            .find(|root| root.has_output)
            .is_some_and(|root| root.open_row);
        body.blocks.remove(0);
        body.first_prose = None;
        let single_term = terms.len() == 1;
        let mut head = mapped_terms(terms, &head.roots);
        head.tail.open_row = open_row;
        if single_term {
            head.hard_rows = hard_rows;
        }
        head.tail.hard_rows = hard_rows;
        let tail = assembly::join(body.blocks);
        let content = if tail.text.is_empty() {
            head
        } else {
            assembly::join([head, tail])
        };
        let mut content = content.syntax_site(BlockSyntax::Phrasing);
        content.contribution.after |= body.pending_space;
        content.attach_navigation_text(body.carriers, false);
        return DefinitionContent { content, has_terms };
    }
    let mut head = if !has_terms && body.first_prose.is_none() && body.first_block.is_some() {
        // Zero-output HEAD destinations were already framed with the BODY.
        MappedText::default()
    } else if body.blocks.is_empty() && !has_terms {
        render_roots(head.roots.iter().chain(body.navigation.iter()), options).into()
    } else {
        mapped_terms(terms, &head.roots)
    };
    let first_syntax = body.blocks.first().map(|block| block.syntax);
    let description = assembly::join(body.blocks);
    // Separate term tails and Paragraph provisional tails have different
    // closing rules. Let the actual next syntax (or positive EOF boundary)
    // complete this row once. Shared prose still occupies the original tail.
    head.tail.term_tail = has_terms && head.tail.open_row && !shared_prose;
    if !has_terms && body.leading_space && !description.text.is_empty() {
        // An empty list marker followed by a blank line cannot retain a
        // two-column-indented BODY. Encode the already requested blank row
        // with the hard-row contract: one marker row, then one completed
        // empty row. No scalar, link, or implicit navigation gap is created.
        head.insert(0, "<br />\n\n");
    }
    let mut content = match (head.text.is_empty(), description.text.is_empty()) {
        (false, false) if !has_terms => {
            // Only destinations were retained from HEAD: preserve their
            // syntax without fabricating a label row or a word separator.
            MappedText::join([head, description], "")
        }
        (false, false) if !body.leading_space && first_syntax == Some(BlockSyntax::Phrasing) => {
            if body.first_prose.is_some() && shared_prose {
                MappedText::join([head, description], " ")
            } else {
                assembly::hard_row(&mut head);
                head.append(description);
                head
            }
        }
        (false, false) => assembly::join([head, description]),
        (false, true) => head,
        (true, false) => description,
        (true, true) => MappedText::default().syntax_site(BlockSyntax::Phrasing),
    };
    if has_terms || body.leading_space || body.first_block.is_none() {
        content = content.syntax_site(BlockSyntax::Phrasing);
    }
    content.attach_navigation_text(body.carriers, false);
    content.contribution.rows = has_terms || body.first_block.is_some();
    content.contribution.before =
        !content.contribution.rows && (body.leading_space || body.pending_space);
    content.contribution.after = content.contribution.rows && body.pending_space;
    DefinitionContent { content, has_terms }
}
