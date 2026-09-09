//! Typed requests emitted when the reader asks its host to update a clipboard.

use std::sync::Arc;

use mant_ir::ResolvedContent;
use mant_protocol::ContentSelector;

/// Maximum clipboard payload accepted by the reader and its standard host.
pub const MAX_COPY_BYTES: usize = 4 * 1024 * 1024;

/// Presentation requested for one complete semantic document node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFormat {
    /// Deterministic unstyled text.
    Text,
    /// Structurally complete `CommonMark`.
    Markdown,
}

/// Clipboard content selected by the interactive reader.
#[derive(Debug, Clone)]
pub enum CopyRequest {
    /// A reference's original typed target, not the source item's body.
    Reference {
        /// Bounded terminal-safe target text selected by an explicit action.
        text: String,
    },
    /// Plain text extracted from an exact visual selection.
    Selection {
        /// Terminal-safe selected text.
        text: String,
    },
    /// One complete addressable node projected by the embedding host.
    Node {
        /// Exact in-memory content currently displayed by the reader.
        content: Arc<ResolvedContent>,
        /// Stable node identity from the document outline.
        selector: ContentSelector,
        /// Requested deterministic presentation.
        format: CopyFormat,
    },
}

impl CopyRequest {
    /// Human-readable content category for a success notice.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Reference { .. } => "reference target",
            Self::Selection { .. } => "selection",
            Self::Node {
                format: CopyFormat::Text,
                ..
            } => "node as text",
            Self::Node {
                format: CopyFormat::Markdown,
                ..
            } => "node as Markdown",
        }
    }
}
