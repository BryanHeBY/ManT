//! Structured tldr content kept distinct from authoritative manual pages.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One normalized quick reference included in a query bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TldrDocument {
    pub title: String,
    /// `CommonMark` paragraphs from the page's leading block quote.
    ///
    /// Source-only soft line breaks are normalized to spaces inside each
    /// element; distinct paragraphs remain distinct elements.
    pub description: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more_information: Option<String>,
    pub examples: Vec<TldrExample>,
    pub platform: String,
    pub language: String,
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
    #[default]
    TldrPages,
    Embedded,
}

impl TldrOrigin {
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
    pub command: String,
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
    Text { value: String },
    Placeholder { value: String },
}

/// How an explicit tldr cache refresh changed local state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TldrCacheAction {
    Cloned,
    Updated,
}

/// Result of an explicit `mant --update-tldr` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TldrCacheUpdate {
    pub action: TldrCacheAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{TldrCacheAction, TldrCacheUpdate, TldrDocument, TldrOrigin};

    #[test]
    fn cache_update_uses_a_stable_camel_case_shape() {
        let update = TldrCacheUpdate {
            action: TldrCacheAction::Cloned,
            cache_dir: Some("/cache/mant/tldr-pages".to_owned()),
            client: None,
            output: None,
            revision: Some("abc123".to_owned()),
        };

        assert_eq!(
            serde_json::to_value(update).expect("serialize cache update"),
            json!({
                "action": "cloned",
                "cacheDir": "/cache/mant/tldr-pages",
                "revision": "abc123"
            })
        );
    }

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
    }
}
