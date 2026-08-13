//! Stable logical identities for documents independent from storage paths.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Storage identity of one registered Markdown document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MarkdownOrigin {
    Documents,
    Source { name: String },
}

/// Stable selector for one discoverable document candidate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DocumentAddress {
    Markdown {
        /// Extension-free path relative to the selected Markdown origin.
        path: String,
        origin: MarkdownOrigin,
    },
    Manual {
        name: String,
        section: String,
    },
}

impl DocumentAddress {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Markdown { path, .. } => path.rsplit('/').next().unwrap_or(path),
            Self::Manual { name, .. } => name,
        }
    }

    /// Stable path relative to its storage namespace.
    #[must_use]
    pub fn relative_path(&self) -> String {
        match self {
            Self::Markdown { path, .. } => path.clone(),
            Self::Manual { name, section } => format!("{section}/{name}"),
        }
    }

    /// Complete, unambiguous path in `ManT`'s unified document tree.
    #[must_use]
    pub fn catalog_path(&self) -> String {
        match self {
            Self::Markdown {
                path,
                origin: MarkdownOrigin::Documents,
            } => format!("documents/{path}"),
            Self::Markdown {
                path,
                origin: MarkdownOrigin::Source { name },
            } => format!("sources/{name}/{path}"),
            Self::Manual { name, section } => format!("manual/{section}/{name}"),
        }
    }
}
