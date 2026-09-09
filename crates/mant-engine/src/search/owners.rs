//! Maps canonical Markdown offsets to renderer-supplied semantic node ranges.

use mant_ir::SourceSpan;
use mant_protocol::{OutlineNodeReference, OutlineTrail};

use mant_codec::encode::{MarkdownArtifact, MarkdownNode, MarkdownNodeRange, MarkdownSection};

#[derive(Clone, Copy)]
pub(super) struct Owner {
    pub(super) key: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) source: Option<SourceSpan>,
}

/// Offset index for manual sections, definition entries, and optional TLDR.
pub(super) struct OwnerIndex<'map, 'src> {
    artifact: &'map MarkdownArtifact<'src>,
    #[cfg(test)]
    materialized: std::cell::Cell<usize>,
    sections: Vec<Owner>,
    entries: Vec<Owner>,
    entry_prefix_max_end: Vec<usize>,
    root: Option<Owner>,
    heading: Option<Owner>,
    tldr: Option<Owner>,
}

impl<'map, 'src> OwnerIndex<'map, 'src> {
    pub(super) fn new(artifact: &'map MarkdownArtifact<'src>) -> Self {
        let mut sections = Vec::new();
        let mut entries = Vec::new();
        let mut root = None;
        let mut heading = None;
        let mut tldr = None;

        for (key, mapped) in artifact.nodes().iter().enumerate() {
            let owner = owner_from_range(key, mapped);
            match mapped.node() {
                MarkdownNode::Tldr => tldr = Some(owner),
                MarkdownNode::DocumentRoot => root = Some(owner),
                MarkdownNode::DocumentHeading { .. } => heading = Some(owner),
                MarkdownNode::DocumentSection { .. } => sections.push(owner),
                MarkdownNode::DocumentEntry { .. } => entries.push(owner),
            }
        }

        sections.sort_by_key(|owner| owner.start);
        entries.sort_by_key(|owner| owner.start);
        let mut maximum_end = 0;
        let entry_prefix_max_end = entries
            .iter()
            .map(|entry| {
                maximum_end = maximum_end.max(entry.end);
                maximum_end
            })
            .collect();
        Self {
            artifact,
            #[cfg(test)]
            materialized: std::cell::Cell::new(0),
            sections,
            entries,
            entry_prefix_max_end,
            root,
            heading,
            tldr,
        }
    }

    /// Materialize presentation only after global search paging selects a group.
    pub(super) fn trail(&self, key: usize) -> OutlineTrail {
        #[cfg(test)]
        self.materialized.set(self.materialized.get() + 1);
        trail(self.artifact, self.artifact.nodes()[key].node())
    }

    pub(super) fn owner(&self, offset: usize) -> Option<&Owner> {
        if let Some(heading) = self
            .heading
            .as_ref()
            .filter(|owner| owner.start <= offset && offset < owner.end)
        {
            return Some(heading);
        }
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
        if let Some(root) = self
            .root
            .as_ref()
            .filter(|owner| owner.start <= offset && offset < owner.end)
        {
            return Some(root);
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

fn owner_from_range(key: usize, mapped: &MarkdownNodeRange<'_>) -> Owner {
    let source = match &mapped.node() {
        MarkdownNode::Tldr | MarkdownNode::DocumentRoot => None,
        MarkdownNode::DocumentHeading { source }
        | MarkdownNode::DocumentSection { source, .. }
        | MarkdownNode::DocumentEntry { source, .. } => *source,
    };
    Owner {
        key,
        start: mapped.range().start,
        end: mapped.range().end,
        source,
    }
}

fn trail(artifact: &MarkdownArtifact<'_>, node: &MarkdownNode<'_>) -> OutlineTrail {
    match node {
        MarkdownNode::Tldr => OutlineTrail {
            ancestors: Vec::new(),
            node: OutlineNodeReference::Tldr {
                path: "0".into(),
                id: "tldr".into(),
                title: "TLDR QUICK REFERENCE".into(),
            },
        },
        MarkdownNode::DocumentRoot | MarkdownNode::DocumentHeading { .. } => OutlineTrail {
            ancestors: Vec::new(),
            node: OutlineNodeReference::DocumentRoot {
                path: "root".into(),
                id: mant_ir::DOCUMENT_ROOT_ID.into(),
                title: "OVERVIEW".into(),
            },
        },
        MarkdownNode::DocumentSection { section, .. } => {
            let section = artifact
                .section(*section)
                .expect("artifact-owned section slot");
            OutlineTrail {
                ancestors: section_ancestors(artifact, section.parent()),
                node: OutlineNodeReference::DocumentSection {
                    path: section.path().to_string().into(),
                    id: section.section().id.clone(),
                    title: section.section().heading.plain_text(),
                },
            }
        }
        MarkdownNode::DocumentEntry {
            path,
            owner,
            names,
            section,
            ..
        } => {
            let facts = owner.facts().expect("mapped semantic owner");
            let ancestors = if section.is_some() {
                section_ancestors(artifact, *section)
            } else {
                vec![mant_protocol::OutlineReference {
                    path: "root".into(),
                    id: mant_ir::DOCUMENT_ROOT_ID.into(),
                    title: "OVERVIEW".into(),
                }]
            };
            OutlineTrail {
                ancestors,
                node: OutlineNodeReference::DocumentEntry {
                    path: path.to_string().into(),
                    id: facts.id.clone(),
                    title: crate::entry_presentation::owner_label(
                        *owner,
                        names,
                        mant_protocol::EntryLabelMode::Compact,
                    ),
                    entry_kind: facts.kind,
                    case: facts.case,
                    names: names.to_vec(),
                },
            }
        }
    }
}

fn section_ancestors(
    artifact: &MarkdownArtifact<'_>,
    mut slot: Option<usize>,
) -> Vec<mant_protocol::OutlineReference> {
    let mut ancestors = Vec::new();
    while let Some(current) = slot {
        let section = artifact
            .section(current)
            .expect("artifact-owned parent slot");
        ancestors.push(section_reference(section));
        slot = section.parent();
    }
    ancestors.reverse();
    ancestors
}

fn section_reference(section: &MarkdownSection<'_>) -> mant_protocol::OutlineReference {
    mant_protocol::OutlineReference {
        path: section.path().to_string().into(),
        id: section.section().id.clone(),
        title: section.section().heading.plain_text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanning_a_large_owner_index_does_not_materialize_trails() {
        use std::fmt::Write;
        let mut source = "# Tool\n\n## Parent\n\n### Child\n\n<!-- mant:entries role=option case=sensitive -->\n".to_owned();
        for index in 0..1000 {
            writeln!(source, "- `--flag-{index}`: Payload{index}.").unwrap();
        }
        let query = crate::query_markdown_text(&source, None).unwrap();
        let artifact = mant_codec::encode::render_addressable_markdown(&query);
        let index = OwnerIndex::new(&artifact);
        assert_eq!(index.entries.len(), 1000);
        assert_eq!(index.materialized.get(), 0);
        for entry in &index.entries {
            assert_eq!(index.owner(entry.start).unwrap().key, entry.key);
        }
        assert_eq!(index.materialized.get(), 0);
        for entry in index.entries.iter().skip(998) {
            let trail = index.trail(entry.key);
            assert_eq!(trail.ancestors.len(), 2);
            assert_eq!(trail.ancestors[0].title, "Parent");
            assert_eq!(trail.ancestors[1].title, "Child");
        }
        assert_eq!(index.materialized.get(), 2);
    }
}
