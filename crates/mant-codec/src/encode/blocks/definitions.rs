//! Assemble definition HEAD/BODY owners using one projected inline context.

mod phrasing;
use phrasing::DefinitionHead;
pub(super) use phrasing::{InlineRoot, render_roots};

use mant_ir::{Block, DefinitionBodyRef, DefinitionItem, EntryOwner, InlineContentRef};

use super::super::inline::block_prefix_escape_position;
use super::super::mapped::{BlockSyntax, MappedText};
use super::super::{MarkdownInlineProjection, MarkdownOptions};
use super::{assembly, join_definition_items, render_block};

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

/// Keep the first effective BODY block and its already executed boundary
/// together. `VerticalSpace`'s distance can simplify in `CommonMark`, but its
/// paragraph boundary cannot disappear when an empty output block is skipped.
struct DefinitionBody<'a> {
    blocks: Vec<MappedText>,
    first_prose: Option<Vec<InlineRoot<'a>>>,
    navigation: Vec<InlineRoot<'a>>,
    first_block: Option<&'a Block>,
    leading_space: bool,
    carriers: MappedText,
    pending_space: bool,
}

impl<'a> DefinitionBody<'a> {
    fn finish_navigation(
        &mut self,
        destinations: Vec<InlineRoot<'a>>,
        head: &DefinitionHead<'_>,
        options: MarkdownOptions,
    ) {
        if self.blocks.is_empty() || head.has_output {
            self.navigation.extend(destinations);
        } else if !destinations.is_empty() {
            let markers = render_roots(destinations.iter(), options);
            self.blocks
                .first_mut()
                .expect("a physical BODY block")
                .append_navigation(&markers);
        }
    }
}

fn definition_body<'a>(
    item: &'a DefinitionItem,
    start: Option<DefinitionBodyRef<'a>>,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
    head: &DefinitionHead<'_>,
) -> DefinitionBody<'a> {
    let mut body = DefinitionBody {
        blocks: Vec::new(),
        first_prose: None,
        navigation: Vec::new(),
        first_block: None,
        leading_space: start.is_some_and(|body| body.has_leading_spacing),
        carriers: MappedText::default(),
        pending_space: false,
    };
    let mut destinations = Vec::new();
    let mut markers = None;
    for (index, block) in item.description.iter().enumerate() {
        body.pending_space |= mant_ir::geometry::block_gap(block) > 0;
        if body.blocks.is_empty() {
            // Selection stops at structural content even if this format
            // omits it. Later executed gaps still precede the first export
            // output; they must not collapse into an ordinary hard row.
            body.leading_space |= mant_ir::geometry::block_gap(block) > 0;
        }
        if start.is_none_or(|body| index < body.block_index) {
            // Borrow the transparent prefix at its original owner address.
            // In particular, an empty literal root with no authored row must
            // not become an empty fence which changes BODY selection.
            // Structural carriers still render below: no effective inline BODY
            // does not prove that a nested container has no positive boundary.
            if let Some(root) = inline_body_root(block, locations) {
                destinations.push(root);
                markers = None;
                continue;
            }
        }
        if matches!(block, Block::VerticalSpace { .. }) {
            continue;
        }
        if let Block::Paragraph {
            children,
            inline_layout,
            ..
        } = block
        {
            // Decoration receives the original root, never an assembled seam.
            let projected = InlineRoot::project(
                InlineContentRef {
                    content: children,
                    layout: inline_layout,
                },
                locations,
            );
            let has_output = projected.has_output;
            let open_row = projected.open_row;
            let hard_rows = projected.hard_rows;
            destinations.push(projected);
            markers = None;
            if !has_output {
                continue;
            }
            let rendered = render_roots(destinations.iter(), options);
            let mut rendered = MappedText::from(rendered)
                .syntax_site(BlockSyntax::Phrasing)
                .tail_grammar(BlockSyntax::Phrasing, open_row)
                .hard_rows(hard_rows);
            rendered.contribution.before = std::mem::take(&mut body.pending_space);
            if body.blocks.is_empty() {
                body.first_prose = Some(std::mem::take(&mut destinations));
                body.first_block = Some(block);
                body.blocks.push(rendered);
            } else if let Some(rendered) = rendered.nonempty() {
                body.blocks.push(rendered);
                destinations.clear();
            }
        } else if let Some(mut rendered) = render_block(block, options, locations, track) {
            if !rendered.contribution.rows {
                body.pending_space |= rendered.contribution.spacing();
                if body.blocks.is_empty() {
                    body.leading_space |= body.pending_space;
                }
                // An omitted child has destinations, not a physical BODY.
                // Keep its borrowed owner ranges without inventing a marker.
                body.carriers.append(rendered);
                continue;
            }
            rendered.contribution.before |= std::mem::take(&mut body.pending_space);
            if body.blocks.is_empty() {
                body.leading_space |= rendered.contribution.before;
            }
            if !body.blocks.is_empty() || !head.has_output {
                let markers = markers.get_or_insert_with(|| {
                    render_roots(
                        head.roots
                            .iter()
                            .filter(|_| body.blocks.is_empty())
                            .chain(destinations.iter()),
                        options,
                    )
                });
                rendered.attach_navigation(markers);
            }
            if body.blocks.is_empty() {
                body.first_block = Some(block);
                body.navigation = std::mem::take(&mut destinations);
            } else {
                destinations.clear();
            }
            markers = None;
            body.pending_space = rendered.contribution.after;
            body.blocks.push(rendered);
        }
    }
    body.finish_navigation(destinations, head, options);
    body
}

fn inline_body_root<'a>(
    block: &'a Block,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> Option<InlineRoot<'a>> {
    match block {
        Block::Paragraph {
            children,
            inline_layout,
            ..
        }
        | Block::Preformatted {
            children,
            inline_layout,
            ..
        } => Some(InlineRoot::project(
            InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            locations,
        )),
        _ => None,
    }
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
