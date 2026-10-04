//! Assemble definition HEAD/BODY owners using one projected inline context.

mod phrasing;
use phrasing::DefinitionHead;
pub(super) use phrasing::{InlineRoot, render_roots};

use mant_ir::{Block, DefinitionBodyRef, DefinitionItem, EntryOwner, InlineContentRef};

use super::super::inline::block_prefix_escape_position;
use super::super::mapped::MappedText;
use super::super::{MarkdownInlineProjection, MarkdownOptions};
use super::{join_definition_items, nonempty, render_block};

pub(super) fn render_definition_list(
    items: &[DefinitionItem],
    compact: bool,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let mut rendered = Vec::new();
    let mut pending = MappedText::default();
    for item in items {
        let Some(DefinitionContent {
            mut content,
            has_terms,
            physical,
        }) = definition_content(item, options, locations, track)
        else {
            continue;
        };
        if !physical {
            content.navigation_only = true;
            pending.append(content.with_owner(EntryOwner::Definition(item), track));
            continue;
        }
        // Protect block syntax formed across the final HEAD/BODY seam.
        if has_terms && let Some(position) = block_prefix_escape_position(&content.text) {
            content.insert(position, "\\");
        }
        if let Some(mut content) = content
            .with_owner(EntryOwner::Definition(item), track)
            .prefix("- ")
        {
            content.attach_navigation_text(std::mem::take(&mut pending), false);
            rendered.push((content, item.layout.spacing_before_lines));
        }
    }
    if let Some((first, _)) = rendered.first_mut() {
        first.attach_navigation_text(pending, true);
    } else {
        return pending.nonempty();
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
    };
    let mut destinations = Vec::new();
    let mut markers = None;
    for (index, block) in item.description.iter().enumerate() {
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
            if let Some(root) = inline_body_root(block, locations) {
                destinations.push(root);
                markers = None;
            }
            continue;
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
            destinations.push(projected);
            markers = None;
            if !has_output {
                continue;
            }
            let rendered = render_roots(destinations.iter(), options);
            if body.blocks.is_empty() {
                body.first_prose = Some(std::mem::take(&mut destinations));
                body.first_block = Some(block);
                body.blocks
                    .push(MappedText::from(rendered).navigation_site(false));
            } else if let Some(rendered) = nonempty(rendered) {
                body.blocks.push(rendered.navigation_site(false));
                destinations.clear();
            }
        } else if let Some(mut rendered) = render_block(block, options, locations, track) {
            if rendered.navigation_only {
                // An omitted child has destinations, not a physical BODY.
                // Keep its borrowed owner ranges without inventing a marker.
                body.carriers.append(rendered);
                continue;
            }
            if matches!(block, Block::ThematicBreak { .. }) {
                rendered.text = "***".into();
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
    physical: bool,
}

fn definition_content(
    item: &DefinitionItem,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<DefinitionContent> {
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
        *term = render_roots(roots, options);
        body.blocks.remove(0);
        body.first_prose = None;
        let head = terms.into_iter().map(|(_, text)| MappedText::from(text));
        let head = MappedText::join(head, "  \n");
        let tail = MappedText::join(body.blocks, "\n\n");
        let content = if tail.text.is_empty() {
            head
        } else {
            MappedText::join([head, tail], "\n\n")
        };
        let mut content = content.navigation_site(false);
        content.attach_navigation_text(body.carriers, false);
        return Some(DefinitionContent {
            content,
            has_terms,
            physical: true,
        });
    }
    let mut head = if !has_terms && body.first_prose.is_none() && body.first_block.is_some() {
        // Zero-output HEAD destinations were already framed with the BODY.
        MappedText::default()
    } else if body.blocks.is_empty() && !has_terms {
        render_roots(head.roots.iter().chain(body.navigation.iter()), options).into()
    } else {
        MappedText::join(terms.into_iter().map(|(_, text)| text.into()), "  \n")
    };
    let description = MappedText::join(body.blocks, "\n\n");
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
        (false, false) => {
            let separator = if body.leading_space
                || matches!(body.first_block, Some(Block::List { kind: mant_ir::ListKind::Ordered { start: Some(start) }, .. }) if *start != 1)
            {
                "\n\n"
            } else if body.first_prose.is_some() {
                if shared_prose { " " } else { "  \n" }
            } else {
                // Fences, nested lists and display equations need their own
                // block; they never share the term's inline coding context.
                "\n"
            };
            MappedText::join([head, description], separator)
        }
        (false, true) => head,
        (true, false) => description,
        (true, true) => MappedText::default().navigation_site(false),
    };
    if has_terms || body.leading_space || body.first_block.is_none() {
        content = content.navigation_site(false);
    }
    content.attach_navigation_text(body.carriers, false);
    if content.text.is_empty() {
        return None;
    }
    Some(DefinitionContent {
        content,
        has_terms,
        physical: has_terms || body.first_block.is_some(),
    })
}
