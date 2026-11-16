//! Immutable sidecar index derived from a normalized document.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    DOCUMENT_ROOT_ID, DefinitionItem, Document, EntryFacts, FragmentAlias, Inline, NodeId, Section,
    visit::{self, Visit},
};

const OVERVIEW_OWNER: usize = 0;

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
    overview_aliases: BTreeMap<FragmentAlias, OverviewAlias>,
    overview_conflict: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OverviewAlias {
    owner: usize,
    conflicting_owners: bool,
}

impl DocumentIndex {
    /// Derive a complete immutable index in one traversal of `document`.
    #[must_use]
    pub fn build(document: &Document) -> Self {
        let mut builder = IndexBuilder::default();
        if document.has_overview() {
            let root = NodeId::from(DOCUMENT_ROOT_ID);
            builder.register(&root, IndexedRole::Anchor);
            builder.has_overview = true;
            for alias in &document.fragment_aliases {
                builder.register_fragment(alias.clone(), &root, true, Some(OVERVIEW_OWNER));
            }
        }
        builder.visit_document(document);
        builder.index
    }

    /// Look up one identity by its normalized string value.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&IndexedNode> {
        self.nodes.get(id)
    }

    /// Test whether a canonical local navigation target uses `id`.
    ///
    /// Sections, entries and inline anchors are all addressable content.
    /// Entry-backed targets do not require a second inline anchor in their
    /// visible content. This lookup does not resolve authored fragment aliases
    /// or suppress independent duplicate/role-collision diagnostics.
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

    /// Whether a section, entry or independent anchor claims the actual overview's ID.
    ///
    /// A direct root anchor may share the synthetic destination. Without an overview
    /// this spelling has the same identity rules as any other ID.
    #[must_use]
    pub fn has_overview_conflict(&self) -> bool {
        self.overview_conflict
    }

    /// Resolve one canonical or source-authored fragment without guessing.
    ///
    /// `None` means the fragment is absent, names more than one target,
    /// or its canonical ID cannot distinguish conflicting overview owners.
    /// Use the range-aware navigation scan to recover a unique exact alias
    /// at its original reveal site in the last case.
    #[must_use]
    pub fn fragment_target(&self, fragment: &str) -> Option<&NodeId> {
        let targets = self.fragment_targets.get(fragment)?;
        let target = targets.first()?;
        (targets.len() == 1 && !(self.overview_conflict && target.as_str() == DOCUMENT_ROOT_ID))
            .then_some(target)
    }

    /// Whether a fragment addresses different canonical IDs or multiple owners of the overview ID.
    #[must_use]
    pub fn is_fragment_ambiguous(&self, fragment: &str) -> bool {
        self.fragment_has_conflicting_owners(fragment)
            || self
                .fragment_targets
                .get(fragment)
                .is_some_and(|targets| targets.len() > 1)
    }

    /// Exact aliases contributed by the overview and its shared inline anchor.
    pub fn overview_aliases(&self) -> impl Iterator<Item = &FragmentAlias> {
        self.overview_aliases
            .iter()
            .filter(|(_, alias)| alias.owner == OVERVIEW_OWNER)
            .map(|(fragment, _)| fragment)
    }

    /// Whether the overview or its shared inline anchor declares this exact alias.
    #[must_use]
    pub fn is_overview_alias(&self, fragment: &str) -> bool {
        self.overview_aliases
            .get(fragment)
            .is_some_and(|alias| alias.owner == OVERVIEW_OWNER)
    }

    /// Iterate exact aliases contributed by document producers.
    pub fn authored_fragments(&self) -> impl Iterator<Item = &FragmentAlias> {
        self.authored_fragments.iter()
    }

    /// Iterate ambiguous fragments and their canonical IDs.
    ///
    /// Independent owners of the overview ID share one canonical spelling.
    pub fn ambiguous_fragments(&self) -> impl Iterator<Item = (&FragmentAlias, &BTreeSet<NodeId>)> {
        self.fragment_targets.iter().filter(|(fragment, targets)| {
            targets.len() > 1 || self.fragment_has_conflicting_owners(fragment)
        })
    }

    fn fragment_has_conflicting_owners(&self, fragment: &str) -> bool {
        (fragment == DOCUMENT_ROOT_ID && self.overview_conflict)
            || self
                .overview_aliases
                .get(fragment)
                .is_some_and(|alias| alias.conflicting_owners)
    }
}

#[derive(Default)]
struct IndexBuilder {
    index: DocumentIndex,
    section_stack: Vec<NodeId>,
    has_overview: bool,
    overview_anchor: bool,
    competing_overview_owners: usize,
    entry_overview_owner: Option<usize>,
    content_depth: usize,
}

impl IndexBuilder {
    fn register_independent(&mut self, id: &NodeId, role: IndexedRole) -> Option<usize> {
        let owner = (self.has_overview && id.as_str() == DOCUMENT_ROOT_ID).then(|| {
            self.index.overview_conflict = true;
            self.competing_overview_owners += 1;
            self.competing_overview_owners
        });
        self.register(id, role);
        owner
    }

