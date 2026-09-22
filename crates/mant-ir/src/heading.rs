//! Authoritative displayed headings, independent of bibliographic labels.

use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ContentContext, Document, Inline, SourceSpan};

/// One visible heading with the same inline vocabulary as document prose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Heading {
    /// Original inline content. Derived plain labels are not a second authority.
    pub content: Vec<Inline>,
    /// Actual heading provenance, when provided by the producer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

impl Heading {
    /// Derive the displayed words without losing the authoritative inline nodes.
    ///
    /// # Panics
    ///
    /// Panics if the heading refers to content absent from `content`.
    #[must_use]
    pub fn plain_text(&self, content: ContentContext<'_>) -> String {
        content
            .plain_text(&self.content)
            .expect("heading content must resolve in its store")
    }

    /// Derive a safe one-line label for outlines, breadcrumbs, and tabs.
    ///
    /// Structural inline breaks remain authoritative in the heading content,
    /// but a tree label cannot embed rows. Whitespace is normalized at that
    /// presentation boundary instead of being mistaken for tree layout.
    #[must_use]
    pub fn single_line_text(&self, content: ContentContext<'_>) -> String {
        self.plain_text(content)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Document {
    /// Derive a display label from visible content, otherwise native metadata.
    /// This does not manufacture a heading for a native manual.
    #[must_use]
    pub fn display_title(&self) -> Option<Cow<'_, str>> {
        self.heading
            .as_ref()
            .and_then(|heading| {
                self.content()
                    .heading_plain_text(heading)
                    .ok()
                    .map(Cow::Owned)
            })
            .or_else(|| self.meta.title.as_deref().map(Cow::Borrowed))
    }
}
