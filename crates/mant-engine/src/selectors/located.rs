//! Borrowed semantic locations and source-order breadcrumbs, without DTOs.
use super::DOCUMENT_ROOT_TITLE;
use crate::definitions::{ContentEntry, content_entries, content_entry_locations};
use mant_ir::{
    Block, DOCUMENT_ROOT_ID, EntryFacts, EntryOwner, NodeId, OutlinePath, Section, SourceSpan,
};
#[derive(Clone)]
pub(crate) struct LocatedBreadcrumb {
    pub(crate) path: OutlinePath,
    pub(crate) id: NodeId,
    pub(crate) title: String,
}

pub(crate) enum LocatedNode<'a> {
    Section {
        order: usize,
        coordinates: Vec<usize>,
        path: OutlinePath,
        breadcrumbs: Vec<LocatedBreadcrumb>,
        section: &'a Section,
    },
    Entry {
        order: usize,
        coordinates: Vec<usize>,
        path: OutlinePath,
        title: String,
        breadcrumbs: Vec<LocatedBreadcrumb>,
        entry: Box<ContentEntry<'a>>,
        source: Option<SourceSpan>,
    },
}

impl LocatedNode<'_> {
    pub(crate) fn order(&self) -> usize {
        match self {
            Self::Section { order, .. } | Self::Entry { order, .. } => *order,
        }
    }

    pub(crate) fn coordinates(&self) -> &[usize] {
        match self {
            Self::Section { coordinates, .. } | Self::Entry { coordinates, .. } => coordinates,
        }
    }

    pub(crate) fn path(&self) -> &OutlinePath {
        match self {
            Self::Section { path, .. } | Self::Entry { path, .. } => path,
        }
    }

    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Section { section, .. } => &section.id,
            Self::Entry { entry, .. } => {
                &entry
                    .owner()
                    .facts()
                    .expect("located entries have identities")
                    .id
            }
        }
    }

    pub(crate) fn facts(&self) -> Option<&EntryFacts> {
        match self {
            Self::Entry { entry, .. } => entry.owner().facts(),
            Self::Section { .. } => None,
        }
    }

    pub(crate) fn source(&self) -> Option<SourceSpan> {
        match self {
            Self::Entry { source, .. } => *source,
            Self::Section { section, .. } => section.source,
        }
    }

    pub(crate) const fn is_section(&self) -> bool {
        matches!(self, Self::Section { .. })
    }
}

pub(crate) fn collect_sections<'a>(
    sections: &'a [Section],
    parent_coordinates: &[usize],
    breadcrumbs: &[LocatedBreadcrumb],
    output: &mut Vec<LocatedNode<'a>>,
) {
    collect_sections_impl::<true>(sections, parent_coordinates, breadcrumbs, output);
}

pub(crate) fn collect_selection_sections<'a>(
    sections: &'a [Section],
    output: &mut Vec<LocatedNode<'a>>,
) {
    collect_sections_impl::<false>(sections, &[], &[], output);
}

fn collect_sections_impl<'a, const DETAILS: bool>(
    sections: &'a [Section],
    parent_coordinates: &[usize],
    breadcrumbs: &[LocatedBreadcrumb],
    output: &mut Vec<LocatedNode<'a>>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent_coordinates.to_vec();
        coordinates.push(index + 1);
        let path =
            OutlinePath::section(&coordinates).expect("enumerated section paths are one-based");
        let order = output.len();
        output.push(LocatedNode::Section {
            order,
            coordinates: coordinates.clone(),
            path: path.clone(),
            breadcrumbs: breadcrumbs.to_vec(),
            section,
        });
        let mut child_breadcrumbs = breadcrumbs.to_vec();
        if DETAILS {
            child_breadcrumbs.push(LocatedBreadcrumb {
                path: path.clone(),
                id: section.id.clone(),
                title: section.heading.plain_text(),
            });
        }
        for located in if DETAILS {
            content_entries(&section.blocks)
        } else {
            content_entry_locations(&section.blocks)
        } {
            let entry = located.owner();
            let mut entry_breadcrumbs = child_breadcrumbs.clone();
            if DETAILS {
                append_entry_breadcrumbs(
                    &mut entry_breadcrumbs,
                    Some(&coordinates),
                    located.indices(),
                    located.ancestors(),
                );
            }
            output.push(LocatedNode::Entry {
                order: output.len(),
                coordinates: coordinates.clone(),
                path: OutlinePath::nested_entry(Some(&coordinates), located.indices())
                    .expect("enumerated entry paths are one-based"),
                title: if DETAILS {
                    definition_title(entry, located.names())
                } else {
                    String::new()
                },
                breadcrumbs: entry_breadcrumbs,
                source: located.source(),
                entry: Box::new(located),
            });
        }
        collect_sections_impl::<DETAILS>(
            &section.children,
            &coordinates,
            &child_breadcrumbs,
            output,
        );
    }
}

