//! Maps native block nodes to portable `CommonMark` block constructs.

mod definitions;

use std::borrow::Cow;

use definitions::{InlineRoot, render_definition_list, render_roots};

use mant_ir::{
    Block, EntryOwner, Inline, InlineContentRef, ListItem, ListKind, SourceSpan, TableRow,
    content_entries,
};

use super::inline::{
    code_span, escape_text, fenced_code, flatten_inline_content, html_anchor,
    preformatted_anchor_markers, protect_block_prefix, render_inline_content,
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
    mapped_sequence(blocks, options, locations, false)
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
    MappedText::join(mapped_sequence(blocks, options, locations, track), "\n\n")
}

fn mapped_sequence(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Vec<MappedText> {
    coalesce_navigation(
        blocks
            .iter()
            .filter_map(|block| render_block(block, options, locations, track)),
    )
}

fn coalesce_navigation(values: impl IntoIterator<Item = MappedText>) -> Vec<MappedText> {
    let mut rendered = Vec::new();
    let mut pending = MappedText::default();
    for mut block in values {
        if block.navigation_only {
            pending.append(block);
        } else {
            block.attach_navigation_text(std::mem::take(&mut pending), false);
            rendered.push(block);
        }
    }
    if let Some(first) = rendered.first_mut() {
        first.attach_navigation_text(pending, true);
    } else if !pending.text.is_empty() {
        rendered.push(pending);
    }
    rendered
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
        } => nonempty(inline(
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
        } => {
            let code = fenced_code(
                &flatten_inline_content(InlineContentRef {
                    content: children,
                    layout: inline_layout,
                }),
                language.as_deref(),
            );
            if options.preserve_anchors {
                let markers = preformatted_anchor_markers(children);
                if !markers.is_empty() {
                    return Some(
                        MappedText::from(format!("{markers}\n\n{code}")).navigation_site(false),
                    );
                }
            }
            Some(code.into())
        }
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
    };
    rendered.map(|text| match block {
        Block::List { .. } | Block::DefinitionList { .. } | Block::Table { .. } => text,
        Block::Paragraph { .. }
        | Block::Unsupported { .. }
        | Block::Equation { display: false, .. } => text.navigation_site(false),
        _ => text.navigation_site(true),
    })
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
                ListKind::Bullet | ListKind::Dash | ListKind::Plain => "- ".to_owned(),
            };
            let mut blocks = list_item_blocks(&item.blocks, options, locations, track);
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
            if content.text.is_empty() {
                content = content.navigation_site(false);
            }
            if !options.preserve_semantics
                && options.preserve_anchors
                && let Some(facts) = &item.entry
            {
                // A retained anchor uses the actual first block's syntax;
                // it cannot be glued to a fence or steal a child's receiver.
                content.attach_navigation(&html_anchor(&facts.id));
                content.navigation_only = false;
            }
            let content = content.with_owner(EntryOwner::List(item), track);
            if content.navigation_only {
                Some((content, item.layout.spacing_before_lines))
            } else {
                content
                    .prefix(&marker)
                    .map(|content| (content, item.layout.spacing_before_lines))
            }
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

