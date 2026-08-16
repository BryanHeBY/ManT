//! Maps canonical Markdown offsets to renderer-supplied semantic node ranges.

use mant_ir::SourceSpan;
use mant_protocol::{OutlineNodeReference, OutlineTrail};

use crate::output::{MarkdownArtifact, MarkdownNode, MarkdownNodeRange, MarkdownSection};

#[derive(Clone)]
pub(super) struct Owner {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) outline: OutlineTrail,
    pub(super) source: Option<SourceSpan>,
}

/// Offset index for manual sections, definition entries, and optional TLDR.
pub(super) struct OwnerIndex {
    sections: Vec<Owner>,
    entries: Vec<Owner>,
    entry_prefix_max_end: Vec<usize>,
    root: Option<Owner>,
    tldr: Option<Owner>,
}

impl OwnerIndex {
    pub(super) fn new(artifact: &MarkdownArtifact) -> Self {
        let mut sections = Vec::new();
        let mut entries = Vec::new();
        let mut root = None;
        let mut tldr = None;

        for mapped in &artifact.nodes {
            let owner = owner_from_range(mapped);
            match mapped.node {
                MarkdownNode::Tldr => tldr = Some(owner),
                MarkdownNode::DocumentRoot => root = Some(owner),
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
            sections,
            entries,
            entry_prefix_max_end,
            root,
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

fn owner_from_range(mapped: &MarkdownNodeRange) -> Owner {
    let range = mapped.range.clone();
    let (outline, source) = match &mapped.node {
        MarkdownNode::Tldr => (
            OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::Tldr {
                    path: "0".to_owned().into(),
                    id: "tldr".into(),
                    title: "TLDR QUICK REFERENCE".to_owned(),
                },
            },
            None,
        ),
        MarkdownNode::DocumentRoot => (
            OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::DocumentRoot {
                    path: "root".to_owned().into(),
                    id: mant_ir::DOCUMENT_ROOT_ID.into(),
                    title: "OVERVIEW".to_owned(),
                },
            },
            None,
        ),
        MarkdownNode::DocumentSection { section, source } => (
            OutlineTrail {
                ancestors: section.ancestors.clone(),
                node: OutlineNodeReference::DocumentSection {
                    path: section.path.to_string().into(),
                    id: section.id.clone(),
                    title: section.title.clone(),
                },
            },
            *source,
        ),
        MarkdownNode::DocumentEntry {
            path,
            id,
            title,
            role,
            case,
            names,
            section,
            source,
        } => (
            OutlineTrail {
                ancestors: entry_ancestors(section.as_ref()),
                node: OutlineNodeReference::DocumentEntry {
                    path: path.to_string().into(),
                    id: id.clone(),
                    title: title.clone(),
                    role: *role,
                    case: *case,
                    names: names.clone(),
                },
            },
            *source,
        ),
    };
    Owner {
        start: range.start,
        end: range.end,
        outline,
        source,
    }
}

fn entry_ancestors(section: Option<&MarkdownSection>) -> Vec<mant_protocol::OutlineReference> {
    section.map_or_else(
        || {
            vec![mant_protocol::OutlineReference {
                path: "root".to_owned().into(),
                id: mant_ir::DOCUMENT_ROOT_ID.into(),
                title: "OVERVIEW".to_owned(),
            }]
        },
        |section| {
            section
                .ancestors
                .iter()
                .cloned()
                .chain(std::iter::once(section_reference(section)))
                .collect()
        },
    )
}

fn section_reference(section: &MarkdownSection) -> mant_protocol::OutlineReference {
    mant_protocol::OutlineReference {
        path: section.path.to_string().into(),
        id: section.id.clone(),
        title: section.title.clone(),
    }
}
