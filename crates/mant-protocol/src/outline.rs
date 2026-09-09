//! Stable contracts for lightweight query outlines and selected excerpts.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use mant_ir::{
    Block, Diagnostic, DocumentAddress, DocumentMeta, DocumentReference, DocumentSource, EntryKind,
    EntrySummary, NameCase, NodeId, Section, TldrDocument,
};

use crate::{ContentSelector, NodePath, Producer};

/// Exact schema marker for a query outline response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum OutlineSchema {
    /// Version 0.11 of the pre-stable outline protocol.
    #[serde(rename = "mant.outline/v0.11")]
    V0Dot11,
}

impl OutlineSchema {
    /// Serialized identifier of the current outline contract.
    pub const ID: &'static str = "mant.outline/v0.11";
}

/// Semantic entry material included beneath structural outline nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EntryProjection {
    /// Include section topology without entry metadata.
    None,
    /// Include recursive entry counts but not individual entry nodes.
    #[default]
    Summary,
    /// Include every nested semantic entry.
    All,
    /// Include entries of the selected kinds and the ancestors needed to reach them.
    Kinds {
        /// Semantic categories retained by the projection.
        #[schemars(length(min = 1, max = 9))]
        kinds: Vec<EntryKind>,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ClosedEntryProjection {
    None {},
    Summary {},
    All {},
    Kinds { kinds: Vec<EntryKind> },
}

impl<'de> Deserialize<'de> for EntryProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match ClosedEntryProjection::deserialize(deserializer)? {
            ClosedEntryProjection::None {} => Self::None,
            ClosedEntryProjection::Summary {} => Self::Summary,
            ClosedEntryProjection::All {} => Self::All,
            ClosedEntryProjection::Kinds { kinds } => Self::Kinds { kinds },
        })
    }
}

/// Compatibility selector for in-process callers migrating from v0.9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineDetail {
    /// Include only section-level navigation nodes.
    Sections,
    /// Include sections and every semantic definition entry.
    Entries,
}

impl From<OutlineDetail> for EntryProjection {
    fn from(value: OutlineDetail) -> Self {
        match value {
            OutlineDetail::Sections => Self::None,
            OutlineDetail::Entries => Self::All,
        }
    }
}

/// A block-free tree used to discover selectable query content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:outline:v0.11"))]
pub struct QueryOutline {
    /// Exact response schema discriminator.
    pub schema: OutlineSchema,
    /// Entry projection used to build this outline.
    pub entries: EntryProjection,
    /// Optional section or entry selector used as the projection root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<ContentSelector>,
    /// Real reference inventory, independent of entry visibility and grouping.
    pub references: crate::ReferenceInventory,
    /// Human-readable selected-document label.
    pub label: String,
    /// Plain display title derived from authoritative heading or native metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// Exact logical document address, absent for direct-file input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<DocumentAddress>,
    /// Authoritative document source, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
    /// Document metadata, when an authoritative document was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    /// Recoverable parser findings available to diagnostic-oriented transports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// False when lowering or shared IR validation found incomplete semantics.
    ///
    /// The field is omitted for complete outlines so compact transports pay no
    /// steady-state bandwidth cost.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub semantics_complete: bool,
    /// Addressable nodes in document order.
    pub nodes: Vec<OutlineNode>,
}

/// One exact cross-document destination declared by a semantic entry term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryDocumentTarget {
    /// Visible term text associated with this destination.
    #[schemars(length(min = 1))]
    pub label: String,
    /// Source-authored logical reference.
    pub reference: DocumentReference,
    /// Exact logical destination resolved in the source document namespace.
    ///
    /// This is absent for direct-file inputs and references, such as an
    /// unqualified manual name, that require catalog lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<DocumentAddress>,
}

/// Resolved value space accepted by one semantic entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EntryValueDomain {
    /// Values represented by nested entry nodes.
    Choices {
        /// True when the listed choices are known to be exhaustive.
        exhaustive: bool,
    },
    /// Entries owned by another logical document form the value space.
    EntrySet {
        /// Source-authored logical reference.
        reference: DocumentReference,
        /// Exact logical destination when namespace-only resolution suffices.
        #[serde(skip_serializing_if = "Option::is_none")]
        address: Option<DocumentAddress>,
        /// Accepted semantic categories in the referenced document.
        #[schemars(length(min = 1, max = 9))]
        entry_kinds: Vec<EntryKind>,
    },
}

