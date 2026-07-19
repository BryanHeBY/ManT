//! Stable contracts for lightweight manual outlines and selected excerpts.

use serde::{Deserialize, Serialize};

use crate::{Diagnostic, DocumentMeta, DocumentSource, Producer, Section};

/// Exact schema marker for a manual outline response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutlineSchema {
    #[serde(rename = "mant.outline/v1")]
    V1,
}

/// A block-free tree used to discover selectable manual sections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualOutline {
    pub schema: OutlineSchema,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
    pub source: DocumentSource,
    pub meta: DocumentMeta,
    pub nodes: Vec<OutlineNode>,
}

/// One uniquely addressable node in a manual outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineNode {
    /// Human-oriented, one-based tree coordinate such as `4.2`.
    pub path: String,
    /// Renderer-neutral ID unique within the source document.
    pub id: String,
    pub title: String,
    pub children: Vec<OutlineNode>,
}

/// Exact schema marker for selected manual content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExcerptSchema {
    #[serde(rename = "mant.excerpt/v1")]
    V1,
}

/// One or more independently selected section subtrees from a manual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualExcerpt {
    pub schema: ExcerptSchema,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
    pub producer: Producer,
    pub source: DocumentSource,
    pub meta: DocumentMeta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    pub selections: Vec<ExcerptSelection>,
}

/// A selected section together with its location in the complete outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcerptSelection {
    pub path: String,
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breadcrumbs: Vec<OutlineReference>,
    /// Complete selected node, including all of its descendant sections.
    pub section: Section,
}

/// Compact ancestor identity attached to an excerpt selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineReference {
    pub path: String,
    pub id: String,
    pub title: String,
}
