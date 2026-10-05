//! Maps source-neutral IR blocks to portable `CommonMark` block constructs.

mod assembly;
mod definitions;
mod lists;
mod table;
use lists::{join_definition_items, render_list};
use table::render_table;

use std::borrow::Cow;

use definitions::{InlineRoot, render_definition_list, render_roots};

use mant_ir::{Block, EntryOwner, Inline, InlineContentRef, SourceSpan, content_entries};

use super::inline::{
    code_span, escape_text, fenced_code, flatten_inline_content, preformatted_anchor_markers,
    protect_block_prefix,
};
use super::mapped::{BlockSyntax, MappedText};
use super::{MarkdownInlineProjection, MarkdownOptions};

pub(super) struct RenderedBlocks<'src> {
    pub(super) text: String,
    pub(super) entries: Vec<RenderedEntry<'src>>,
}

pub(super) struct RenderedEntry<'src> {
    pub(super) indices: Vec<usize>,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) owner: EntryOwner<'src>,
    pub(super) names: &'src [String],
    pub(super) source: Option<SourceSpan>,
}

pub(crate) fn render_blocks(blocks: &[Block], options: MarkdownOptions) -> Vec<String> {
    render_located_blocks(blocks, options, None)
}
pub(crate) fn render_located_blocks(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> Vec<String> {
    mapped_blocks(blocks, options, locations, false)
        .nonempty()
        .into_iter()
        .map(|block| block.text)
        .collect()
}

pub(super) fn render_blocks_with_entries(
    blocks: &[Block],
    options: MarkdownOptions,
    track: bool,
) -> RenderedBlocks<'_> {
    let rendered = mapped_blocks(blocks, options, None, track);
    let entries = if track {
        let ranges = rendered.owner_ranges();
        content_entries(blocks)
            .into_iter()
            .filter_map(|located| {
                let owner = located.owner();
                let identity = owner.facts()?;
                let range = ranges.get(&std::ptr::from_ref(identity))?;
                Some(RenderedEntry {
                    owner,
                    names: located.names(),
                    indices: located.indices().to_vec(),
                    start: range.start,
                    end: range.end,
                    source: located.source(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    RenderedBlocks {
        text: rendered.text,
        entries,
    }
}

fn mapped_blocks(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> MappedText {
    assembly::join(assembly::finish_boundaries(mapped_sequence(
        blocks, options, locations, track,
    )))
}

fn mapped_sequence(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Vec<MappedText> {
    assembly::coalesce(
        blocks
            .iter()
            .filter_map(|block| render_block(block, options, locations, track)),
    )
}

fn render_block(
    block: &Block,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let rendered = match block {
        Block::Paragraph {
            children,
            inline_layout,
            ..
        } => Some(render_phrasing(
            InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            options,
            locations,
        )),
        Block::Preformatted {
            children,
            language,
            inline_layout,
            ..
        } => Some(render_literal(
            InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            language.as_deref(),
            options,
            locations,
        )),
        Block::List {
            kind,
            compact,
            items,
            ..
        } => render_list(*kind, *compact, items, options, locations, track),
        Block::DefinitionList { items, compact, .. } => {
            render_definition_list(items, *compact, options, locations, track)
        }
        Block::Table { rows, .. } => render_table(rows, options, track),
        Block::Equation { value, display, .. } => {
            if *display {
                Some(
                    MappedText::from(fenced_code(value, Some("math")))
                        .syntax_site(BlockSyntax::Fence)
                        .tail_grammar(BlockSyntax::Fence, false),
                )
            } else {
                nonempty(format!("Equation: {}", code_span(value)))
            }
        }
        Block::VerticalSpace { .. } => {
            Some(MappedText::default().syntax_site(BlockSyntax::Phrasing))
        }
        // A dash rule can become a setext heading after zero-width navigation.
        // The star spelling is safe at every root and nested receiver.
        Block::ThematicBreak { .. } => Some(
            MappedText::from("***".to_owned())
                .syntax_site(BlockSyntax::Rule)
                .tail_grammar(BlockSyntax::Rule, false),
        ),
        Block::Unsupported { name, text, .. } => {
            let text = escape_text(text.trim())
                .lines()
                .map(protect_block_prefix)
                .collect::<Vec<_>>()
                .join("  \n");
            if text.is_empty() {
                None
            } else {
                Some(
                    name.as_deref()
                        .map_or(text.clone(), |name| {
                            format!("**{}:** {text}", escape_text(name))
                        })
                        .into(),
                )
            }
        }
    };
    let mut rendered =
        rendered.unwrap_or_else(|| MappedText::default().syntax_site(BlockSyntax::Phrasing));
    if rendered.navigation.is_none() {
        rendered = rendered.syntax_site(BlockSyntax::Phrasing);
    }
    rendered.contribution.before |= mant_ir::geometry::block_gap(block) > 0;
    rendered.nonempty()
}

fn render_phrasing(
    content: InlineContentRef<'_>,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> MappedText {
    let root = InlineRoot::project(content, locations);
    let mut rendered = MappedText::from(render_roots(std::iter::once(&root), options))
        .syntax_site(BlockSyntax::Phrasing)
        .tail_grammar(BlockSyntax::Phrasing, root.open_row)
        .hard_rows(root.hard_rows);
    rendered.contribution.rows = root.has_output;
    rendered
}

fn render_literal(
    content: InlineContentRef<'_>,
    language: Option<&str>,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> MappedText {
    // Empty roots and targets alone own no authored literal row at any address.
    // Text("") and explicit hard breaks do, even without a visible scalar.
    if !mant_ir::geometry::has_literal_rows(content.content) {
        return render_phrasing(content, options, locations);
    }
    let code = fenced_code(&flatten_inline_content(content), language);
    let markers = if options.preserve_anchors {
        preformatted_anchor_markers(content.content)
    } else {
        String::new()
    };
    let rendered = if markers.is_empty() {
        MappedText::from(code).syntax_site(BlockSyntax::Fence)
    } else {
        MappedText::from(format!("{markers}\n\n{code}")).syntax_site(BlockSyntax::Phrasing)
    };
    rendered.tail_grammar(BlockSyntax::Fence, false)
}

fn nonempty(value: String) -> Option<MappedText> {
    MappedText::from(value).nonempty()
}

fn project_inline<'a>(
    nodes: &'a [Inline],
    locations: Option<&dyn MarkdownInlineProjection>,
) -> Cow<'a, [Inline]> {
    locations.map_or(Cow::Borrowed(nodes), |map| map.project(nodes))
}
