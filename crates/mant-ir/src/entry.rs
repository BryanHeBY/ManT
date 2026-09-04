//! Source-neutral semantic entry index derived from document definitions.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    Block, DefinitionCase, DefinitionItem, DefinitionRole, Document, Inline, LinkTarget, NodeId,
};

/// Semantic category used for outline filtering and nested presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EntryKind {
    /// Executable command, builtin, subcommand, or verb.
    Command,
    /// Command input whose exact behavior is described by [`ParameterKind`].
    Parameter {
        /// Parameter syntax family.
        parameter_kind: ParameterKind,
    },
    /// Named key accepted by a configuration language or parameter.
    ConfigurationKey,
    /// Process environment variable.
    EnvironmentVariable,
    /// Shell, language, or application variable.
    Variable,
    /// One value accepted by a parent entry.
    Value,
    /// Addressable definition without a more specific reliable category.
    Term,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ClosedEntryKind {
    Command {},
    Parameter { parameter_kind: ParameterKind },
    ConfigurationKey {},
    EnvironmentVariable {},
    Variable {},
    Value {},
    Term {},
}

impl<'de> Deserialize<'de> for EntryKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match ClosedEntryKind::deserialize(deserializer)? {
            ClosedEntryKind::Command {} => Self::Command,
            ClosedEntryKind::Parameter { parameter_kind } => Self::Parameter { parameter_kind },
            ClosedEntryKind::ConfigurationKey {} => Self::ConfigurationKey,
            ClosedEntryKind::EnvironmentVariable {} => Self::EnvironmentVariable,
            ClosedEntryKind::Variable {} => Self::Variable,
            ClosedEntryKind::Value {} => Self::Value,
            ClosedEntryKind::Term {} => Self::Term,
        })
    }
}

/// Semantic behavior of one command parameter.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterKind {
    /// Named option or switch such as `-L`, `/?`, or `+r`.
    Option,
    /// Parser-control marker such as `--` or PowerShell's `--%`.
    Marker,
    /// Positional or special operand such as a documented stdin `-`.
    Operand,
}

/// Logical value space accepted by one semantic entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ValueDomain {
    /// Values represented by child [`SemanticEntry`] nodes.
    Choices {
        /// True when the listed choices are known to be exhaustive.
        exhaustive: bool,
    },
    /// Entries owned by another logical document form the value space.
    EntrySet {
        /// Source-neutral reference to the document that owns the entries.
        reference: SemanticDocumentReference,
        /// Accepted semantic categories in the referenced document.
        entry_kinds: Vec<EntryKind>,
    },
}

/// A source-neutral reference to another locally addressable document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SemanticDocumentReference {
    /// A relative Markdown document in the current registered namespace.
    Document {
        /// Extension-free relative document path.
        name: String,
        /// Optional document-local fragment.
        #[serde(skip_serializing_if = "Option::is_none")]
        fragment: Option<String>,
    },
    /// A typed native manual reference.
    Manual {
        /// Manual topic without a section suffix.
        name: String,
        /// Native manual category, when source-specified.
        #[serde(skip_serializing_if = "Option::is_none")]
        manual_section: Option<String>,
    },
}

impl SemanticDocumentReference {
    /// Retain a locally addressable document target and reject other links.
    #[must_use]
    pub fn from_link_target(target: &LinkTarget) -> Option<Self> {
        match target {
            LinkTarget::Document { name, fragment } => Some(Self::Document {
                name: name.clone(),
                fragment: fragment.clone(),
            }),
            LinkTarget::Manual {
                name,
                manual_section,
            } => Some(Self::Manual {
                name: name.clone(),
                manual_section: manual_section.clone(),
            }),
            LinkTarget::External { .. } | LinkTarget::Email { .. } | LinkTarget::Section { .. } => {
                None
            }
        }
    }

    /// Resolve this reference without catalog I/O in the referring namespace.
    ///
    /// An unqualified manual reference remains unresolved because selecting a
    /// section requires catalog precedence and ambiguity handling.
    #[must_use]
    pub fn resolve_from(&self, from: &crate::DocumentAddress) -> Option<crate::DocumentAddress> {
        match self {
            Self::Document { name, .. } => from.resolve_document_reference(name),
            Self::Manual {
                name,
                manual_section: Some(manual_section),
            } => Some(crate::DocumentAddress::Manual {
                name: name.clone(),
                manual_section: manual_section.clone(),
            }),
            Self::Manual {
                manual_section: None,
                ..
            } => None,
        }
    }
}

