//! Immutable sidecar index derived from a normalized document.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    DOCUMENT_ROOT_ID, DefinitionItem, Document, FragmentAlias, Inline, NodeId, Section,
    visit::{self, Visit},
};

/// Semantic roles that can share one document-local identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndexedRole {
    /// A semantic document section.
    Section,
    /// A named command, option, or variable entry.
    Entry,
    /// An explicit inline navigation destination.
    Anchor,
}

/// Everything known about one ID without borrowing the document tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedNode {
    roles: BTreeSet<IndexedRole>,
    containing_section: Option<NodeId>,
}

impl IndexedNode {
    /// Return every semantic role registered for this identity.
    #[must_use]
    pub fn roles(&self) -> &BTreeSet<IndexedRole> {
        &self.roles
    }

    /// Return the nearest containing section, if any.
    #[must_use]
    pub fn containing_section(&self) -> Option<&NodeId> {
        self.containing_section.as_ref()
    }

    /// Test whether this identity carries a particular semantic role.
    #[must_use]
    pub fn has_role(&self, role: IndexedRole) -> bool {
        self.roles.contains(&role)
    }
}

/// A repeated identity with the same semantic role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateIdentity {
    /// Repeated document-local identity.
    pub id: NodeId,
    /// Semantic role that was registered more than once.
    pub role: IndexedRole,
}

/// One-pass lookup index for navigation, validation, and projections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentIndex {
    nodes: BTreeMap<NodeId, IndexedNode>,
    duplicates: Vec<DuplicateIdentity>,
    fragment_targets: BTreeMap<FragmentAlias, BTreeSet<NodeId>>,
    authored_fragments: BTreeSet<FragmentAlias>,
}

impl DocumentIndex {
    /// Derive a complete immutable index in one traversal of `document`.
    #[must_use]
    pub fn build(document: &Document) -> Self {
        let mut builder = IndexBuilder::default();
        builder.visit_document(document);
        if (!document.blocks.is_empty() || !document.fragment_aliases.is_empty())
            && !builder.index.nodes.contains_key(DOCUMENT_ROOT_ID)
        {
            builder.register(&NodeId::from(DOCUMENT_ROOT_ID), IndexedRole::Anchor);
        }
        for alias in &document.fragment_aliases {
            builder.register_fragment(alias.clone(), &NodeId::from(DOCUMENT_ROOT_ID), true);
        }
        builder.index
    }

    /// Look up one identity by its normalized string value.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&IndexedNode> {
        self.nodes.get(id)
    }

    /// Test whether any semantic node uses `id`.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    /// Iterate over identities in lexical order.
    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &IndexedNode)> {
        self.nodes.iter()
    }

    /// Return repeated same-role identities discovered while indexing.
    #[must_use]
    pub fn duplicates(&self) -> &[DuplicateIdentity] {
        &self.duplicates
    }

    /// Resolve one canonical or source-authored fragment without guessing.
    ///
    /// `None` means the fragment is absent or names more than one target.
    #[must_use]
    pub fn fragment_target(&self, fragment: &str) -> Option<&NodeId> {
        let targets = self.fragment_targets.get(fragment)?;
        let mut targets = targets.iter();
        let target = targets.next()?;
        targets.next().is_none().then_some(target)
    }

    /// Iterate exact aliases contributed by document producers.
    pub fn authored_fragments(&self) -> impl Iterator<Item = &FragmentAlias> {
        self.authored_fragments.iter()
    }

    /// Iterate fragments that resolve to more than one canonical target.
    pub fn ambiguous_fragments(&self) -> impl Iterator<Item = (&FragmentAlias, &BTreeSet<NodeId>)> {
        self.fragment_targets
            .iter()
            .filter(|(_, targets)| targets.len() > 1)
    }
}

#[derive(Default)]
struct IndexBuilder {
    index: DocumentIndex,
    section_stack: Vec<NodeId>,
}

