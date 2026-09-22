//! Maps native block nodes to portable `CommonMark` block constructs.

use mant_ir::{
    Block, ContentContext, ContentRootKey, DefinitionItem, EntryOwner, Inline, InlineView,
    ListItem, ListKind, SourceSpan, TableRow, content_entries,
};

use super::inline::{
    block_prefix_escape_position, code_span, escape_text, fenced_code, flatten_inline, html_anchor,
    protect_block_prefix, render_inline,
};
use super::mapped::MappedText;
use super::source_map::RenderedRootRange;
use super::{MarkdownInlineProjection, MarkdownOptions};

pub(super) struct RenderedBlocks<'src> {
    pub(super) text: String,
    pub(super) entries: Vec<RenderedEntry<'src>>,
    pub(super) roots: Vec<RenderedRootRange>,
}

pub(super) struct RenderedEntry<'src> {
    pub(super) indices: Vec<usize>,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) owner: EntryOwner<'src>,
    pub(super) names: &'src [String],
    pub(super) source: Option<SourceSpan>,
}

pub(crate) fn render_blocks(
    content: ContentContext<'_>,
    blocks: &[Block],
    options: MarkdownOptions,
) -> Vec<String> {
    render_located_blocks(content, blocks, options, None)
}
pub(crate) fn render_located_blocks(
    content: ContentContext<'_>,
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| render_block(content, block, options, locations, false))
        .map(|block| block.text)
        .collect()
}

pub(super) fn render_blocks_with_entries<'a>(
    content: ContentContext<'a>,
    blocks: &'a [Block],
    options: MarkdownOptions,
    track: bool,
) -> RenderedBlocks<'a> {
    let rendered = mapped_blocks(content, blocks, options, None, track);
    let entries = if track {
        let ranges = rendered.owner_ranges();
        content_entries(content, blocks)
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
        roots: rendered.roots,
    }
}

fn mapped_blocks(
    content: ContentContext<'_>,
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> MappedText {
    MappedText::join(
        blocks
            .iter()
            .filter_map(|block| render_block(content, block, options, locations, track)),
        "\n\n",
    )
}

fn render_block(
    content: ContentContext<'_>,
    block: &Block,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    match block {
        Block::Paragraph { children, .. } => {
            nonempty(inline(content, children, options, locations, track))
        }
        Block::Preformatted {
            children, language, ..
        } => Some(
            MappedText::from(fenced_code(
                &flatten_inline(content, children),
                language.as_deref(),
            ))
            .with_root(track.then(|| inline_root(content, children)).flatten()),
        ),
        Block::List {
            kind,
            compact,
            items,
            ..
        } => render_list(content, *kind, *compact, items, options, locations, track),
        Block::DefinitionList { items, compact, .. } => {
            render_definition_list(content, items, *compact, options, locations, track)
        }
        Block::Table { rows, .. } => render_table(content, rows, track),
        Block::Equation { value, display, .. } => {
            if *display {
                Some(fenced_code(value, Some("math")).into())
            } else {
                nonempty(format!("Equation: {}", code_span(value)).into())
            }
        }
        Block::VerticalSpace { .. } => None,
        Block::ThematicBreak { .. } => Some("---".to_owned().into()),
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
    }
}

fn render_list(
    content: ContentContext<'_>,
    kind: ListKind,
    compact: bool,
    items: &[ListItem],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let rendered = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let marker = match kind {
                ListKind::Ordered { .. } => {
                    format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                }
                ListKind::Bullet | ListKind::Plain => "- ".to_owned(),
            };
            let mut blocks = item
                .blocks
                .iter()
                .filter_map(|block| render_block(content, block, options, locations, track))
                .collect::<Vec<_>>();
            if options.preserve_semantics
                && let Some(facts) = &item.entry
            {
                if let Some(head) = blocks.first_mut() {
                    head.text.push_str(&super::semantic::metadata(facts));
                }
                if let Some(domain) = facts
                    .value_domain
                    .as_ref()
                    .and_then(super::semantic::domain)
                {
                    blocks.insert(1, domain.into());
                }
            }
            let mut rendered_content = MappedText::join(blocks, "\n\n");
            if !options.preserve_semantics
                && options.preserve_anchors
                && let Some(facts) = &item.entry
            {
                rendered_content.insert(0, &html_anchor(&facts.id));
            }
            rendered_content
                .with_owner(EntryOwner::List(item), track)
                .prefix(&marker)
                .map(|content| (content, item.layout.spacing_before_lines))
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| {
        let mut rendered_content =
            join_definition_items(rendered, compact).expect("nonempty list items");
        if options.preserve_semantics
            && let Some(facts) = items.first().and_then(|i| i.entry.as_ref())
        {
            rendered_content.insert(
                0,
                &format!("{}\n", super::semantic::declaration(content, facts, items)),
            );
        }
        rendered_content
    })
}