/// One explicit cross-document destination carried by an entry term.
///
/// The relationship is derived only from a link that wraps term content. A
/// link in the entry description remains ordinary reference material and does
/// not change where the semantic entry itself leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDocumentTarget {
    /// Visible term text associated with this destination.
    pub label: String,
    /// Source-neutral logical destination.
    pub reference: SemanticDocumentReference,
}

/// One indexed semantic concept backed by one or more document definitions.
///
/// This value is derived from [`DefinitionIdentity`](crate::DefinitionIdentity)
/// facts in the document tree. It groups selection, presentation, and content
/// ownership metadata without replacing those authoritative definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticEntry {
    /// Stable document-local semantic identity.
    pub id: NodeId,
    /// Semantic category used by outline filters and presentation.
    pub kind: EntryKind,
    /// Exact selectable spellings in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Alias case-matching policy.
    pub case: DefinitionCase,
    /// Complete author-written input forms, distinct from selectable aliases.
    pub forms: Vec<String>,
    /// Explicit cross-document destinations carried by linked terms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub document_targets: Vec<SemanticDocumentTarget>,
    /// Nested semantic entries owned by this concept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SemanticEntry>,
    /// Optional finite or cross-document value space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_domain: Option<ValueDomain>,
}

impl SemanticEntry {
    /// Count this entry and every nested semantic entry.
    #[must_use]
    pub fn subtree_len(&self) -> usize {
        1 + self.children.iter().map(Self::subtree_len).sum::<usize>()
    }
}

/// Compact coverage metadata for one document scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    /// Entries directly owned by the scope.
    pub direct: u32,
    /// Entries nested under direct entries.
    pub descendants: u32,
    /// Author-written input forms across direct and nested entries.
    pub forms: u32,
    /// Recursive entry totals grouped by semantic category.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_kind: Vec<EntryKindCount>,
}

/// Number of entries belonging to one semantic category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryKindCount {
    /// Semantic category being counted.
    pub kind: EntryKind,
    /// Recursive number of matching entries.
    pub count: u32,
}

impl EntrySummary {
    /// Summarize one source-ordered entry slice.
    #[must_use]
    pub fn for_entries(entries: &[SemanticEntry]) -> Self {
        let mut summary = Self {
            direct: u32::try_from(entries.len()).unwrap_or(u32::MAX),
            ..Self::default()
        };
        for entry in entries {
            summarize_entry(entry, &mut summary, true);
        }
        summary
    }

    /// Return true when the scope contains no entries or forms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.direct == 0 && self.descendants == 0 && self.forms == 0
    }
}

/// Rebuildable semantic index for the document root and every section.
///
/// The index is a derived navigation sidecar. Definitions and their identities
/// remain in the [`Document`] content tree, so callers may rebuild this value
/// after a trusted document transformation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticIndex {
    root: Vec<SemanticEntry>,
    sections: BTreeMap<NodeId, Vec<SemanticEntry>>,
}

impl SemanticIndex {
    /// Build the semantic index from finalized definition identities.
    #[must_use]
    pub fn build(document: &Document) -> Self {
        let root = entries_in_blocks(&document.blocks);
        let mut sections = BTreeMap::new();
        collect_section_entries(&document.sections, &mut sections);
        Self { root, sections }
    }

    /// Entries directly owned by content before the first section.
    #[must_use]
    pub fn root(&self) -> &[SemanticEntry] {
        &self.root
    }

    /// Entries directly owned by one section.
    #[must_use]
    pub fn section(&self, id: &str) -> &[SemanticEntry] {
        self.sections.get(id).map_or(&[], Vec::as_slice)
    }

    /// Summary for content before the first section.
    #[must_use]
    pub fn root_summary(&self) -> EntrySummary {
        EntrySummary::for_entries(&self.root)
    }

