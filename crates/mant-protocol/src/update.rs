//! Process result contracts for explicit cache mutations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Exact schema marker for an explicit tldr cache update result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TldrCacheUpdateSchema {
    /// Version 1 of the tldr cache maintenance result.
    #[serde(rename = "mant.tldr-update/v1")]
    V1,
}

impl TldrCacheUpdateSchema {
    /// Serialized identifier of the current result contract.
    pub const ID: &'static str = "mant.tldr-update/v1";
}

/// How an explicit tldr cache refresh changed local state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TldrCacheAction {
    /// A cache did not exist and was cloned.
    Cloned,
    /// An existing cache advanced or was refreshed.
    Updated,
}

/// Result of an explicit `mant --update-tldr` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:tldr-update:v1"))]
pub struct TldrCacheUpdate {
    /// Exact response schema discriminator.
    pub schema: TldrCacheUpdateSchema,
    /// Mutation performed by the client-specific update path.
    pub action: TldrCacheAction,
    /// Updated cache directory, when the client exposes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<String>,
    /// External tldr client used for the operation, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// Trimmed human-readable client output, when useful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Resulting cache revision, when discoverable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{TldrCacheAction, TldrCacheUpdate, TldrCacheUpdateSchema};

    #[test]
    fn cache_update_uses_a_stable_camel_case_shape() {
        let update = TldrCacheUpdate {
            schema: TldrCacheUpdateSchema::V1,
            action: TldrCacheAction::Cloned,
            cache_dir: Some("/cache/mant/tldr-pages".to_owned()),
            client: None,
            output: None,
            revision: Some("abc123".to_owned()),
        };

        assert_eq!(
            serde_json::to_value(update).expect("serialize update"),
            json!({
                "schema": "mant.tldr-update/v1",
                "action": "cloned",
                "cacheDir": "/cache/mant/tldr-pages",
                "revision": "abc123"
            })
        );
    }
}
