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
