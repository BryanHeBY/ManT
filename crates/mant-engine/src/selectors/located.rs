//! Borrowed semantic locations and source-order breadcrumbs, without DTOs.
use super::DOCUMENT_ROOT_TITLE;
use crate::{
    definitions::{ContentEntry, content_entries},
    inline::plain_text,
};
use mant_ir::{
    Block, DOCUMENT_ROOT_ID, DefinitionIdentity, EntryOwner, NodeId, OutlinePath, Section,
    SourceSpan,
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
        entry: ContentEntry<'a>,
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
                    .item
                    .facts()
                    .expect("located entries have identities")
                    .id
            }
        }
    }

    pub(crate) fn identity(&self) -> Option<&DefinitionIdentity> {
        match self {
            Self::Entry { entry, .. } => entry.item.facts(),
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
        child_breadcrumbs.push(LocatedBreadcrumb {
            path: path.clone(),
            id: section.id.clone(),
            title: section.title.clone(),
        });
        for located in content_entries(&section.blocks) {
            let entry = located.item;
            let mut entry_breadcrumbs = child_breadcrumbs.clone();
            append_entry_breadcrumbs(
                &mut entry_breadcrumbs,
                Some(&coordinates),
                &located.indices,
                &located.ancestors,
            );
            output.push(LocatedNode::Entry {
                order: output.len(),
                coordinates: coordinates.clone(),
                path: OutlinePath::nested_entry(Some(&coordinates), &located.indices)
                    .expect("enumerated entry paths are one-based"),
                title: definition_title(entry, located.names),
                breadcrumbs: entry_breadcrumbs,
                source: located.source,
                entry: located,
            });
        }
        collect_sections(&section.children, &coordinates, &child_breadcrumbs, output);
    }
}

pub(crate) fn collect_root_entries<'a>(blocks: &'a [Block], output: &mut Vec<LocatedNode<'a>>) {
    let breadcrumbs = vec![LocatedBreadcrumb {
        path: OutlinePath::DocumentRoot,
        id: DOCUMENT_ROOT_ID.into(),
        title: DOCUMENT_ROOT_TITLE.to_owned(),
    }];
    for located in content_entries(blocks) {
        let entry = located.item;
        let mut entry_breadcrumbs = breadcrumbs.clone();
        append_entry_breadcrumbs(
            &mut entry_breadcrumbs,
            None,
            &located.indices,
            &located.ancestors,
        );
        output.push(LocatedNode::Entry {
            order: output.len(),
            coordinates: Vec::new(),
            path: OutlinePath::nested_entry(None, &located.indices)
                .expect("enumerated entry paths are one-based"),
            title: definition_title(entry, located.names),
            breadcrumbs: entry_breadcrumbs,
            source: located.source,
            entry: located,
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
    let identity = entry.facts().expect("semantic entries have identities");
    if !names.is_empty() {
        return names.join(", ");
    }
    let forms = entry
        .forms()
        .unwrap_or_default()
        .iter()
        .map(plain_text)
        .filter(|form| !form.is_empty())
        .collect::<Vec<_>>();
    if !forms.is_empty() {
        return forms.join(" | ");
    }
    identity.id.to_string()
}