    /// Summary for one section without including child sections.
    #[must_use]
    pub fn section_summary(&self, id: &str) -> EntrySummary {
        EntrySummary::for_entries(self.section(id))
    }
}

fn collect_section_entries(
    sections: &[crate::Section],
    output: &mut BTreeMap<NodeId, Vec<SemanticEntry>>,
) {
    for section in sections {
        output.insert(section.id.clone(), entries_in_blocks(&section.blocks));
        collect_section_entries(&section.children, output);
    }
}

fn entries_in_blocks(blocks: &[Block]) -> Vec<SemanticEntry> {
    let mut entries = Vec::new();
    for block in blocks {
        match block {
            Block::DefinitionList { items, .. } => {
                entries.extend(items.iter().filter_map(entry_from_definition));
            }
            Block::List { items, .. } => {
                for item in items {
                    entries.extend(entries_in_blocks(&item.blocks));
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    entries.extend(entries_in_blocks(&cell.blocks));
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
    entries
}

fn entry_from_definition(item: &DefinitionItem) -> Option<SemanticEntry> {
    let identity = item.identity.as_ref()?;
    let children = entries_in_blocks(&item.description);
    let value_domain = identity.value_domain.clone().or_else(|| {
        (!children.is_empty() && children.iter().all(|child| child.kind == EntryKind::Value))
            .then_some(ValueDomain::Choices { exhaustive: false })
    });
    Some(SemanticEntry {
        id: identity.id.clone(),
        kind: entry_kind(identity.role),
        aliases: identity.names.clone(),
        case: identity.case,
        forms: item.terms.iter().map(|term| inline_text(term)).collect(),
        document_targets: document_targets(&item.terms),
        children,
        value_domain,
    })
}

fn document_targets(terms: &[Vec<Inline>]) -> Vec<SemanticDocumentTarget> {
    let mut targets = Vec::new();
    for term in terms {
        collect_document_targets(term, &mut targets);
    }
    targets
}

fn collect_document_targets(inlines: &[Inline], output: &mut Vec<SemanticDocumentTarget>) {
    for inline in inlines {
        match inline {
            Inline::Link {
                target, children, ..
            } if SemanticDocumentReference::from_link_target(target).is_some() => {
                let candidate = SemanticDocumentTarget {
                    label: inline_text(children),
                    reference: SemanticDocumentReference::from_link_target(target)
                        .expect("the match guard accepts a document reference"),
                };
                if !output.contains(&candidate) {
                    output.push(candidate);
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_document_targets(children, output),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
}

const fn entry_kind(role: DefinitionRole) -> EntryKind {
    match role {
        DefinitionRole::Option => EntryKind::Parameter {
            parameter_kind: ParameterKind::Option,
        },
        DefinitionRole::Marker => EntryKind::Parameter {
            parameter_kind: ParameterKind::Marker,
        },
        DefinitionRole::Operand => EntryKind::Parameter {
            parameter_kind: ParameterKind::Operand,
        },
        DefinitionRole::Command => EntryKind::Command,
        DefinitionRole::ConfigurationKey => EntryKind::ConfigurationKey,
        DefinitionRole::EnvironmentVariable => EntryKind::EnvironmentVariable,
        DefinitionRole::Variable => EntryKind::Variable,
        DefinitionRole::Value => EntryKind::Value,
        DefinitionRole::Term => EntryKind::Term,
    }
}

fn summarize_entry(entry: &SemanticEntry, summary: &mut EntrySummary, direct: bool) {
    if !direct {
        summary.descendants = summary.descendants.saturating_add(1);
    }
    summary.forms = summary
        .forms
        .saturating_add(u32::try_from(entry.forms.len()).unwrap_or(u32::MAX));
    if let Some(count) = summary
        .by_kind
        .iter_mut()
        .find(|count| count.kind == entry.kind)
    {
        count.count = count.count.saturating_add(1);
    } else {
        summary.by_kind.push(EntryKindCount {
            kind: entry.kind,
            count: 1,
        });
        summary.by_kind.sort_by_key(|count| count.kind);
    }
    for child in &entry.children {
        summarize_entry(child, summary, false);
    }
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut output = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => output.push_str(&inline_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use crate::{
        DefinitionCase, DefinitionIdentity, DocumentMeta, DocumentSource, LayoutHint, Section,
        SourceFormat,
    };

    use super::*;

    fn definition(
        id: &str,
        role: DefinitionRole,
        aliases: &[&str],
        forms: &[&str],
        description: Vec<Block>,
    ) -> DefinitionItem {
        DefinitionItem {
            identity: Some(DefinitionIdentity {
                id: id.into(),
                role,
                case: DefinitionCase::Sensitive,
                names: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
                value_domain: None,
            }),
            terms: forms
                .iter()
                .map(|form| {
                    vec![Inline::Code {
                        value: (*form).to_owned(),
                    }]
                })
                .collect(),
            description,
            inline_term: false,
            spacing_before_lines: None,
        }
    }

    #[test]
    fn preserves_definition_nesting_and_counts_forms_separately() {
        let option = definition(
            "option-local-forward",
            DefinitionRole::Option,
            &["-L"],
            &["-L port:host:hostport", "-L socket:remote_socket"],
            Vec::new(),
        );
        let command = definition(
            "command-ssh",
            DefinitionRole::Command,
            &["ssh"],
            &["ssh destination"],
            vec![Block::DefinitionList {
                items: vec![option],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        );
        let document = Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Mdoc,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "synopsis".into(),
                fragment_aliases: Vec::new(),
                title: "SYNOPSIS".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
                    items: vec![command],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: Vec::new(),
                source: None,
            }],
        };

        let index = SemanticIndex::build(&document);
        let entries = index.section("synopsis");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryKind::Command);
        assert_eq!(entries[0].children.len(), 1);
        assert_eq!(entries[0].children[0].aliases, ["-L"]);
        assert_eq!(entries[0].children[0].forms.len(), 2);
        assert_eq!(entries[0].subtree_len(), 2);

        assert_eq!(
            index.section_summary("synopsis"),
            EntrySummary {
                direct: 1,
                descendants: 1,
                forms: 3,
                by_kind: vec![
                    EntryKindCount {
                        kind: EntryKind::Command,
                        count: 1,
                    },
                    EntryKindCount {
                        kind: EntryKind::Parameter {
                            parameter_kind: ParameterKind::Option,
                        },
                        count: 1,
                    },
                ],
            }
        );
    }

    #[test]
    fn derives_document_targets_only_from_linked_terms() {
        let mut item = definition(
            "command-winget",
            DefinitionRole::Command,
            &["winget.exe"],
            &[],
            vec![Block::Paragraph {
                children: vec![Inline::Link {
                    target: LinkTarget::Document {
                        name: "description-only".to_owned(),
                        fragment: None,
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "details".to_owned(),
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
        );
        item.terms = vec![vec![Inline::Link {
            target: LinkTarget::Document {
                name: "winget.exe".to_owned(),
                fragment: None,
            },
            title: None,
            children: vec![Inline::Code {
                value: "winget.exe".to_owned(),
            }],
        }]];

        let entry = entry_from_definition(&item).expect("entry");
        assert_eq!(
            entry.document_targets,
            [SemanticDocumentTarget {
                label: "winget.exe".to_owned(),
                reference: SemanticDocumentReference::Document {
                    name: "winget.exe".to_owned(),
                    fragment: None,
                },
            }]
        );
    }

    #[test]
    fn explicit_cross_document_domain_survives_index_derivation() {
        let mut item = definition(
            "option-config",
            DefinitionRole::Option,
            &["-o"],
            &["-o option"],
            Vec::new(),
        );
        item.identity.as_mut().expect("identity").value_domain = Some(ValueDomain::EntrySet {
            reference: SemanticDocumentReference::Manual {
                name: "ssh_config".to_owned(),
                manual_section: Some("5".to_owned()),
            },
            entry_kinds: vec![EntryKind::ConfigurationKey],
        });

        let entry = entry_from_definition(&item).expect("entry");
        assert!(matches!(
            entry.value_domain,
            Some(ValueDomain::EntrySet {
                reference: SemanticDocumentReference::Manual {
                    ref name,
                    manual_section: Some(ref section),
                },
                ref entry_kinds,
            }) if name == "ssh_config"
                && section == "5"
                && entry_kinds == &[EntryKind::ConfigurationKey]
        ));
    }
}