pub(crate) fn collect_root_entries<'a>(blocks: &'a [Block], output: &mut Vec<LocatedNode<'a>>) {
    collect_root_entries_impl::<true>(blocks, output);
}

pub(crate) fn collect_selection_root_entries<'a>(
    blocks: &'a [Block],
    output: &mut Vec<LocatedNode<'a>>,
) {
    collect_root_entries_impl::<false>(blocks, output);
}

fn collect_root_entries_impl<'a, const DETAILS: bool>(
    blocks: &'a [Block],
    output: &mut Vec<LocatedNode<'a>>,
) {
    let breadcrumbs = if DETAILS {
        vec![LocatedBreadcrumb {
            path: OutlinePath::DocumentRoot,
            id: DOCUMENT_ROOT_ID.into(),
            title: DOCUMENT_ROOT_TITLE.to_owned(),
        }]
    } else {
        Vec::new()
    };
    for located in if DETAILS {
        content_entries(blocks)
    } else {
        content_entry_locations(blocks)
    } {
        let entry = located.owner();
        let mut entry_breadcrumbs = breadcrumbs.clone();
        if DETAILS {
            append_entry_breadcrumbs(
                &mut entry_breadcrumbs,
                None,
                located.indices(),
                located.ancestors(),
            );
        }
        output.push(LocatedNode::Entry {
            order: output.len(),
            coordinates: Vec::new(),
            path: OutlinePath::nested_entry(None, located.indices())
                .expect("enumerated entry paths are one-based"),
            title: if DETAILS {
                definition_title(entry, located.names())
            } else {
                String::new()
            },
            breadcrumbs: entry_breadcrumbs,
            source: located.source(),
            entry: Box::new(located),
        });
    }
}

fn append_entry_breadcrumbs(
    breadcrumbs: &mut Vec<LocatedBreadcrumb>,
    section: Option<&[usize]>,
    indices: &[usize],
    ancestors: &[EntryOwner<'_>],
) {
    for (depth, ancestor) in ancestors.iter().enumerate() {
        let path = OutlinePath::nested_entry(section, &indices[..=depth])
            .expect("ancestor entry paths are one-based");
        let identity = ancestor
            .facts()
            .expect("semantic entry ancestors have identities");
        breadcrumbs.push(LocatedBreadcrumb {
            path: path.clone(),
            id: identity.id.clone(),
            title: definition_title(*ancestor, ancestor.validated_names().unwrap_or_default()),
        });
    }
}

fn definition_title(entry: EntryOwner<'_>, names: &[String]) -> String {
    crate::entry_presentation::owner_label(entry, names, mant_protocol::EntryLabelMode::Compact)
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn address_only_locations_never_materialize_names_titles_or_breadcrumbs() {
        let query = crate::query_markdown_text("# Heading\n\n<!-- mant:entries role=command case=sensitive -->\n- `root-command`: Root.\n\n## Section heading\n\n<!-- mant:entries role=command case=sensitive -->\n- `nested-command`: Child.\n", None).unwrap();
        let document = query.document.unwrap();
        let mut located = Vec::new();
        collect_selection_root_entries(&document.blocks, &mut located);
        collect_selection_sections(&document.sections, &mut located);
        assert_eq!(located.len(), 3);
        for node in located {
            match node {
                LocatedNode::Section { breadcrumbs, .. } => assert!(breadcrumbs.is_empty()),
                LocatedNode::Entry {
                    title,
                    breadcrumbs,
                    entry,
                    ..
                } => {
                    assert!(title.is_empty());
                    assert!(breadcrumbs.is_empty());
                    assert!(entry.names().is_empty());
                    assert!(!entry.block_path().is_empty());
                    assert!(entry.owner().facts().is_some());
                }
            }
        }
    }
}