const fn default_true() -> bool {
    true
}

// Serde's `skip_serializing_if` predicate receives a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_true(value: &bool) -> bool {
    *value
}

/// One uniquely addressable node in a query outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OutlineNode {
    /// Optional quick-reference node.
    Tldr {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local identity.
        id: NodeId,
        /// Display title.
        title: String,
    },
    /// Addressable document content that precedes the first heading.
    DocumentRoot {
        /// Canonical structural outline path.
        path: NodePath,
        /// Virtual document-root identity.
        id: NodeId,
        /// Display title for the leading content.
        title: String,
        /// Recursive semantic entry coverage for this scope.
        #[serde(skip_serializing_if = "Option::is_none")]
        entry_summary: Option<EntrySummary>,
        /// Nested semantic entries when explicitly expanded.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<OutlineNode>,
    },
    /// One semantic document section.
    DocumentSection {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local section identity.
        id: NodeId,
        /// Section heading text.
        title: String,
        /// Recursive semantic entry coverage owned directly by this section.
        #[serde(skip_serializing_if = "Option::is_none")]
        entry_summary: Option<EntrySummary>,
        /// Nested section and entry nodes.
        children: Vec<OutlineNode>,
    },
    /// One source-neutral semantic definition.
    DocumentEntry {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local entry identity.
        id: NodeId,
        /// Primary display term.
        title: String,
        /// Semantic category of the entry.
        entry_kind: EntryKind,
        /// Alias case-matching policy.
        case: NameCase,
        /// Exact selectable names, not proof of behavioral equivalence.
        names: Vec<String>,
        /// Explicit owner-local equivalence groups; no default canonical member.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        alias_groups: Vec<Vec<String>>,
        /// Explicit same-document entry relationship; bodies stay independent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alias_of: Option<NodeId>,
        /// Author-written input forms.
        forms: Vec<String>,
        /// Exact cross-document destinations declared by linked entry terms.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        document_targets: Vec<EntryDocumentTarget>,
        /// Optional finite or cross-document value space.
        #[serde(skip_serializing_if = "Option::is_none")]
        value_domain: Option<Box<EntryValueDomain>>,
        /// Recursive semantic entry coverage owned by this entry.
        #[serde(skip_serializing_if = "Option::is_none")]
        entry_summary: Option<EntrySummary>,
        /// Nested entry nodes.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<OutlineNode>,
    },
}

impl OutlineNode {
    /// Return the canonical structural path.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Tldr { path, .. }
            | Self::DocumentRoot { path, .. }
            | Self::DocumentSection { path, .. }
            | Self::DocumentEntry { path, .. } => path,
        }
    }

    /// Return the stable document-local identity.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Tldr { id, .. }
            | Self::DocumentRoot { id, .. }
            | Self::DocumentSection { id, .. }
            | Self::DocumentEntry { id, .. } => id,
        }
    }

    /// Return the node's display title.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Tldr { title, .. }
            | Self::DocumentRoot { title, .. }
            | Self::DocumentSection { title, .. }
            | Self::DocumentEntry { title, .. } => title,
        }
    }

    /// Return child nodes, or an empty slice for leaf variants.
    #[must_use]
    pub fn children(&self) -> &[Self] {
        match self {
            Self::DocumentRoot { children, .. }
            | Self::DocumentSection { children, .. }
            | Self::DocumentEntry { children, .. } => children,
            Self::Tldr { .. } => &[],
        }
    }
}

/// Exact schema marker for selected query content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ExcerptSchema {
    /// Version 0.11 of the pre-stable excerpt protocol.
    #[serde(rename = "mant.excerpt/v0.11")]
    V0Dot11,
}

impl ExcerptSchema {
    /// Serialized identifier of the current excerpt contract.
    pub const ID: &'static str = "mant.excerpt/v0.11";
}

/// One or more independently selected nodes from a complete query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:excerpt:v0.11"))]
pub struct QueryExcerpt {
    /// Exact response schema discriminator.
    pub schema: ExcerptSchema,
    /// Human-readable selected-document label.
    pub label: String,
    /// Plain display title derived from authoritative heading or native metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// Logical namespace for references in the selected raw IR content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<DocumentAddress>,
    /// False when the selected document has rejected or invalid semantics.
    /// This is document-wide, not a claim restricted to the selected range.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub semantics_complete: bool,
    /// Process and parser provenance, when a document was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    /// Authoritative document source, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
    /// Document metadata, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    /// Recoverable parser and validation findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Selected nodes in canonical source order after duplicate selectors are removed.
    pub selections: Vec<ExcerptSelection>,
}

