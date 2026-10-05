//! Borrow effective BODY content, navigation carriers and resolved boundaries.
use super::phrasing::{DefinitionHead, InlineRoot, render_roots};
use crate::encode::{
    MarkdownInlineProjection, MarkdownOptions,
    blocks::render_block,
    mapped::{BlockSyntax, MappedText},
};
use mant_ir::{Block, DefinitionBodyRef, DefinitionItem, InlineContentRef};

/// Keep the first effective BODY block and its already executed boundary
/// together. `VerticalSpace`'s distance can simplify in `CommonMark`, but its
/// paragraph boundary cannot disappear when an empty output block is skipped.
pub(super) struct DefinitionBody<'a> {
    pub(super) blocks: Vec<MappedText>,
    pub(super) first_prose: Option<Vec<InlineRoot<'a>>>,
    pub(super) navigation: Vec<InlineRoot<'a>>,
    pub(super) first_block: Option<&'a Block>,
    pub(super) leading_space: bool,
    pub(super) carriers: MappedText,
    pub(super) pending_space: bool,
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

pub(super) fn definition_body<'a>(
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
