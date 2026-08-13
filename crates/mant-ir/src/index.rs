//! Immutable sidecar index derived from a normalized document.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    DOCUMENT_ROOT_ID, DefinitionItem, Document, Inline, NodeId, Section,
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
}

impl DocumentIndex {
    /// Derive a complete immutable index in one traversal of `document`.
    #[must_use]
    pub fn build(document: &Document) -> Self {
        let mut builder = IndexBuilder::default();
        if !document.blocks.is_empty() {
            builder.register(&NodeId::from(DOCUMENT_ROOT_ID), IndexedRole::Anchor);
        }
        builder.visit_document(document);
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
}

#[derive(Default)]
struct IndexBuilder {
    index: DocumentIndex,
    section_stack: Vec<NodeId>,
}

impl IndexBuilder {
    fn register(&mut self, id: &NodeId, role: IndexedRole) {
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
}

impl<'ir> Visit<'ir> for IndexBuilder {
    fn visit_section(&mut self, section: &'ir Section) {
        self.register(&section.id, IndexedRole::Section);
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
        if let Inline::Anchor { id } = inline {
            self.register(id, IndexedRole::Anchor);
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
            diagnostics: Vec::new(),
            blocks: vec![crate::Block::DefinitionList {
                items: vec![DefinitionItem {
                    identity: Some(DefinitionIdentity {
                        id: id.clone(),
                        role: DefinitionRole::Option,
                        case: DefinitionCase::Sensitive,
                        names: vec!["--help".to_owned()],
                    }),
                    terms: vec![vec![Inline::Anchor { id: id.clone() }]],
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
}
