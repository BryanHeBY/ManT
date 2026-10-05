//! Ordinary list markers and shared item-boundary assembly.
use super::{assembly, mapped_sequence};
use crate::encode::{
    MarkdownInlineProjection, MarkdownOptions,
    inline::html_anchor,
    mapped::{BlockSyntax, MappedText},
};
use mant_ir::{Block, EntryOwner, ListItem, ListKind};

pub(super) fn render_list(
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
                    head.text
                        .push_str(&crate::encode::semantic::metadata(facts));
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
            &format!("{}\n", crate::encode::semantic::declaration(facts, items)),
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
