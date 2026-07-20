//! Stable contracts for lightweight query outlines and selected excerpts.

use serde::{Deserialize, Serialize};

use crate::{Diagnostic, DocumentMeta, DocumentSource, Producer, Section, TldrDocument};

/// Exact schema marker for a query outline response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutlineSchema {
    #[serde(rename = "mant.outline/v1")]
    V1,
}

/// A block-free tree used to discover selectable query content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryOutline {
    pub schema: OutlineSchema,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    pub nodes: Vec<OutlineNode>,
}

/// Kind of document content addressed by an outline node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutlineNodeKind {
    Tldr,
    ManualSection,
}

/// One uniquely addressable node in a query outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineNode {
    pub kind: OutlineNodeKind,
    /// Human-oriented tree coordinate. Manual sections are one-based, while
    /// the synthetic quick reference uses `0` because it precedes the manual.
    pub path: String,
    /// Renderer-neutral ID unique within the source document.
    pub id: String,
    pub title: String,
    pub children: Vec<OutlineNode>,
}

/// Exact schema marker for selected query content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExcerptSchema {
    #[serde(rename = "mant.excerpt/v1")]
    V1,
}

/// One or more independently selected nodes from a complete query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryExcerpt {
    pub schema: ExcerptSchema,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ExcerptSelection {
    /// Optional quick-reference content preceding the authoritative manual.
    Tldr {
        path: String,
        id: String,
        title: String,
        document: TldrDocument,
    },
    /// Complete selected manual node, including all descendant sections.
    ManualSection {
        path: String,
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        breadcrumbs: Vec<OutlineReference>,
        section: Section,
    },
}

/// Compact ancestor identity attached to an excerpt selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineReference {
    pub path: String,
    pub id: String,
    pub title: String,
}
