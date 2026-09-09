//! Stable document nodes independent from their source parser.
use crate::{Heading, NodeId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod blocks;
mod diagnostic;
mod inline;
mod source;
pub use blocks::{
    Block, DefinitionItem, DefinitionLayout, LayoutHint, ListItem, ListItemLayout, ListKind,
    TableAlignment, TableCell, TableRow,
};
pub use diagnostic::{Diagnostic, DiagnosticImpact, DiagnosticLevel, semantics_complete};
pub use inline::{Inline, LinkTarget};
pub use source::{DocumentSource, SourceFormat, SourceSpan, TextRange, TextSize};

/// A normalized document ready for interactive or textual rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    /// Parser provenance retained independently from process-protocol metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parser: Option<ParserInfo>,
    /// Original source format and stable path.
    pub source: DocumentSource,
    /// Metadata normalized across all supported source formats.
    pub meta: DocumentMeta,
    /// Original visible document heading, distinct from native bibliographic metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<Heading>,
    /// Exact source fragments resolving to the normalized document root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<crate::FragmentAlias>,
    /// Recoverable findings retained for callers that need source quality data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Content preceding the first section heading.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Top-level semantic sections in source order.
    pub sections: Vec<Section>,
}

/// Parser implementation that produced this normalized document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParserInfo {
    /// Stable parser implementation name.
    pub name: String,
    /// Parser version used to produce the document.
    pub version: String,
}

/// Metadata normalized from TH, Dt, and the validated libmandoc result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    /// Native bibliographic title; Markdown visible titles belong to `Document.heading`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Native manual category such as `1` or `3p`; unrelated to document headings.
    pub manual_section: Option<String>,
    /// Normalized publication or revision date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Manual volume or collection label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    /// Operating-system label declared by the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Architecture qualifier declared by the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// Primary name followed by any aliases from the NAME section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// Logical target of a native `.so` alias page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_target: Option<String>,
}

/// Source-neutral document section headed by Markdown, man, or mdoc content.
///
/// This is a content subtree, not the native manual category stored in
/// [`DocumentMeta::manual_section`]. Depth is derived from tree position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Section {
    /// Unique within one document; consumers must not treat it as a global ID.
    pub id: NodeId,
    /// Exact source fragments resolving to this normalized section identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<crate::FragmentAlias>,
    /// Authoritative visible heading content, including links and styles.
    pub heading: Heading,
    /// Terminal rows requested before this heading by the source macro set.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub spacing_before_lines: u16,
    /// Content directly owned by this section.
    pub blocks: Vec<Block>,
    /// Nested subsections in source order.
    pub children: Vec<Section>,
    /// Heading location in the original source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}
