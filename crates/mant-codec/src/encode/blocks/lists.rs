//! Ordinary list markers and shared item-boundary assembly.
use super::{assembly, mapped_sequence};
use crate::encode::math_markdown::RenderOptions;
use crate::encode::{
    MAX_MARKDOWN_ORDINAL, MarkdownInlineProjection,
    mapped::{BlockSyntax, MappedText},
};
use mant_ir::{Block, EntryOwner, ListItem, ListKind};

pub(super) fn render_list(
    kind: ListKind,
    compact: bool,
    items: &[ListItem],
    options: RenderOptions<'_>,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
    previous: BlockSyntax,
) -> Option<MappedText> {
    let bullet = format!("{} ", previous.following_list_marker('-', '+'));
    let period = previous.following_list_marker('.', ')');
    let prepared: Vec<_> = items
        .iter()
        .map(|item| render_list_item(item, options, locations, track))
        .collect();
    let explicit_ordinals = needs_explicit_ordinals(kind, &prepared);
    let rendered = items
        .iter()
        .zip(prepared)
        .enumerate()
        .filter_map(|(index, (item, mut content))| {
            if explicit_ordinals
                && content.contribution.rows
                && let Some(ordinal) = kind.ordinal(index)
            {
                content = with_explicit_ordinal(content, ordinal);
            }
            content.contribution.before |= mant_ir::geometry::list_item_spacing(
                item.layout.spacing_before_lines,
                index,
                compact,
            ) > 0;
            let content = content
                .with_owner(EntryOwner::List(item), track)
                .nonempty()?;
            if content.contribution.rows {
                let marker =
                    if let Some(ordinal) = kind.ordinal(index).filter(|_| !explicit_ordinals) {
                        // Only the opening marker determines the effective start.
                        let ordinal = if ordinal <= MAX_MARKDOWN_ORDINAL {
                            ordinal
                        } else {
                            1
                        };
                        format!("{ordinal}{period} ")
                    } else {
                        bullet.clone()
                    };
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
            &format!("{}\n", crate::encode::semantic::declaration(facts, items)),
        );
    }
    Some(content)
}

fn render_list_item(
    item: &ListItem,
    options: RenderOptions<'_>,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> MappedText {
    let mut blocks = list_item_blocks(&item.blocks, options, locations, track);
    if options.preserve_semantics
        && let Some(facts) = &item.entry
    {
        if let Some(head) = blocks.first_mut() {
            // Metadata is on its own syntax line. Adding a separating space
            // would change the visible head once the comment is consumed.
            if !head.text.ends_with('\n') {
                head.text.push('\n');
            }
            head.text
                .push_str(crate::encode::semantic::metadata(facts).trim_start_matches(' '));
        }
        if let Some(domain) = facts
            .value_domain
            .as_ref()
            .and_then(crate::encode::semantic::domain)
        {
            blocks.insert(1, domain.into());
        }
    }
    let mut content = assembly::join(blocks);
    if content.text.is_empty() {
        content = content.syntax_site(BlockSyntax::Phrasing);
    }
    if !options.preserve_semantics
        && options.preserve_anchors
        && let Some(facts) = &item.entry
        && let Some(anchor) = options.anchor(&facts.id)
    {
        // Navigation uses the original content grammar, including leading
        // fences. The generated ordinal label has no authored destination.
        content.attach_navigation(&anchor);
    }
    content
}

fn needs_explicit_ordinals(kind: ListKind, items: &[MappedText]) -> bool {
    let mut retained = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| item.contribution.rows.then_some(index));
    let Some(first) = retained.next() else {
        return false;
    };
    kind.ordinal(first).is_some_and(|start| {
        start > MAX_MARKDOWN_ORDINAL
            // After the first marker, CommonMark always counts consecutively.
            // A missing interior owner therefore needs visible explicit labels.
            || retained.zip(first + 1..).any(|(actual, expected)| actual != expected)
    })
}

fn with_explicit_ordinal(mut content: MappedText, ordinal: u64) -> MappedText {
    let label = format!("{ordinal}\\.");
    if content.syntax == BlockSyntax::Phrasing && !content.hard_rows {
        content.insert(0, &format!("{label} "));
        content
    } else {
        // Wide numbers are visible labels. Keep a child's actual grammar
        // and indentation independent of the size of that label.
        let label = MappedText::from(label).syntax_site(BlockSyntax::Phrasing);
        assembly::join([label, content])
    }
}

fn list_item_blocks(
    blocks: &[Block],
    options: RenderOptions<'_>,
    locations: Option<&dyn MarkdownInlineProjection>,
    track: bool,
) -> Vec<MappedText> {
    assembly::list_body(mapped_sequence(blocks, options, locations, track))
}

/// Preserve a man(7) `.PD` override when one is present, otherwise fall back
/// to the list-wide compactness used by mdoc(7) and HTML inputs.
pub(super) fn join_definition_items(
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
        assembly::append_item(&mut output, item, blank_lines);
    }
    Some(output)
}
