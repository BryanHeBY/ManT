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
    #[serde(rename = "mant.outline/v7")]
    V7,
}

/// Amount of semantic detail included in an outline projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OutlineDetail {
    Sections,
    Entries,
}

/// A block-free tree used to discover selectable query content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:outline:v7"))]
pub struct QueryOutline {
    pub schema: OutlineSchema,
    pub detail: OutlineDetail,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
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
    Tldr {
        path: NodePath,
        id: NodeId,
        title: String,
    },
    /// Addressable document content that precedes the first heading.
    DocumentRoot {
        path: NodePath,
        id: NodeId,
        title: String,
    },
    DocumentSection {
        path: NodePath,
        id: NodeId,
        title: String,
        children: Vec<OutlineNode>,
    },
    DocumentEntry {
        path: NodePath,
        id: NodeId,
        title: String,
        role: DefinitionRole,
        case: DefinitionCase,
        names: Vec<String>,
    },
}

impl OutlineNode {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Tldr { path, .. }
            | Self::DocumentRoot { path, .. }
            | Self::DocumentSection { path, .. }
            | Self::DocumentEntry { path, .. } => path,
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Tldr { id, .. }
            | Self::DocumentRoot { id, .. }
            | Self::DocumentSection { id, .. }
            | Self::DocumentEntry { id, .. } => id,
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Tldr { title, .. }
            | Self::DocumentRoot { title, .. }
            | Self::DocumentSection { title, .. }
            | Self::DocumentEntry { title, .. } => title,
        }
    }

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
    #[serde(rename = "mant.excerpt/v7")]
    V7,
}

/// One or more independently selected nodes from a complete query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:excerpt:v7"))]
pub struct QueryExcerpt {
    pub schema: ExcerptSchema,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
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
        path: NodePath,
        id: NodeId,
        title: String,
        document: TldrDocument,
    },
    /// Complete document content that appears before the first heading.
    DocumentRoot {
        path: NodePath,
        id: NodeId,
        title: String,
        blocks: Vec<Block>,
    },
    /// Complete selected document node, including all descendant sections.
    DocumentSection {
        path: NodePath,
        id: NodeId,
        title: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        breadcrumbs: Vec<OutlineReference>,
        section: Section,
    },
    /// One addressable semantic definition and its complete description.
    DocumentEntry {
        path: NodePath,
        id: NodeId,
        title: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        breadcrumbs: Vec<OutlineReference>,
        entry: DefinitionItem,
    },
}

/// Compact ancestor identity attached to an excerpt selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutlineReference {
    pub path: NodePath,
    pub id: NodeId,
    pub title: String,
}