/// One selected document node together with its location in the complete outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExcerptSelection {
    /// Optional quick-reference content preceding the primary document.
    Tldr {
        /// Complete logical location in the document outline.
        outline: OutlineTrail,
        /// Complete quick-reference content.
        document: TldrDocument,
    },
    /// Complete document content that appears before the first heading.
    DocumentRoot {
        /// Complete logical location in the document outline.
        outline: OutlineTrail,
        /// Original visible document title, not a generated report label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<mant_ir::Heading>,
        /// Complete leading blocks.
        blocks: Vec<Block>,
    },
    /// Complete selected document node, including all descendant sections.
    DocumentSection {
        /// Complete logical location in the document outline.
        outline: OutlineTrail,
        /// Complete selected section including descendants.
        section: Section,
    },
    /// One addressable owner with its complete, original content.
    DocumentEntry {
        /// Complete logical location in the document outline.
        outline: OutlineTrail,
        /// A single-item list or definition list, retaining original layout and
        /// numbering. Its sole item owns the entry facts; no body is rewritten.
        entry: Block,
    },
}

impl ExcerptSelection {
    /// Return the complete logical location of this selection.
    #[must_use]
    pub const fn outline(&self) -> &OutlineTrail {
        match self {
            Self::Tldr { outline, .. }
            | Self::DocumentRoot { outline, .. }
            | Self::DocumentSection { outline, .. }
            | Self::DocumentEntry { outline, .. } => outline,
        }
    }
}

/// Complete logical location of one addressable document node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutlineTrail {
    /// Ordered ancestors from the document root to the direct parent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ancestors: Vec<OutlineReference>,
    /// Selected or matching node at the end of the trail.
    pub node: OutlineNodeReference,
}

impl OutlineTrail {
    /// Return the canonical structural path of the terminal node.
    #[must_use]
    pub fn path(&self) -> &str {
        self.node.path()
    }

    /// Return the display title of the terminal node.
    #[must_use]
    pub fn title(&self) -> &str {
        self.node.title()
    }
}

/// Compact ancestor identity attached to an excerpt selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutlineReference {
    /// Canonical structural outline path.
    pub path: NodePath,
    /// Stable document-local identity.
    pub id: NodeId,
    /// Display title.
    pub title: String,
}

/// Compact typed identity for the terminal node in an [`OutlineTrail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OutlineNodeReference {
    /// Optional quick-reference node.
    Tldr {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local identity.
        id: NodeId,
        /// Display title.
        title: String,
    },
    /// Addressable content before the first heading.
    DocumentRoot {
        /// Canonical structural outline path.
        path: NodePath,
        /// Virtual document-root identity.
        id: NodeId,
        /// Display title.
        title: String,
    },
    /// One semantic document section.
    DocumentSection {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local identity.
        id: NodeId,
        /// Section heading text.
        title: String,
    },
    /// One semantic command, option, or variable definition.
    DocumentEntry {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local identity.
        id: NodeId,
        /// Primary display term.
        title: String,
        /// Semantic category of the definition.
        entry_kind: EntryKind,
        /// Alias case-matching policy.
        case: NameCase,
        /// Validated selectable names, not an equivalence assertion.
        names: Vec<String>,
    },
}

impl OutlineNodeReference {
    /// Return the canonical structural path.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Tldr { path, .. }
            | Self::DocumentRoot { path, .. }
            | Self::DocumentSection { path, .. }
            | Self::DocumentEntry { path, .. } => path,
        }
    }

    /// Return the stable document-local identity.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Tldr { id, .. }
            | Self::DocumentRoot { id, .. }
            | Self::DocumentSection { id, .. }
            | Self::DocumentEntry { id, .. } => id,
        }
    }

    /// Return the display title.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Tldr { title, .. }
            | Self::DocumentRoot { title, .. }
            | Self::DocumentSection { title, .. }
            | Self::DocumentEntry { title, .. } => title,
        }
    }
}
