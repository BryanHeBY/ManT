//! Heading identity allocation, nesting and exact local-fragment remapping.
use crate::producer_identity::HeadingIds;
use mant_ir::{
    Block, Document, DocumentIndex, FragmentAlias, Heading, Inline, Section,
    visit::{self, VisitMut},
};
use pulldown_cmark::{Event, HeadingLevel};
use std::collections::HashSet;

fn heading_slug(heading: &Heading) -> String {
    slug(&super::inline::inline_text(&heading.content))
}

/// Remove a source attribute without interpreting escaped or decoded text as
/// marker syntax. The attribute itself can span several inline events (for
/// example, the underscores of `{#_foo_}`), but cannot cross a title wrapper.
pub(super) fn take_explicit_heading_id(
    cursor: &mut super::EventCursor<'_>,
    source: &super::source::MarkdownSource<'_>,
    heading_start: usize,
) -> Option<String> {
    let inlines = cursor.heading_inlines();
    let (Event::Text(_), last) = inlines.last()? else {
        return None;
    };
    let raw = source.raw(&(heading_start..last.end));
    let (opening, id) = crate::heading_attribute::trailing_id(raw)?;
    let title = raw[..opening]
        .trim_end_matches(|ch: char| ch.is_whitespace() && !matches!(ch, '\r' | '\n'));
    let cut = heading_start + title.len();
    let first = inlines.iter().position(|(_, range)| range.end > cut)?;
    let (event, range) = &inlines[first];
    let prefix = if range.start < cut {
        let Event::Text(value) = event else {
            return None;
        };
        if value.as_ref() != source.raw(range) {
            return None;
        }
        source.raw(&(range.start..cut))
    } else {
        ""
    };
    // A soft source wrap before the attribute is only a separator. A hard
    // break is still part of the title.
    let prefix_start = range.start;
    let first =
        if prefix.is_empty() && first > 0 && matches!(inlines[first - 1].0, Event::SoftBreak) {
            first - 1
        } else {
            first
        };
    for (event, range) in &mut inlines[first..] {
        *event = Event::Text("".into());
        range.end = range.start;
    }
    inlines[first].0 = Event::Text(prefix.to_owned().into());
    if !prefix.is_empty() {
        inlines[first].1 = prefix_start..cut;
    }
    Some(id.to_owned())
}

pub(super) fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Move a leading H1 into the real document-heading root without duplicating it.
pub(super) fn extract_document_title(
    root_blocks: &mut Vec<Block>,
    sections: &mut Vec<Section>,
    document_title_id: Option<&str>,
) -> Option<(Heading, Vec<mant_ir::FragmentAlias>)> {
    let document_title_id = document_title_id?;
    if sections.first().map(|section| section.id.as_str()) != Some(document_title_id) {
        return None;
    }
    let title = sections.remove(0);
    root_blocks.extend(title.blocks);
    sections.splice(0..0, title.children);
    let mut aliases = title.fragment_aliases;
    if !aliases
        .iter()
        .any(|alias| alias.as_str() == title.id.as_str())
    {
        aliases.push(title.id.to_string().into());
    }
    Some((title.heading, aliases))
}

pub(super) struct FlatSection {
    pub(super) level: u8,
    pub(super) is_document_title: bool,
    pub(super) section: Section,
}

pub(super) fn nest_sections(flat: Vec<FlatSection>) -> Vec<Section> {
    let mut roots = Vec::new();
    let mut stack: Vec<FlatSection> = Vec::new();

    for next in flat {
        while stack
            .last()
            .is_some_and(|current| current.is_document_title || current.level >= next.level)
        {
            attach_completed(&mut stack, &mut roots);
        }
        stack.push(next);
    }
    while !stack.is_empty() {
        attach_completed(&mut stack, &mut roots);
    }
    roots
}

fn attach_completed(stack: &mut Vec<FlatSection>, roots: &mut Vec<Section>) {
    let completed = stack.pop().expect("caller checks non-empty stack").section;
    if let Some(parent) = stack.last_mut() {
        parent.section.children.push(completed);
    } else {
        roots.push(completed);
    }
}

