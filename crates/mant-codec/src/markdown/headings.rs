//! Heading identity allocation, nesting and exact local-fragment remapping.
use mant_ir::{
    Block, Document, Heading, Inline, Section,
    visit::{self, VisitMut},
};
use pulldown_cmark::HeadingLevel;
use std::collections::{HashMap, HashSet};

pub(super) fn take_explicit_heading_id(children: &mut Vec<Inline>) -> Option<String> {
    let (id, empty) = {
        let Inline::Text { value } = children.last_mut()? else {
            return None;
        };
        let trimmed = value.trim_end();
        let opening = trimmed.rfind("{#")?;
        if !trimmed.ends_with('}') {
            return None;
        }
        if opening != 0
            && !trimmed[..opening]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            return None;
        }
        let id = trimmed
            .get(opening + 2..trimmed.len().checked_sub(1)?)?
            .to_owned();
        if id.is_empty()
            || id.bytes().any(|byte| {
                byte.is_ascii_whitespace() || matches!(byte, b'{' | b'}' | b'\\' | b'<' | b'>')
            })
        {
            return None;
        }
        let title_end = trimmed[..opening].trim_end().len();
        value.truncate(title_end);
        (id, value.is_empty())
    };
    if empty {
        children.pop();
    }
    Some(id)
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

#[derive(Default)]
pub(super) struct SectionIds {
    counts: HashMap<String, usize>,
    assigned: HashSet<String>,
    targets: HashMap<String, String>,
}

impl SectionIds {
    /// Reserve actual destinations, not authored aliases, during entry allocation.
    pub(super) fn reserved_targets(&self) -> HashSet<String> {
        self.targets.values().cloned().collect()
    }

    pub(super) fn retain_targets(&mut self, targets: impl IntoIterator<Item = String>) {
        for target in targets {
            self.targets.insert(target.clone(), target);
        }
    }

    /// Explicit entry metadata replaces only the old destination's mappings.
    pub(super) fn replace_target(&mut self, old: &str, new: String) {
        self.targets.retain(|_, target| target != old);
        self.targets.insert(new.clone(), new);
    }

    pub(super) fn resolve_links(&self, document: &mut Document) {
        LocalLinkResolver::new(&self.targets).visit_document_mut(document);
    }

    pub(super) fn allocate(&mut self, title: &str, explicit: Option<&str>) -> String {
        let explicit = explicit
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let normalized_explicit = explicit
            .as_deref()
            .map(crate::definitions::document_id_slug);
        let base = explicit
            .as_deref()
            .zip(normalized_explicit.as_deref())
            .filter(|(authored, normalized)| {
                *authored == *normalized
                    && !crate::producer_identity::is_reserved_selector(authored)
            })
            .map_or_else(|| slug(title), |(_, normalized)| normalized.to_owned());
        let base = if base.is_empty() {
            "section".to_owned()
        } else if crate::producer_identity::is_reserved_selector(&base) {
            // Reserved selectors and bare tree paths would shadow this
            // heading in excerpt selection; keep it addressable instead.
            format!("{base}-section")
        } else {
            base
        };
        // Disambiguate on the final id, not the per-base count: `# Foo 2`
        // slugs to base `foo-2`, which collides with the `foo-2` a second
        // `# Foo` produces. Counting per base alone would hand both the same
        // id, silently misattributing search ownership between them.
        let count = self.counts.entry(base.clone()).or_default();
        let id = loop {
            *count += 1;
            let candidate = if *count == 1 {
                base.clone()
            } else {
                format!("{base}-{}", *count)
            };
            if self.assigned.insert(candidate.clone()) {
                break candidate;
            }
        };
        // Ambiguous human-facing keys resolve to the first section that
        // claimed them, matching the bare slug this heading renders as its
        // anchor. A later duplicate owns only its own disambiguated id.
        self.targets
            .entry(base.clone())
            .or_insert_with(|| id.clone());
        // Heading attributes are source-level link aliases. Preserve the
        // original alias even when its final section ID had to move out of the
        // selector namespace (`{#root}`, `{#2.1}`, or `{#2.1/e3}`).
        if let Some(explicit) = explicit {
            self.targets.entry(explicit).or_insert_with(|| id.clone());
        }
        self.targets
            .entry(slug(title))
            .or_insert_with(|| id.clone());
        self.targets.insert(id.clone(), id.clone());
        id
    }

    pub(super) fn remap_target(&mut self, current: Option<&str>, replacement: Option<&str>) {
        let Some(current) = current else {
            return;
        };
        if let Some(replacement) = replacement {
            for target in self.targets.values_mut() {
                if target == current {
                    replacement.clone_into(target);
                }
            }
        } else {
            self.targets.retain(|_, target| target != current);
        }
    }
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

struct LocalLinkResolver<'targets> {
    targets: &'targets HashMap<String, String>,
}

impl<'targets> LocalLinkResolver<'targets> {
    fn new(targets: &'targets HashMap<String, String>) -> Self {
        Self { targets }
    }
}

impl VisitMut for LocalLinkResolver<'_> {
    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        if let Inline::Link {
            target: mant_ir::LinkTarget::Section { id },
            ..
        } = inline
        {
            // URI syntax and percent decoding were consumed by link_target.
            // The remaining fragment is an exact identity, not another URI
            // or a heading title to trim/slug into a different destination.
            if let Some(resolved) = self.targets.get(id.as_str()) {
                *id = resolved.as_str().into();
            }
        }
        visit::walk_inline_mut(self, inline);
    }
}
