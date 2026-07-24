//! Resolves canonical Markdown offsets back to semantic manual nodes.
//!
//! Search itself operates on a visible-text projection.  This index is the
//! complementary source map: it assigns each canonical Markdown offset to the
//! most specific enclosing section or definition entry.

use mant_ast::{
    Block, DefinitionItem, QueryBundle, SearchNode, SearchSectionReference, Section, SourceSpan,
};

use crate::output::html_anchor;

#[derive(Clone)]
pub(super) struct Owner {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) node: SearchNode,
    pub(super) section: Option<SearchSectionReference>,
    pub(super) source: Option<SourceSpan>,
}

/// Offset index for manual sections, definition entries, and optional TLDR.
pub(super) struct OwnerIndex {
    sections: Vec<Owner>,
    entries: Vec<Owner>,
    entry_prefix_max_end: Vec<usize>,
    tldr: Option<Owner>,
}

impl OwnerIndex {
    pub(super) fn new(query: &QueryBundle, markdown: &str) -> Self {
        let mut sections = Vec::new();
        let mut entries = Vec::new();
        if let Some(manual) = &query.document {
            collect_section_owners(&manual.sections, &[], markdown, &mut sections, &mut entries);
            sections.sort_by_key(|owner| owner.start);
            for index in 0..sections.len() {
                sections[index].end = sections
                    .get(index + 1)
                    .map_or(markdown.len(), |next| next.start);
            }
            for entry in &mut entries {
                if let Some(section_end) = entry.section.as_ref().and_then(|reference| {
                    sections
                        .iter()
                        .find(|section| {
                            section.section.as_ref().map(|value| value.id.as_str())
                                == Some(reference.id.as_str())
                        })
                        .map(|section| section.end)
                }) {
                    entry.end = entry.end.min(section_end);
                }
            }
            entries.sort_by_key(|owner| owner.start);
        }
        let manual_start = sections.first().map_or(markdown.len(), |owner| owner.start);
        let tldr = query.tldr.as_ref().and_then(|_| {
            markdown.find("## TLDR").map(|start| Owner {
                start,
                end: manual_start,
                node: SearchNode::Tldr {
                    path: "0".to_owned(),
                    id: "tldr".to_owned(),
                    title: "TLDR QUICK REFERENCE".to_owned(),
                },
                section: None,
                source: None,
            })
        });
        let mut maximum_end = 0;
        let entry_prefix_max_end = entries
            .iter()
            .map(|entry| {
                maximum_end = maximum_end.max(entry.end);
                maximum_end
            })
            .collect();
        Self {
            sections,
            entries,
            entry_prefix_max_end,
            tldr,
        }
    }

    pub(super) fn owner(&self, offset: usize) -> Option<&Owner> {
        if let Some(entry) = self.entry_owner(offset) {
            return Some(entry);
        }
        let section_index = self.sections.partition_point(|owner| owner.start <= offset);
        if let Some(section) = section_index
            .checked_sub(1)
            .and_then(|index| self.sections.get(index))
            .filter(|owner| offset < owner.end)
        {
            return Some(section);
        }
        self.tldr
            .as_ref()
            .filter(|owner| owner.start <= offset && offset < owner.end)
    }

    fn entry_owner(&self, offset: usize) -> Option<&Owner> {
        let mut index = self.entries.partition_point(|owner| owner.start <= offset);
        while let Some(candidate_index) = index.checked_sub(1) {
            let candidate = &self.entries[candidate_index];
            if offset < candidate.end {
                return Some(candidate);
            }
            if candidate_index == 0 || self.entry_prefix_max_end[candidate_index - 1] <= offset {
                break;
            }
            index = candidate_index;
        }
        None
    }
}

fn collect_section_owners(
    sections: &[Section],
    parent: &[usize],
    markdown: &str,
    section_owners: &mut Vec<Owner>,
    entry_owners: &mut Vec<Owner>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent.to_vec();
        coordinates.push(index + 1);
        let path = format_path(&coordinates);
        let anchor = html_anchor(&section.id);
        let Some(start) = markdown.find(&anchor) else {
            continue;
        };
        let section_reference = SearchSectionReference {
            path: path.clone(),
            id: section.id.clone(),
            title: section.title.clone(),
        };
        section_owners.push(Owner {
            start,
            end: markdown.len(),
            node: SearchNode::DocumentSection {
                path: path.clone(),
                id: section.id.clone(),
                title: section.title.clone(),
            },
            section: Some(section_reference.clone()),
            source: section.source,
        });

        let mut entries = Vec::new();
        collect_entries(&section.blocks, &mut entries);
        for (entry_index, (entry, source)) in entries.into_iter().enumerate() {
            let Some(identity) = &entry.identity else {
                continue;
            };
            let entry_anchor = html_anchor(&identity.id);
            let Some(entry_start) = markdown[start..]
                .find(&entry_anchor)
                .map(|relative| start + relative)
            else {
                continue;
            };
            entry_owners.push(Owner {
                start: entry_start,
                end: definition_item_end(markdown, entry_start),
                node: SearchNode::DocumentEntry {
                    path: format!("{path}/o{}", entry_index + 1),
                    id: identity.id.clone(),
                    title: identity.names.join(", "),
                    role: identity.role,
                    names: identity.names.clone(),
                },
                section: Some(section_reference.clone()),
                source,
            });
        }
        collect_section_owners(
            &section.children,
            &coordinates,
            markdown,
            section_owners,
            entry_owners,
        );
    }
}

fn collect_entries<'a>(
    blocks: &'a [Block],
    output: &mut Vec<(&'a DefinitionItem, Option<SourceSpan>)>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_entries(&item.blocks, output);
                }
            }
            Block::DefinitionList { items, source, .. } => {
                for item in items {
                    if item.identity.is_some() {
                        output.push((item, *source));
                    }
                    collect_entries(&item.description, output);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_entries(&cell.blocks, output);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn definition_item_end(markdown: &str, anchor_start: usize) -> usize {
    let line_start = markdown[..anchor_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let prefix = &markdown[line_start..anchor_start];
    if prefix.is_empty() {
        return markdown.len();
    }
    let content_indent = prefix.chars().count();
    let mut cursor = markdown[anchor_start..]
        .find('\n')
        .map_or(markdown.len(), |relative| anchor_start + relative + 1);
    let mut after_blank = false;

    while cursor < markdown.len() {
        let end = markdown[cursor..]
            .find('\n')
            .map_or(markdown.len(), |relative| cursor + relative);
        let line = &markdown[cursor..end];
        if line.starts_with(prefix) {
            return cursor;
        }
        if line.trim().is_empty() {
            after_blank = true;
        } else {
            let indent = line
                .chars()
                .take_while(|character| *character == ' ')
                .count();
            if after_blank && indent < content_indent {
                return cursor;
            }
            after_blank = false;
        }
        cursor = end.saturating_add(1);
    }
    markdown.len()
}

fn format_path(coordinates: &[usize]) -> String {
    coordinates
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}
