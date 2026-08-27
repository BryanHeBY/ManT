//! Source-neutral semantic entry index derived from document definitions.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Block, DefinitionCase, DefinitionItem, DefinitionRole, Document, DocumentAddress, Inline,
    NodeId,
};

/// Semantic category used for outline filtering and nested presentation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "kebab-case")]
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
        /// Logical document that owns the referenced entries.
        document: DocumentAddress,
        /// Accepted semantic categories in the referenced document.
        entry_kinds: Vec<EntryKind>,
    },
    /// Union of several independently described value spaces.
    Union {
        /// Constituent value spaces in source order.
        domains: Vec<ValueDomain>,
    },
}

/// One semantic concept backed by one or more document definitions.
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
    /// Author-written input forms, distinct from selectable aliases.
    pub forms: Vec<String>,
    /// Definition nodes that supply content for this concept.
    pub targets: Vec<NodeId>,
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
    Some(SemanticEntry {
        id: identity.id.clone(),
        kind: entry_kind(identity.role),
        aliases: identity.names.clone(),
        case: identity.case,
        forms: item.terms.iter().map(|term| inline_text(term)).collect(),
        targets: vec![identity.id.clone()],
        children: entries_in_blocks(&item.description),
        value_domain: None,
    })
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
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "synopsis".into(),
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
}