impl IndexBuilder {
    fn register(&mut self, id: &NodeId, role: IndexedRole) {
        self.register_fragment(FragmentAlias::from(id.as_str()), id, false);
        let containing_section = self.section_stack.last().cloned();
        let node = self
            .index
            .nodes
            .entry(id.clone())
            .or_insert_with(|| IndexedNode {
                roles: BTreeSet::new(),
                containing_section,
            });
        if !node.roles.insert(role) {
            self.index.duplicates.push(DuplicateIdentity {
                id: id.clone(),
                role,
            });
        }
    }

    fn register_fragment(&mut self, alias: FragmentAlias, id: &NodeId, authored: bool) {
        if authored {
            self.index.authored_fragments.insert(alias.clone());
        }
        self.index
            .fragment_targets
            .entry(alias)
            .or_default()
            .insert(id.clone());
    }
}

impl<'ir> Visit<'ir> for IndexBuilder {
    fn visit_section(&mut self, section: &'ir Section) {
        self.register(&section.id, IndexedRole::Section);
        for alias in &section.fragment_aliases {
            self.register_fragment(alias.clone(), &section.id, true);
        }
        self.section_stack.push(section.id.clone());
        visit::walk_section(self, section);
        self.section_stack.pop();
    }

    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        if let Some(identity) = &item.identity {
            self.register(&identity.id, IndexedRole::Entry);
        }
        visit::walk_definition_item(self, item);
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Anchor {
            id,
            fragment_aliases,
        } = inline
        {
            self.register(id, IndexedRole::Anchor);
            for alias in fragment_aliases {
                self.register_fragment(alias.clone(), id, true);
            }
        }
        visit::walk_inline(self, inline);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        DefinitionCase, DefinitionIdentity, DefinitionRole, DocumentMeta, DocumentSource,
        SourceFormat,
    };

    use super::*;

    #[test]
    fn indexes_shared_entry_anchors_without_calling_them_duplicates() {
        let id = NodeId::from("help");
        let document = Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: vec![crate::Block::DefinitionList {
                items: vec![DefinitionItem {
                    identity: Some(DefinitionIdentity {
                        id: id.clone(),
                        role: DefinitionRole::Option,
                        case: DefinitionCase::Sensitive,
                        names: vec!["--help".to_owned()],
                    }),
                    terms: vec![vec![Inline::anchor(id.clone())]],
                    description: Vec::new(),
                    inline_term: false,
                    spacing_before_lines: None,
                }],
                compact: false,
                layout: crate::LayoutHint::default(),
                source: None,
            }],
            sections: Vec::new(),
        };

        let index = DocumentIndex::build(&document);
        let indexed = index.get("help").expect("entry must be indexed");
        assert_eq!(
            indexed.roles(),
            &BTreeSet::from([IndexedRole::Entry, IndexedRole::Anchor])
        );
        assert!(index.duplicates().is_empty());
    }

    #[test]
    fn resolves_exact_fragments_to_normalized_targets_without_guessing() {
        let mut section = Section {
            id: "mixed-target".into(),
            fragment_aliases: vec![FragmentAlias::from("Mixed.Target")],
            title: "Mixed target".to_owned(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        };
        section.blocks.push(crate::Block::Paragraph {
            children: vec![Inline::anchor_with_aliases(
                "option",
                vec![FragmentAlias::from("--option")],
            )],
            layout: crate::LayoutHint::default(),
            source: None,
        });
        let document = Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![section],
        };

        let index = DocumentIndex::build(&document);
        assert_eq!(
            index.fragment_target("Mixed.Target").map(NodeId::as_str),
            Some("mixed-target")
        );
        assert_eq!(
            index.fragment_target("--option").map(NodeId::as_str),
            Some("option")
        );
        assert_eq!(
            index.fragment_target("mixed-target").map(NodeId::as_str),
            Some("mixed-target")
        );
    }
}
