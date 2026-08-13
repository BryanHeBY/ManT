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
    /// The user's primary `documents` tree.
    Documents,
    /// A configured source cache.
    Source {
        /// Configured source name.
        name: String,
    },
}

/// Stable selector for one discoverable document candidate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DocumentAddress {
    /// A Markdown document registered in the document catalog.
    Markdown {
        /// Extension-free path relative to the selected Markdown origin.
        path: String,
        /// Storage namespace containing the relative path.
        origin: MarkdownOrigin,
    },
    /// An installed native manual page.
    Manual {
        /// Manual topic without its section suffix.
        name: String,
        /// Native manual category such as `1` or `3p`.
        manual_section: String,
    },
}

impl DocumentAddress {
    /// Return the basename used as the document's short lookup name.
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
            Self::Manual {
                name,
                manual_section,
            } => format!("{manual_section}/{name}"),
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
            Self::Manual {
                name,
                manual_section,
            } => format!("manual/{manual_section}/{name}"),
        }
    }
}