fn list_item_blocks(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Vec<MappedText> {
    let mut rendered = Vec::new();
    let mut pending = Vec::new();
    let mut markers = None;
    let mut leading_space = false;
    for block in blocks {
        if rendered.is_empty() {
            leading_space |= mant_ir::geometry::block_gap(block) > 0;
        }
        if let Block::Paragraph {
            children,
            inline_layout,
            ..
        } = block
        {
            let root = InlineRoot::project(
                InlineContentRef {
                    content: children,
                    layout: inline_layout,
                },
                locations,
            );
            let has_output = root.has_output;
            pending.push(root);
            markers = None;
            if has_output {
                rendered.push(
                    MappedText::from(render_roots(pending.iter(), options)).navigation_site(false),
                );
                pending.clear();
            }
        } else if let Some(block) = render_block_with_navigation(
            block,
            options,
            locations,
            track,
            markers.get_or_insert_with(|| render_roots(pending.iter(), options)),
        ) {
            rendered.push(block);
            pending.clear();
            markers = None;
        }
    }
    let markers = render_roots(pending.iter(), options);
    if let Some(first) = rendered.first_mut() {
        first.append_navigation(&markers);
    } else if let Some(markers) = nonempty(markers) {
        rendered.push(markers.navigation_site(false));
    }
    let mut rendered = coalesce_navigation(rendered);
    if leading_space && !rendered.is_empty() {
        // Represent the already resolved positive boundary independently of
        // a transparent prefix. The first marker row frames the real blank.
        rendered.insert(
            0,
            MappedText::from("<br />".to_owned()).navigation_site(false),
        );
    }
    rendered
}

/// Frame navigation using the actual block syntax without making a paragraph
/// gap. This consumes original blocks once and retains their mapped owners.
fn render_block_with_navigation(
    block: &Block,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
    navigation: &str,
) -> Option<MappedText> {
    if matches!(block, Block::ThematicBreak { .. }) {
        // A dash rule after phrasing becomes a setext underline; directly
        // after a bullet marker it can also escape the surrounding item.
        let mut rendered = MappedText::from("***".to_owned()).navigation_site(true);
        rendered.attach_navigation(navigation);
        return Some(rendered);
    }
    let mut rendered = render_block(block, options, locations, track)?;
    rendered.attach_navigation(navigation);
    Some(rendered)
}

/// Preserve a man(7) `.PD` override when one is present, otherwise fall back
/// to the list-wide compactness used by mdoc(7) and HTML inputs.
fn join_definition_items(
    items: Vec<(MappedText, Option<u16>)>,
    compact: bool,
) -> Option<MappedText> {
    let mut physical = Vec::new();
    let mut pending = MappedText::default();
    for (mut item, spacing) in items {
        if item.navigation_only {
            pending.append(item);
        } else {
            item.attach_navigation_text(std::mem::take(&mut pending), false);
            physical.push((item, spacing));
        }
    }
    if let Some((first, _)) = physical.first_mut() {
        first.attach_navigation_text(pending, true);
    } else {
        return pending.nonempty();
    }
    let mut items = physical.into_iter();
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

fn render_table(rows: &[TableRow], options: MarkdownOptions, track: bool) -> Option<MappedText> {
    // Empty data rows and native rules are authored structure, not fence
    // delimiters. Keep them in the same source-order portable row plan.
    let markers = if options.preserve_anchors {
        rows.iter()
            .filter(|row| mant_ir::table_row_is_navigation_only(row))
            .flat_map(|row| &row.cells)
            .flat_map(|cell| &cell.blocks)
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(preformatted_anchor_markers(children)),
                _ => None,
            })
            .collect::<String>()
    } else {
        String::new()
    };
    let rows = super::flat::rows(rows, track);
    if rows.is_empty() {
        return nonempty(markers).map(|markers| markers.navigation_site(false));
    }
    Some({
        let mut body = MappedText::join(rows, "\n");
        let fenced = fenced_code(&body.text, None);
        let prefix = fenced.find('\n').expect("fence header") + 1;
        for (_, range) in &mut body.owners {
            range.start += prefix;
            range.end += prefix;
        }
        body.text = fenced;
        if !markers.is_empty() {
            body.insert(0, &format!("{markers}\n\n"));
        }
        body.navigation_site(markers.is_empty())
    })
}

fn nonempty(value: String) -> Option<MappedText> {
    MappedText::from(value).nonempty()
}

fn inline(
    content: InlineContentRef<'_>,
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
) -> String {
    render_inline_content(
        InlineContentRef {
            content: &project_inline(content.content, locations),
            layout: content.layout,
        },
        options,
    )
}

fn project_inline<'a>(
    nodes: &'a [Inline],
    locations: Option<&dyn MarkdownInlineProjection>,
) -> Cow<'a, [Inline]> {
    locations.map_or(Cow::Borrowed(nodes), |map| map.project(nodes))
}