pub(super) fn assign_section_ids(sections: &mut [FlatSection]) -> Option<String> {
    let mut ids = HeadingIds::default();
    // Claim all the original slug spellings first. A shortened candidate
    // must not take a later heading's already valid full identity.
    let plans = sections
        .iter()
        .map(|flat| {
            let authored = flat
                .section
                .fragment_aliases
                .first()
                .map(FragmentAlias::as_str);
            let base = authored
                .filter(|id| mant_ir::is_normalized_node_id(id))
                .filter(|id| !crate::producer_identity::is_reserved_selector(id))
                .map_or_else(|| heading_slug(&flat.section.heading), str::to_owned);
            let base = if base.is_empty() {
                "section".to_owned()
            } else if crate::producer_identity::is_reserved_selector(&base) {
                format!("{base}-section")
            } else {
                base
            };
            let full = ids.claim_full(&base);
            (base, full)
        })
        .collect::<Vec<_>>();

    let mut document_title_id = None;
    for (flat, (base, full)) in sections.iter_mut().zip(plans) {
        let (id, long_fragment) = ids.complete(&base, full);
        if let Some(fragment) = long_fragment
            && !flat.section.fragment_aliases.contains(&fragment)
        {
            flat.section.fragment_aliases.push(fragment);
        }
        if flat.is_document_title {
            document_title_id = Some(id.clone());
        }
        flat.section.id = id.into();
    }
    document_title_id
}

pub(super) fn reserved_section_ids(sections: &[Section]) -> HashSet<String> {
    fn collect(sections: &[Section], ids: &mut HashSet<String>) {
        for section in sections {
            ids.insert(section.id.to_string());
            collect(&section.children, ids);
        }
    }
    let mut ids = HashSet::new();
    collect(sections, &mut ids);
    ids
}

pub(super) fn resolve_local_links(document: &mut Document) {
    fn publish_fallback(
        heading: &Heading,
        aliases: &mut Vec<FragmentAlias>,
        claimed: &DocumentIndex,
    ) -> bool {
        // Canonical IDs and authored or retained fragments take precedence.
        // Among unclaimed slugs, publish every heading claim so duplicate
        // titles with distinct explicit IDs stay ambiguous.
        let fragment = heading_slug(heading);
        if fragment.is_empty()
            || claimed.contains(&fragment)
            || claimed.fragment_target(&fragment).is_some()
            || claimed.is_fragment_ambiguous(&fragment)
        {
            return false;
        }
        aliases.push(fragment.into());
        true
    }

    fn section_fallbacks(sections: &mut [Section], index: &DocumentIndex) -> bool {
        let mut changed = false;
        for section in sections {
            changed |= publish_fallback(&section.heading, &mut section.fragment_aliases, index);
            changed |= section_fallbacks(&mut section.children, index);
        }
        changed
    }

    let mut index = DocumentIndex::build(document);
    let mut changed = false;
    if let Some(heading) = &document.heading {
        changed |= publish_fallback(heading, &mut document.fragment_aliases, &index);
    }
    changed |= section_fallbacks(&mut document.sections, &index);
    if changed {
        index = DocumentIndex::build(document);
    }
    LocalLinkResolver(&index).visit_document_mut(document);
}

fn slug(value: &str) -> String {
    let mut output = String::new();
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' {
            if separator && !output.is_empty() {
                output.push('-');
            }
            separator = false;
            output.push(character);
        } else {
            separator = true;
        }
    }
    output.trim_matches('-').to_owned()
}

struct LocalLinkResolver<'a>(&'a DocumentIndex);

impl VisitMut for LocalLinkResolver<'_> {
    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        if let Inline::Link {
            target: mant_ir::LinkTarget::Section { id },
            ..
        } = inline
            && let Some(target) = self.0.fragment_target(id.as_str())
            && self.0.fragment_target(target.as_str()) == Some(target)
        {
            // Keep a raw fragment when canonicalizing would lose ambiguity
            // or the only unambiguous spelling of a target.
            id.clone_from(target);
        }
        visit::walk_inline_mut(self, inline);
    }
}
