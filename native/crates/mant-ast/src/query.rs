//! Query envelope combining authoritative manuals with optional tldr content.

use serde::{Deserialize, Serialize};

use crate::{MantDocument, TldrDocument};

/// Exact schema marker for a complete Mant query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuerySchema {
    /// First query envelope built around `mant.document/v1`.
    #[serde(rename = "mant.query/v1")]
    V1,
}

/// Validated use-case input accepted by the native query boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryRequest {
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// Native result consumed by JSON, Markdown, and interactive frontends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryBundle {
    pub schema: QuerySchema,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual: Option<MantDocument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tldr: Option<TldrDocument>,
}
