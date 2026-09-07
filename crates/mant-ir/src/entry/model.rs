//! Source-neutral semantic entry index derived from document definitions.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{DefinitionCase, LinkTarget, NodeId};

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
        #[schemars(length(min = 1, max = 9))]
        entry_kinds: Vec<EntryKind>,
        /// Source position of the relationship declaration, when available.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<crate::SourceSpan>,
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
        #[schemars(length(min = 1))]
        name: String,
        /// Optional document-local fragment.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1))]
        fragment: Option<String>,
    },
    /// A typed native manual reference.
    Manual {
        /// Manual topic without a section suffix.
        #[schemars(length(min = 1))]
        name: String,
        /// Native manual category, when source-specified.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1))]
        manual_section: Option<String>,
    },
}

impl SemanticDocumentReference {
    /// Return whether every component follows the source-neutral reference
    /// grammar shared by producers and IR validation.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        match self {
            Self::Document { name, fragment } => {
                !name.is_empty()
                    && !name.starts_with('/')
                    && !name.contains(['\\', '?', '#'])
                    && !name.chars().any(char::is_control)
                    && name.split('/').all(|component| !component.is_empty())
                    && fragment.as_deref().is_none_or(|fragment| {
                        !fragment.is_empty() && !fragment.chars().any(char::is_control)
                    })
            }
            Self::Manual {
                name,
                manual_section,
            } => {
                !name.is_empty()
                    && !name.contains(['/', '\\'])
                    && !name
                        .chars()
                        .any(|character| character.is_whitespace() || character.is_control())
                    && manual_section
                        .as_deref()
                        .is_none_or(crate::is_manual_section)
            }
        }
    }

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
