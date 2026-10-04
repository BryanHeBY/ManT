//! Maps source-neutral IR blocks to portable `CommonMark` block constructs.

mod assembly;
mod definitions;

use std::borrow::Cow;

use definitions::{InlineRoot, render_definition_list, render_roots};

use mant_ir::{
    Block, EntryOwner, Inline, InlineContentRef, ListItem, ListKind, SourceSpan, TableRow,
    content_entries,
};

use super::inline::{
    code_span, escape_text, fenced_code, flatten_inline_content, html_anchor,
    preformatted_anchor_markers, protect_block_prefix,
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
    assembly::finish_boundaries(mapped_sequence(blocks, options, locations, false))
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
    MappedText::join(
        assembly::finish_boundaries(mapped_sequence(blocks, options, locations, track)),
        "\n\n",
    )
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
        .tail_grammar(BlockSyntax::Phrasing, root.open_row);
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
            content.contribution.before |= mant_ir::geometry::list_item_spacing(
                item.layout.spacing_before_lines,
                index,
                compact,
            ) > 0;
            if content.text.is_empty() {
                content = content.syntax_site(BlockSyntax::Phrasing);
            }
            if !options.preserve_semantics
                && options.preserve_anchors
                && let Some(facts) = &item.entry
            {
                // A retained anchor uses the actual first block's syntax;
                // it cannot be glued to a fence or steal a child's receiver.
                content.attach_navigation(&html_anchor(&facts.id));
            }
            let content = content
                .with_owner(EntryOwner::List(item), track)
                .nonempty()?;
            if content.contribution.rows {
                content
                    .prefix(&marker)
                    .map(|content| (content, item.layout.spacing_before_lines))
            } else {
                Some((content, item.layout.spacing_before_lines))
            }
        })
        .collect::<Vec<_>>();
    let mut content = join_definition_items(rendered, compact)?;
    if options.preserve_semantics
        && let Some(facts) = items.first().and_then(|i| i.entry.as_ref())
    {
        content.insert(
            0,
            &format!("{}\n", super::semantic::declaration(facts, items)),
        );
    }
    Some(content)
}

fn list_item_blocks(
    blocks: &[Block],
    options: MarkdownOptions,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Vec<MappedText> {
    assembly::list_body(mapped_sequence(blocks, options, locations, track))
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
        item.contribution.before |= spacing.is_some_and(|rows| rows > 0);
        if item.contribution.rows {
            item.contribution.before |= pending.contribution.spacing();
            item.attach_navigation_text(std::mem::take(&mut pending), false);
            physical.push((item, spacing));
        } else {
            pending.append(item);
        }
    }
    if let Some((last, _)) = physical.last_mut() {
        last.contribution.after |= pending.contribution.spacing();
        physical
            .first_mut()
            .expect("a physical item")
            .0
            .attach_navigation_text(pending, true);
    } else {
        return pending.nonempty();
    }
    let mut items = physical.into_iter();
    let (mut output, _) = items.next()?;
    for (item, spacing_before_lines) in items {
        let blank_lines = spacing_before_lines
            .unwrap_or(u16::from(!compact))
            .max(u16::from(
                output.contribution.after || item.contribution.before,
            ));
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
        return nonempty(markers).map(|mut markers| {
            markers.contribution.rows = false;
            markers.syntax_site(BlockSyntax::Phrasing)
        });
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
        // Accepted data rows remain physical even when every cell is empty.
        // Their fence is structure, unlike a navigation-only row filtered above.
        body.contribution.rows = true;
        if !markers.is_empty() {
            body.insert(0, &format!("{markers}\n\n"));
        }
        body.syntax_site(if markers.is_empty() {
            BlockSyntax::Fence
        } else {
            BlockSyntax::Phrasing
        })
        .tail_grammar(BlockSyntax::Fence, false)
    })
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
