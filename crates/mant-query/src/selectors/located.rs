//! Borrowed semantic locations and source-order breadcrumbs, without DTOs.
use super::DOCUMENT_ROOT_TITLE;
use mant_ir::{
    Block, ContentContext, ContentEntry, DOCUMENT_ROOT_ID, EntryFacts, EntryOwner, NodeId,
    OutlinePath, Section, SourceSpan, content_entries, content_entry_locations,
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
        title: String,
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
    content: ContentContext<'a>,
    sections: &'a [Section],
    parent_coordinates: &[usize],
    breadcrumbs: &[LocatedBreadcrumb],
    output: &mut Vec<LocatedNode<'a>>,
) {
    collect_sections_impl::<true>(
        Some(content),
        sections,
        parent_coordinates,
        breadcrumbs,
        output,
    );
}

pub(crate) fn collect_selection_sections<'a>(
    sections: &'a [Section],
    output: &mut Vec<LocatedNode<'a>>,
) {
    collect_sections_impl::<false>(None, sections, &[], &[], output);
}

fn collect_sections_impl<'a, const DETAILS: bool>(
    content: Option<ContentContext<'a>>,
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
            title: content.map_or_else(String::new, |content| {
                content
                    .heading_single_line_text(&section.heading)
                    .expect("document heading resolves in its own content store")
            }),
            section,
        });
        let mut child_breadcrumbs = breadcrumbs.to_vec();
        if DETAILS {
            child_breadcrumbs.push(LocatedBreadcrumb {
                path: path.clone(),
                id: section.id.clone(),
                title: content
                    .expect("detailed collection has document content")
                    .heading_single_line_text(&section.heading)
                    .expect("document heading resolves in its own content store"),
            });
        }
        for located in if DETAILS {
            content_entries(
                content.expect("detailed collection has document content"),
                &section.blocks,
            )
        } else {
            content_entry_locations(&section.blocks)
        } {
            let entry = located.owner();
            let mut entry_breadcrumbs = child_breadcrumbs.clone();
            if DETAILS {
                append_entry_breadcrumbs(
                    content.expect("detailed collection has document content"),
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
                    definition_title(
                        content.expect("detailed collection has document content"),
                        entry,
                        located.names(),
                    )
                } else {
                    String::new()
                },
                breadcrumbs: entry_breadcrumbs,
                source: located.source(),
                entry: Box::new(located),
            });
        }
        collect_sections_impl::<DETAILS>(
            content,
            &section.children,
            &coordinates,
            &child_breadcrumbs,
            output,
        );
    }
}

pub(crate) fn collect_root_entries<'a>(
    content: ContentContext<'a>,
    blocks: &'a [Block],
    output: &mut Vec<LocatedNode<'a>>,
) {
    collect_root_entries_impl::<true>(Some(content), blocks, output);
}

pub(crate) fn collect_selection_root_entries<'a>(
    blocks: &'a [Block],
    output: &mut Vec<LocatedNode<'a>>,
) {
    collect_root_entries_impl::<false>(None, blocks, output);
}

fn collect_root_entries_impl<'a, const DETAILS: bool>(
    content: Option<ContentContext<'a>>,
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
        content_entries(
            content.expect("detailed collection has document content"),
            blocks,
        )
    } else {
        content_entry_locations(blocks)
    } {
        let entry = located.owner();
        let mut entry_breadcrumbs = breadcrumbs.clone();
        if DETAILS {
            append_entry_breadcrumbs(
                content.expect("detailed collection has document content"),
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
                definition_title(
                    content.expect("detailed collection has document content"),
                    entry,
                    located.names(),
                )
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
    content: ContentContext<'_>,
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
            title: definition_title(
                content,
                *ancestor,
                content
                    .entry_validated_names(*ancestor)
                    .expect("document entry names resolve in their own content store")
                    .unwrap_or_default(),
            ),
        });
    }
}

fn definition_title(
    content: ContentContext<'_>,
    entry: EntryOwner<'_>,
    names: &[String],
) -> String {
    crate::entry_presentation::owner_label(
        content,
        entry,
        names,
        mant_protocol::EntryLabelMode::Compact,
    )
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn address_only_locations_never_materialize_names_titles_or_breadcrumbs() {
        let query = crate::query_fixture::markdown("# Heading\n\n<!-- mant:entries role=command case=sensitive -->\n- `root-command`: Root.\n\n## Section heading\n\n<!-- mant:entries role=command case=sensitive -->\n- `nested-command`: Child.\n", None).unwrap();
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
