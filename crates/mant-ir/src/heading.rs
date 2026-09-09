//! Authoritative displayed headings, independent of bibliographic labels.

use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Document, Inline, SourceSpan};

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
    #[must_use]
    pub fn plain_text(&self) -> String {
        fn append(nodes: &[Inline], output: &mut String) {
            for node in nodes {
                match node {
                    Inline::Text { value } | Inline::Code { value } => output.push_str(value),
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => append(children, output),
                    Inline::LineBreak => output.push('\n'),
                    Inline::Anchor { .. } => {}
                }
            }
        }
        let mut output = String::new();
        append(&self.content, &mut output);
        output
    }
}

impl From<String> for Heading {
    fn from(value: String) -> Self {
        Self {
            content: vec![Inline::Text { value }],
            source: None,
        }
    }
}

impl From<&str> for Heading {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl Document {
    /// Derive a display label from visible content, otherwise native metadata.
    /// This does not manufacture a heading for a native manual.
    #[must_use]
    pub fn display_title(&self) -> Option<Cow<'_, str>> {
        self.heading
            .as_ref()
            .map(|heading| Cow::Owned(heading.plain_text()))
            .or_else(|| self.meta.title.as_deref().map(Cow::Borrowed))
    }
}
