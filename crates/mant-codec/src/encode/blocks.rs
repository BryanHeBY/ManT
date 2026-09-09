//! Maps native block nodes to portable `CommonMark` block constructs.

use mant_ir::{
    Block, DefinitionItem, EntryOwner, ListItem, ListKind, SourceSpan, TableRow, content_entries,
};

use super::inline::{
    block_prefix_escape_position, code_span, escape_text, fenced_code, flatten_inline, html_anchor,
    protect_block_prefix, render_inline,
};
use super::mapped::MappedText;
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
    blocks
        .iter()
        .filter_map(|block| render_block(block, options, locations, false))
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
    MappedText::join(
        blocks
            .iter()
            .filter_map(|block| render_block(block, options, locations, track)),
        "\n\n",
    )
}

fn render_block(
    block: &Block,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    match block {
        Block::Paragraph { children, .. } => nonempty(inline(children, options, locations)),
        Block::Preformatted {
            children, language, ..
        } => Some(fenced_code(&flatten_inline(children), language.as_deref()).into()),
        Block::List {
            kind,
            compact,
            items,
            ..
        } => render_list(*kind, *compact, items, options, locations, track),
        Block::DefinitionList { items, compact, .. } => {
            render_definition_list(items, *compact, options, locations, track)
        }
        Block::Table { rows, .. } => render_table(rows, track),
        Block::Equation { value, display, .. } => {
            if *display {
                Some(fenced_code(value, Some("math")).into())
            } else {
                nonempty(format!("Equation: {}", code_span(value)))
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
                .filter_map(|block| render_block(block, options, locations, track))
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
            let mut content = MappedText::join(blocks, "\n\n");
            if !options.preserve_semantics
                && options.preserve_anchors
                && let Some(facts) = &item.entry
            {
                content.insert(0, &html_anchor(&facts.id));
            }
            content
                .with_owner(EntryOwner::List(item), track)
                .prefix(&marker)
                .map(|content| (content, item.layout.spacing_before_lines))
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| {
        let mut content = join_definition_items(rendered, compact).expect("nonempty list items");
        if options.preserve_semantics
            && let Some(facts) = items.first().and_then(|i| i.entry.as_ref())
        {
            content.insert(
                0,
                &format!("{}\n", super::semantic::declaration(facts, items)),
            );
        }
        content
    })
}

fn render_definition_list(
    items: &[DefinitionItem],
    compact: bool,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Option<MappedText> {
    let rendered = items
        .iter()
        .filter_map(|item| {
            let terms = item
                .terms
                .iter()
                .map(|term| inline(term, options, locations))
                .filter(|term| !term.is_empty())
                .collect::<Vec<_>>()
                .join("  \n");
            let description = mapped_blocks(&item.description, options, locations, track);
            let has_terms = !terms.is_empty();
            let mut content = match (terms.is_empty(), description.text.is_empty()) {
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
                    MappedText::join([terms.into(), description], sep)
                }
                (false, true) => terms.into(),
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

fn render_table(rows: &[TableRow], track: bool) -> Option<MappedText> {
    let rows = super::flat::rows(rows, track)
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
        body.text = fenced;
        body
    })
}

fn nonempty(value: String) -> Option<MappedText> {
    MappedText::from(value).nonempty()
}

fn inline(
    nodes: &[mant_ir::Inline],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> String {
    locations.map_or_else(
        || render_inline(nodes, options),
        |map| render_inline(&map.project(nodes), options),
    )
}
