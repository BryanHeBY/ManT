//! Structured tldr content kept distinct from authoritative manual pages.

use serde::{Deserialize, Serialize};

/// One cached tldr page included in a query bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TldrDocument {
    pub title: String,
    pub description: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more_information: Option<String>,
    pub examples: Vec<TldrExample>,
    pub platform: String,
    pub language: String,
    pub source_path: String,
}

/// Human explanation paired with one shell command example.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TldrExample {
    pub description: String,
    pub command: String,
    pub command_parts: Vec<TldrCommandPart>,
}

/// Styled command fragment used by the TUI to distinguish placeholders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TldrCacheAction {
    Cloned,
    Updated,
}

/// Result of an explicit `mant-cli --update-tldr` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    use super::{TldrCacheAction, TldrCacheUpdate};

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
}