fn render_definition_list(
    content: ContentContext<'_>,
    items: &[DefinitionItem],
    compact: bool,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let rendered = items
        .iter()
        .filter_map(|item| {
            let terms = MappedText::join(
                item.terms
                    .iter()
                    .map(|term| inline(content, term, options, locations, track))
                    .filter(|term| !term.text.is_empty()),
                "  \n",
            );
            let description = mapped_blocks(content, &item.description, options, locations, track);
            let has_terms = !terms.text.is_empty();
            let mut content = match (terms.text.is_empty(), description.text.is_empty()) {
                (false, false) => {
                    // A definition term can share a physical line only with
                    // inline prose. Gluing a fenced code block, nested list,
                    // table, or display equation to the term produces invalid
                    // CommonMark and changes the block's meaning.
                    let sep = if item.layout.inline_term
                        && matches!(item.description.first(), Some(Block::Paragraph { .. }))
                    {
                        " "
                    } else {
                        "\n"
                    };
                    MappedText::join([terms, description], sep)
                }
                (false, true) => terms,
                (true, false) => description,
                (true, true) => return None,
            };
            // `render_inline` protects block markers visible inside one inline
            // run, but a hanging definition can assemble the marker only when
            // its term and description are joined (`1.` + ` text`). Protect
            // that final first line so a semantic definition cannot reparse as
            // a nested ordered list in the public CommonMark projection.
            if has_terms && let Some(position) = block_prefix_escape_position(&content.text) {
                content.insert(position, "\\");
            }
            content
                .with_owner(EntryOwner::Definition(item), track)
                .prefix("- ")
                .map(|content| (content, item.layout.spacing_before_lines))
        })
        .collect::<Vec<_>>();
    join_definition_items(rendered, compact)
}

/// Preserve a man(7) `.PD` override when one is present, otherwise fall back
/// to the list-wide compactness used by mdoc(7) and HTML inputs.
fn join_definition_items(
    items: Vec<(MappedText, Option<u16>)>,
    compact: bool,
) -> Option<MappedText> {
    let mut items = items.into_iter();
    let (mut output, _) = items.next()?;
    for (item, spacing_before_lines) in items {
        let blank_lines = spacing_before_lines.unwrap_or(u16::from(!compact));
        output
            .text
            .push_str(&"\n".repeat(usize::from(blank_lines) + 1));
        output.append(item);
    }
    Some(output)
}

fn render_table(content: ContentContext<'_>, rows: &[TableRow], track: bool) -> Option<MappedText> {
    let rows = super::flat::rows(content, rows, track)
        .into_iter()
        .filter(|row| !row.text.trim().is_empty())
        .collect::<Vec<_>>();
    (!rows.is_empty()).then(|| {
        let mut body = MappedText::join(rows, "\n");
        let fenced = fenced_code(&body.text, None);
        let prefix = fenced.find('\n').expect("fence header") + 1;
        for (_, range) in &mut body.owners {
            range.start += prefix;
            range.end += prefix;
        }
        for root in &mut body.roots {
            root.markdown.start += prefix;
            root.markdown.end += prefix;
        }
        body.text = fenced;
        body
    })
}

fn nonempty(value: MappedText) -> Option<MappedText> {
    value.nonempty()
}

fn inline(
    content: ContentContext<'_>,
    nodes: &[mant_ir::Inline],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track_roots: bool,
) -> MappedText {
    let rendered = locations.map_or_else(
        || render_inline(content, nodes, options),
        |map| {
            super::inline::render_inline_projected(
                content,
                nodes,
                options,
                map.scalar_ranges(nodes),
            )
        },
    );
    MappedText::from(rendered).with_root(
        (track_roots && locations.is_none())
            .then(|| inline_root(content, nodes))
            .flatten(),
    )
}

pub(super) fn inline_root(content: ContentContext<'_>, nodes: &[Inline]) -> Option<ContentRootKey> {
    fn visit(
        content: ContentContext<'_>,
        nodes: &[Inline],
        root: &mut Option<ContentRootKey>,
    ) -> Option<()> {
        for node in nodes {
            let candidate = match content.inline(node).ok()? {
                InlineView::Text(_) | InlineView::Code(_) => match node {
                    Inline::Text { content } | Inline::Code { content } => content.atom,
                    _ => unreachable!("resolved text keeps its structural leaf"),
                },
                InlineView::LineBreak => match node {
                    Inline::LineBreak { atom } => *atom,
                    _ => unreachable!("resolved break keeps its structural leaf"),
                },
                InlineView::Anchor(anchor) => {
                    let candidate = content.point(anchor.point())?.root;
                    if root.is_some_and(|root| root != candidate) {
                        return None;
                    }
                    *root = Some(candidate);
                    continue;
                }
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    visit(content, children, root)?;
                    continue;
                }
                InlineView::Link(link) => {
                    visit(content, link.children(), root)?;
                    continue;
                }
                _ => return None,
            };
            let candidate = content.atom(candidate)?.root;
            if root.is_some_and(|root| root != candidate) {
                return None;
            }
            *root = Some(candidate);
        }
        Some(())
    }

    let mut root = None;
    visit(content, nodes, &mut root)?;
    root
}
