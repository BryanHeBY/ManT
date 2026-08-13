//! Stable contracts for lightweight query outlines and selected excerpts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use mant_ir::{
    Block, DefinitionCase, DefinitionItem, DefinitionRole, Diagnostic, DocumentMeta,
    DocumentSource, NodeId, Section, TldrDocument,
};

use crate::{NodePath, Producer};

/// Exact schema marker for a query outline response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum OutlineSchema {
    /// Version 7 of the outline protocol.
    #[serde(rename = "mant.outline/v7")]
    V7,
}

/// Amount of semantic detail included in an outline projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OutlineDetail {
    /// Include only section-level navigation nodes.
    Sections,
    /// Include sections and semantic definition entries.
    Entries,
}

/// A block-free tree used to discover selectable query content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:outline:v7"))]
pub struct QueryOutline {
    /// Exact response schema discriminator.
    pub schema: OutlineSchema,
    /// Detail level used to build this projection.
    pub detail: OutlineDetail,
    /// Human-readable selected-document label.
    pub label: String,
    /// Authoritative document source, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
    /// Document metadata, when an authoritative document was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    /// Recoverable parser findings available to diagnostic-oriented transports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// False when semantic-entry declarations were rejected during lowering.
    ///
    /// The field is omitted for complete outlines so compact transports pay no
    /// steady-state bandwidth cost.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub entries_complete: bool,
    /// Addressable nodes in document order.
    pub nodes: Vec<OutlineNode>,
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
    rename_all_fields = "camelCase"
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
    },
    /// One semantic document section.
    DocumentSection {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local section identity.
        id: NodeId,
        /// Section heading text.
        title: String,
        /// Nested section and entry nodes.
        children: Vec<OutlineNode>,
    },
    /// One semantic command, option, or variable definition.
    DocumentEntry {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local entry identity.
        id: NodeId,
        /// Primary display term.
        title: String,
        /// Semantic category of the entry.
        role: DefinitionRole,
        /// Alias case-matching policy.
        case: DefinitionCase,
        /// Normalized selectable aliases.
        names: Vec<String>,
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
            Self::DocumentSection { children, .. } => children,
            Self::Tldr { .. } | Self::DocumentRoot { .. } | Self::DocumentEntry { .. } => &[],
        }
    }
}

/// Exact schema marker for selected query content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ExcerptSchema {
    /// Version 7 of the excerpt protocol.
    #[serde(rename = "mant.excerpt/v7")]
    V7,
}

/// One or more independently selected nodes from a complete query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:excerpt:v7"))]
pub struct QueryExcerpt {
    /// Exact response schema discriminator.
    pub schema: ExcerptSchema,
    /// Human-readable selected-document label.
    pub label: String,
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
    /// Selected nodes in request order.
    pub selections: Vec<ExcerptSelection>,
}

/// One selected document node together with its location in the complete outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ExcerptSelection {
    /// Optional quick-reference content preceding the primary document.
    Tldr {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local identity.
        id: NodeId,
        /// Display title.
        title: String,
        /// Complete quick-reference content.
        document: TldrDocument,
    },
    /// Complete document content that appears before the first heading.
    DocumentRoot {
        /// Canonical structural outline path.
        path: NodePath,
        /// Virtual document-root identity.
        id: NodeId,
        /// Display title for the leading content.
        title: String,
        /// Complete leading blocks.
        blocks: Vec<Block>,
    },
    /// Complete selected document node, including all descendant sections.
    DocumentSection {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local section identity.
        id: NodeId,
        /// Section heading text.
        title: String,
        /// Ordered ancestors from the document root to the parent section.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        breadcrumbs: Vec<OutlineReference>,
        /// Complete selected section including descendants.
        section: Section,
    },
    /// One addressable semantic definition and its complete description.
    DocumentEntry {
        /// Canonical structural outline path.
        path: NodePath,
        /// Stable document-local entry identity.
        id: NodeId,
        /// Primary display term.
        title: String,
        /// Ordered containing sections from outermost to innermost.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        breadcrumbs: Vec<OutlineReference>,
        /// Complete semantic definition.
        entry: DefinitionItem,
    },
}

/// Compact ancestor identity attached to an excerpt selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutlineReference {
    /// Canonical structural outline path.
    pub path: NodePath,
    /// Stable document-local identity.
    pub id: NodeId,
    /// Display title.
    pub title: String,
}
