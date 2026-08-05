//! Stable contracts for lightweight query outlines and selected excerpts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Block, DefinitionItem, DefinitionRole, Diagnostic, DocumentMeta, DocumentSource, Producer,
    Section, TldrDocument,
};

/// Exact schema marker for a query outline response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum OutlineSchema {
    #[serde(rename = "mant.outline/v3")]
    V3,
}

/// Amount of semantic detail included in an outline projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OutlineDetail {
    Sections,
    Options,
}

/// A block-free tree used to discover selectable query content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:outline:v3"))]
pub struct QueryOutline {
    pub schema: OutlineSchema,
    pub detail: OutlineDetail,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    pub nodes: Vec<OutlineNode>,
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
        path: String,
        id: String,
        title: String,
    },
    /// Addressable document content that precedes the first heading.
    DocumentRoot {
        path: String,
        id: String,
        title: String,
    },
    DocumentSection {
        path: String,
        id: String,
        title: String,
        children: Vec<OutlineNode>,
    },
    DocumentEntry {
        path: String,
        id: String,
        title: String,
        role: DefinitionRole,
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
    #[serde(rename = "mant.excerpt/v3")]
    V3,
}

/// One or more independently selected nodes from a complete query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:excerpt:v3"))]
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
        path: String,
        id: String,
        title: String,
        document: TldrDocument,
    },
    /// Complete document content that appears before the first heading.
    DocumentRoot {
        path: String,
        id: String,
        title: String,
        blocks: Vec<Block>,
    },
    /// Complete selected document node, including all descendant sections.
    DocumentSection {
        path: String,
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        breadcrumbs: Vec<OutlineReference>,
        section: Section,
    },
    /// One addressable semantic definition and its complete description.
    DocumentEntry {
        path: String,
        id: String,
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
    pub path: String,
    pub id: String,
    pub title: String,
}
