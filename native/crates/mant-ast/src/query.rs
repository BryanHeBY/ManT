//! Query envelope combining authoritative manuals with optional tldr content.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{MantDocument, TldrDocument};

/// Exact schema marker for a complete `ManT` query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum QuerySchema {
    /// First query envelope built around `mant.document/v1`.
    #[serde(rename = "mant.query/v1")]
    V1,
}

/// Exact schema marker for a native query request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RequestSchema {
    /// First request contract accepted through `--request-json`.
    #[serde(rename = "mant.request/v1")]
    V1,
}

/// Validated use-case input accepted by the native query boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:request:v1"))]
pub struct QueryRequest {
    pub schema: RequestSchema,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// Native result consumed by JSON, Markdown, and interactive frontends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:query:v1"))]
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
