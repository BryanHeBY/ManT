//! Structured tldr content kept distinct from authoritative manual pages.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One normalized quick reference included in a query bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TldrDocument {
    /// Page title, normally the command name.
    pub title: String,
    /// `CommonMark` paragraphs from the page's leading block quote.
    ///
    /// Source-only soft line breaks are normalized to spaces inside each
    /// element; distinct paragraphs remain distinct elements.
    pub description: Vec<String>,
    /// Upstream reference URL extracted from the page, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more_information: Option<String>,
    /// Command examples in display order.
    pub examples: Vec<TldrExample>,
    /// tldr platform bucket such as `common`, `linux`, or `windows`.
    pub platform: String,
    /// BCP-47-like tldr language directory, such as `en` or `zh`.
    pub language: String,
    /// Stable source path used for diagnostics and attribution.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_path: String,
    /// Distinguishes community cache data from a document-owned quick reference.
    ///
    /// The field is omitted for tldr-pages data to preserve the established
    /// wire representation. Absence therefore means [`TldrOrigin::TldrPages`].
    #[serde(default, skip_serializing_if = "TldrOrigin::is_tldr_pages")]
    pub origin: TldrOrigin,
}

/// Provenance controls attribution without changing quick-reference rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TldrOrigin {
    /// Content obtained from the community tldr-pages cache.
    #[default]
    TldrPages,
    /// Quick reference embedded in the authoritative Markdown document.
    Embedded,
}

impl TldrOrigin {
    /// Return whether this origin is the community tldr-pages project.
    #[must_use]
    pub const fn is_tldr_pages(&self) -> bool {
        matches!(self, Self::TldrPages)
    }
}

/// Human explanation paired with one shell command example.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TldrExample {
    /// One normalized prose paragraph describing the command example.
    pub description: String,
    /// Complete shell command with placeholders in their source spelling.
    pub command: String,
    /// Styled fragments whose concatenation equals [`Self::command`].
    pub command_parts: Vec<TldrCommandPart>,
}

/// Styled command fragment used by the TUI to distinguish placeholders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum TldrCommandPart {
    /// Literal command text.
    Text {
        /// Literal fragment value.
        value: String,
    },
    /// Replaceable value marked by the tldr placeholder syntax.
    Placeholder {
        /// Placeholder text without presentation styling.
        value: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{TldrDocument, TldrOrigin};

    #[test]
    fn origin_is_additive_and_omitted_for_tldr_pages() {
        let page = |origin| TldrDocument {
            title: "demo".to_owned(),
            description: Vec::new(),
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/source/demo.md".to_owned(),
            origin,
        };

        let cached = serde_json::to_value(page(TldrOrigin::TldrPages)).expect("cached page");
        assert!(cached.get("origin").is_none());
        let embedded = serde_json::to_value(page(TldrOrigin::Embedded)).expect("embedded page");
        assert_eq!(embedded["origin"], "embedded");

        let mut redacted = page(TldrOrigin::TldrPages);
        redacted.source_path.clear();
        let redacted = serde_json::to_value(redacted).expect("redacted page");
        assert!(redacted.get("sourcePath").is_none());
    }
}
