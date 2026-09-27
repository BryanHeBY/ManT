//! Immutable sidecar index derived from a normalized document.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ContentContext, DOCUMENT_ROOT_ID, DefinitionItem, Document, DocumentBodyRef, FragmentAlias,
    Inline, InlineView, NodeId, Section,
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
        Self::build_with_fixed_validation(document, None)
    }

    pub(crate) fn build_with_fixed_validation(
        document: &Document,
        fixed_validation: Option<&crate::validation::FixedBodyValidation<'_>>,
    ) -> Self {
        let mut builder = IndexBuilder {
            content: document.content(),
            index: Self::default(),
            section_stack: Vec::new(),
        };
        match document.body() {
            DocumentBodyRef::Flow(flow) => {
                builder.visit_document(document);
                if (flow.heading.is_some()
                    || !flow.blocks.is_empty()
                    || !document.fragment_aliases.is_empty())
                    && !builder.index.nodes.contains_key(DOCUMENT_ROOT_ID)
                {
                    builder.register(&NodeId::from(DOCUMENT_ROOT_ID), IndexedRole::Anchor);
                }
            }
            DocumentBodyRef::Fixed(fixed) => {
                let all_entries_valid = fixed_validation
                    .and_then(|proof| proof.result_for(fixed))
                    .is_some_and(Result::is_ok);
                let root_hint_valid = document.fixed_root_configuration_hint_valid();
                // The codec assigned normalized identities independently of
                // native mark keys. Keep raw declarations as provenance and
                // register only the fragments actually emitted by native HTML.
                builder.register(&NodeId::from(DOCUMENT_ROOT_ID), IndexedRole::Anchor);
                for heading in &fixed.headings {
                    builder.section_stack.clear();
                    if let Some(parent) = heading.parent
                        && let Some(parent) = fixed.headings.get((parent.get() - 1) as usize)
                    {
                        builder.section_stack.push(parent.id.clone());
                    }
                    builder.register(&heading.id, IndexedRole::Section);
                    for alias in &heading.fragment_aliases {
                        builder.index.authored_fragments.insert(alias.clone());
                    }
                    for alias in &heading.rendered_fragment_aliases {
                        builder.register_fragment(alias.clone(), &heading.id, false);
                    }
                }
                builder.section_stack.clear();
                for owner in &fixed.owners {
                    if !root_hint_valid && fixed.root_configuration_dependent(owner) {
                        continue;
                    }
                    if !(all_entries_valid && owner.entry.is_some())
                        && fixed.validated_entry(owner).is_none()
                    {
                        continue;
                    }
                    if let Some(section) = owner.section
                        && let Some(section) = fixed.headings.get((section.get() - 1) as usize)
                    {
                        builder.section_stack.push(section.id.clone());
                    }
                    builder.register(&owner.id, IndexedRole::Entry);
                    builder.section_stack.clear();
                }
                for anchor in &fixed.anchors {
                    if let Some(section) = anchor.section
                        && let Some(section) = fixed.headings.get((section.get() - 1) as usize)
                    {
                        builder.section_stack.push(section.id.clone());
                    }
                    builder.register(&anchor.id, IndexedRole::Anchor);
                    if anchor.authored {
                        builder
                            .index
                            .authored_fragments
                            .insert(FragmentAlias::from(anchor.name.as_str()));
                    }
                    builder.register_fragment(anchor.rendered_fragment.clone(), &anchor.id, false);
                    builder.section_stack.clear();
                }
            }
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

    /// Resolve one canonical or emitted fragment without guessing.
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

struct IndexBuilder<'ir> {
    content: ContentContext<'ir>,
    index: DocumentIndex,
    section_stack: Vec<NodeId>,
}

impl IndexBuilder<'_> {
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

impl<'ir> Visit<'ir> for IndexBuilder<'ir> {
    fn visit_list_item(&mut self, item: &'ir crate::ListItem) {
        if let Some(facts) = &item.entry {
            self.register(&facts.id, IndexedRole::Entry);
        }
        visit::walk_list_item(self, item);
    }
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
        if let Some(identity) = &item.entry {
            self.register(&identity.id, IndexedRole::Entry);
        }
        visit::walk_definition_item(self, item);
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        match self
            .content
            .inline(inline)
            .expect("document inline resolves in its own content store")
        {
            InlineView::Anchor(anchor) => {
                self.register(anchor.id(), IndexedRole::Anchor);
                for alias in anchor.fragment_aliases() {
                    self.register_fragment(alias.clone(), anchor.id(), true);
                }
            }
            InlineView::Strong(children) | InlineView::Emphasis(children) => {
                for child in children {
                    self.visit_inline(child);
                }
            }
            InlineView::Link(link) => {
                for child in link.children() {
                    self.visit_inline(child);
                }
            }
            InlineView::Text(_) | InlineView::Code(_) | InlineView::LineBreak => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        DocumentMeta, EntryFacts, EntryKind, NameCase, SourceCoordinates, SourceFormat,
        SourceIdentity, SourceKey, SourceRecord,
    };

    use super::*;

    #[test]
    fn indexes_shared_entry_anchors_without_calling_them_duplicates() {
        let id = NodeId::from("help");
        let mut fixture = crate::test_support::ContentFixture::body();
        let anchor = fixture.anchor(id.clone());
        let document = Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "test".to_owned(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            body: crate::DocumentBody::Flow(crate::FlowBody {
                content_store: fixture.finish(),
                heading: None,
                blocks: vec![crate::Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    items: vec![DefinitionItem {
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
                        terms: vec![vec![anchor]],
                        description: Vec::new(),
                        layout: crate::DefinitionLayout {
                            inline_term: false,
                            spacing_before_lines: None,
                            ..Default::default()
                        },
                    }],
                    compact: false,
                    layout: crate::LayoutHint::default(),
                    source: None,
                }],
                sections: Vec::new(),
            }),
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
        let mut fixture = crate::test_support::ContentFixture::body();
        let heading_text = fixture.text("Mixed target");
        let anchor = fixture.anchor_with_aliases("option", vec![FragmentAlias::from("--option")]);
        let mut section = Section {
            id: "mixed-target".into(),
            fragment_aliases: vec![FragmentAlias::from("Mixed.Target")],
            heading: crate::Heading {
                content: vec![heading_text],
                source: None,
            },
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        };
        section.blocks.push(crate::Block::Paragraph {
            children: vec![anchor],
            layout: crate::LayoutHint::default(),
            source: None,
        });
        let document = Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "test".to_owned(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            body: crate::DocumentBody::Flow(crate::FlowBody {
                content_store: fixture.finish(),
                heading: None,
                blocks: Vec::new(),
                sections: vec![section],
            }),
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

    #[test]
    #[allow(clippy::too_many_lines)] // Complete synthetic Fixed mark graph keeps identity edges auditable.
    fn fixed_marks_index_normalized_ids_and_exact_authored_aliases() {
        let key = std::num::NonZeroU32::new(1).unwrap();
        let empty = crate::TextSelection {
            parts: Vec::new(),
            joins: Vec::new(),
        };
        let document = Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "fixed".to_owned(),
                },
                format: SourceFormat::Mdoc,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::NativeNormalizedBytes,
            }],
            root_source: SourceKey::FIRST,
            body: crate::DocumentBody::Fixed(crate::FixedBody {
                root_configuration_hint: false,
                surface: crate::DisplaySurface {
                    text: String::new(),
                    rows: Vec::new(),
                    runs: Vec::new(),
                },
                headings: vec![crate::HeadingMark {
                    key,
                    id: NodeId::from("mixed-target"),
                    fragment_aliases: vec![FragmentAlias::from("Mixed.Target")],
                    generated_fragment_aliases: vec![FragmentAlias::from("MIXED_TARGET")],
                    rendered_fragment_aliases: vec![
                        FragmentAlias::from("Mixed.Target"),
                        FragmentAlias::from("MIXED_TARGET"),
                    ],
                    parent: None,
                    level_hint: 1,
                    at: crate::DisplayPoint::DocumentEnd { row_count: 0 },
                    title: empty.clone(),
                    direct_body: empty,
                    source_key: None,
                    source: None,
                }],
                owners: Vec::new(),
                links: Vec::new(),
                anchors: vec![
                    crate::AnchorMark {
                        key,
                        id: NodeId::from("option"),
                        section: Some(key),
                        name: "--option".to_owned(),
                        rendered_fragment: "--option".into(),
                        authored: true,
                        at: crate::DisplayPoint::DocumentEnd { row_count: 0 },
                        source_key: None,
                        source: None,
                    },
                    crate::AnchorMark {
                        key: std::num::NonZeroU32::new(2).unwrap(),
                        id: NodeId::from("generated"),
                        section: Some(key),
                        name: "Generated.Tag".to_owned(),
                        rendered_fragment: "Generated.Tag".into(),
                        authored: false,
                        at: crate::DisplayPoint::DocumentEnd { row_count: 0 },
                        source_key: None,
                        source: None,
                    },
                ],
                regions: Vec::new(),
            }),
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        };
        assert!(crate::validate_document(&document).is_empty());
        let index = DocumentIndex::build(&document);
        assert_eq!(
            index.fragment_target("Mixed.Target").map(NodeId::as_str),
            Some("mixed-target")
        );
        assert_eq!(
            index.fragment_target("MIXED_TARGET").map(NodeId::as_str),
            Some("mixed-target")
        );
        assert!(
            !index
                .authored_fragments()
                .any(|alias| alias.as_str() == "MIXED_TARGET")
        );
        assert_eq!(
            index.fragment_target("--option").map(NodeId::as_str),
            Some("option")
        );
        assert_eq!(
            index.fragment_target("Generated.Tag").map(NodeId::as_str),
            Some("generated")
        );
        assert!(
            !index
                .authored_fragments()
                .any(|alias| alias.as_str() == "Generated.Tag")
        );

        let mut collision = document.clone();
        let crate::DocumentBody::Fixed(fixed) = &mut collision.body else {
            unreachable!();
        };
        fixed.anchors[1].rendered_fragment = "--option".into();
        assert!(crate::validate_document(&collision)
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("ir.ambiguous-fragment-alias")));
    }
}