    fn register(&mut self, id: &NodeId, role: IndexedRole) {
        self.register_fragment(FragmentAlias::from(id.as_str()), id, false, None);
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

    fn register_fragment(
        &mut self,
        alias: FragmentAlias,
        id: &NodeId,
        authored: bool,
        overview_owner: Option<usize>,
    ) {
        if authored {
            self.index.authored_fragments.insert(alias.clone());
        }
        if let Some(owner) = overview_owner {
            let claim = self
                .index
                .overview_aliases
                .entry(alias.clone())
                .or_insert(OverviewAlias {
                    owner,
                    conflicting_owners: false,
                });
            claim.conflicting_owners |= claim.owner != owner;
            // Keep the overview declaration visible to the root exporter even
            // when the competing declaration was encountered first.
            claim.owner = claim.owner.min(owner);
        }
        self.index
            .fragment_targets
            .entry(alias)
            .or_default()
            .insert(id.clone());
    }

    fn enter_item(&mut self, facts: Option<&EntryFacts>) -> Option<usize> {
        let previous = self.entry_overview_owner;
        if let Some(facts) = facts {
            self.entry_overview_owner = self.register_independent(&facts.id, IndexedRole::Entry);
        }
        self.content_depth += 1;
        previous
    }

    fn leave_item(&mut self, previous: Option<usize>) {
        self.content_depth -= 1;
        self.entry_overview_owner = previous;
    }
}

impl<'ir> Visit<'ir> for IndexBuilder {
    fn visit_list_item(&mut self, item: &'ir crate::ListItem) {
        let previous = self.enter_item(item.entry.as_ref());
        visit::walk_list_item(self, item);
        self.leave_item(previous);
    }
    fn visit_section(&mut self, section: &'ir Section) {
        let owner = self.register_independent(&section.id, IndexedRole::Section);
        for alias in &section.fragment_aliases {
            self.register_fragment(alias.clone(), &section.id, true, owner);
        }
        self.section_stack.push(section.id.clone());
        visit::walk_section(self, section);
        self.section_stack.pop();
    }

    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        let previous = self.enter_item(item.entry.as_ref());
        visit::walk_definition_item(self, item);
        self.leave_item(previous);
    }

    fn visit_block(&mut self, block: &'ir crate::Block) {
        let table = usize::from(matches!(block, crate::Block::Table { .. }));
        self.content_depth += table;
        visit::walk_block(self, block);
        self.content_depth -= table;
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Anchor {
            id,
            fragment_aliases,
            ..
        } = inline
        {
            let owner = if id.as_str() == DOCUMENT_ROOT_ID && self.has_overview {
                if self.section_stack.is_empty() && self.content_depth == 0 && !self.overview_anchor
                {
                    self.overview_anchor = true;
                    Some(OVERVIEW_OWNER)
                } else if let Some(owner) = self.entry_overview_owner {
                    self.register(id, IndexedRole::Anchor);
                    Some(owner)
                } else {
                    self.register_independent(id, IndexedRole::Anchor)
                }
            } else {
                self.register_independent(id, IndexedRole::Anchor)
            };
            for alias in fragment_aliases {
                self.register_fragment(alias.clone(), id, true, owner);
            }
        }
        visit::walk_inline(self, inline);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        DocumentMeta, DocumentSource, EntryFacts, EntryKind, HeadBodyRelation, NameCase,
        SourceFormat,
    };

    use super::*;

    #[test]
    fn indexes_shared_entry_anchors_without_calling_them_duplicates() {
        let id = NodeId::from("help");
        let document = Document {
            heading: None,
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: vec![crate::Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![DefinitionItem {
                    head_body_relation: HeadBodyRelation::from(false),
                    source: None,
                    entry: Some(EntryFacts {
                        name_bindings: Vec::new(),
                        alias_groups: Vec::new(),
                        alias_of: None,
                        forms: Vec::new(),
                        id: id.clone(),
                        kind: EntryKind::Parameter {
                            parameter_kind: crate::ParameterKind::Option,
                        },
                        case: NameCase::Sensitive,
                        names: vec!["--help".to_owned()],
                        value_domain: None,
                    }),
                    terms: (vec![vec![Inline::anchor(id.clone())]])
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                    description: Vec::new(),
                    layout: crate::DefinitionLayout {
                        body_alignment: crate::DefinitionBodyAlignment::Indented,
                        spacing_before_lines: None,
                        ..Default::default()
                    },
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
        assert_eq!(index.duplicates().len(), 0);
    }

    #[test]
    fn resolves_exact_fragments_to_normalized_targets_without_guessing() {
        let mut section = Section {
            id: "mixed-target".into(),
            fragment_aliases: vec![FragmentAlias::from("Mixed.Target")],
            heading: "Mixed target".into(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        };
        section.blocks.push(crate::Block::Paragraph {
            inline_layout: crate::InlineLayout::default(),
            children: vec![Inline::anchor_with_aliases(
                "option",
                vec![FragmentAlias::from("--option")],
            )],
            layout: crate::LayoutHint::default(),
            source: None,
        });
        let document = Document {
            heading: None,
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
