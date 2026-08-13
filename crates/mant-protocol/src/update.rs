//! Process result contracts for explicit cache mutations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
            serde_json::to_value(update).expect("serialize update"),
            json!({
                "action": "cloned",
                "cacheDir": "/cache/mant/tldr-pages",
                "revision": "abc123"
            })
        );
    }
}
