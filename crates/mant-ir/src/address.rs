//! Stable logical identities for documents independent from storage paths.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Return whether a value is a conventional native manual section.
///
/// Numeric sections may carry an ASCII-alphanumeric extension such as `1p`
/// or `3type`; the historical single-letter `l` and `n` sections are also
/// accepted. The 16-byte bound matches `ManT`'s public selector boundary.
#[must_use]
pub fn is_manual_section(value: &str) -> bool {
    if value.is_empty() || value.len() > 16 {
        return false;
    }
    let mut characters = value.chars();
    match characters.next() {
        Some(first) if first.is_ascii_digit() => {
            characters.all(|character| character.is_ascii_alphanumeric())
        }
        Some('l' | 'n') => characters.next().is_none(),
        _ => false,
    }
}

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
    /// Parse one complete catalog path into its logical document address.
    ///
    /// Accepted paths are `documents/<path>`, `sources/<source>/<path>`, and
    /// `manual/<section>/<name>`. Physical filesystem paths are never
    /// interpreted here.
    #[must_use]
    pub fn parse_catalog_path(value: &str) -> Option<Self> {
        if let Some(path) = value.strip_prefix("documents/")
            && !path.is_empty()
        {
            return Some(Self::Markdown {
                path: path.to_owned(),
                origin: MarkdownOrigin::Documents,
            });
        }
        if let Some(rest) = value.strip_prefix("sources/") {
            let (source, path) = rest.split_once('/')?;
            if !source.is_empty() && !path.is_empty() {
                return Some(Self::Markdown {
                    path: path.to_owned(),
                    origin: MarkdownOrigin::Source {
                        name: source.to_owned(),
                    },
                });
            }
        }
        if let Some(rest) = value.strip_prefix("manual/") {
            let (manual_section, name) = rest.split_once('/')?;
            if !manual_section.is_empty() && !name.is_empty() && !name.contains('/') {
                return Some(Self::Manual {
                    name: name.to_owned(),
                    manual_section: manual_section.to_owned(),
                });
            }
        }
        None
    }

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

    /// Resolve an extension-free relative Markdown document reference inside
    /// the current registered namespace.
    ///
    /// The result never crosses from personal documents into a configured
    /// source, or between configured sources. References that would escape the
    /// namespace root are rejected.
    #[must_use]
    pub fn resolve_document_reference(&self, reference: &str) -> Option<Self> {
        let Self::Markdown { path, origin } = self else {
            return None;
        };
        let mut components = path.split('/').collect::<Vec<_>>();
        components.pop();
        for component in reference.split('/') {
            match component {
                "." => {}
                ".." => {
                    components.pop()?;
                }
                value if !value.is_empty() => components.push(value),
                _ => return None,
            }
        }
        (!components.is_empty()).then(|| Self::Markdown {
            path: components.join("/"),
            origin: origin.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentAddress, MarkdownOrigin};

    #[test]
    fn catalog_paths_round_trip_through_logical_addresses() {
        for address in [
            DocumentAddress::Markdown {
                path: "guides/mant".to_owned(),
                origin: MarkdownOrigin::Documents,
            },
            DocumentAddress::Markdown {
                path: "Get-Item".to_owned(),
                origin: MarkdownOrigin::Source {
                    name: "pwsh".to_owned(),
                },
            },
            DocumentAddress::Manual {
                name: "git".to_owned(),
                manual_section: "1".to_owned(),
            },
        ] {
            assert_eq!(
                DocumentAddress::parse_catalog_path(&address.catalog_path()),
                Some(address)
            );
        }
    }

    #[test]
    fn malformed_catalog_paths_are_not_interpreted_as_addresses() {
        for value in [
            "git",
            "documents/",
            "sources/pwsh",
            "sources//Get-Item",
            "manual/1",
            "manual//git",
            "manual/1/git/add",
        ] {
            assert_eq!(DocumentAddress::parse_catalog_path(value), None, "{value}");
        }
    }

    #[test]
    fn markdown_references_remain_inside_their_registered_namespace() {
        let current = DocumentAddress::Markdown {
            path: "guides/git/start".to_owned(),
            origin: MarkdownOrigin::Source {
                name: "tooling".to_owned(),
            },
        };
        assert_eq!(
            current.resolve_document_reference("../reference/options"),
            Some(DocumentAddress::Markdown {
                path: "guides/reference/options".to_owned(),
                origin: MarkdownOrigin::Source {
                    name: "tooling".to_owned(),
                },
            })
        );
        assert_eq!(current.resolve_document_reference("../../../escape"), None);
        assert_eq!(current.resolve_document_reference("/absolute"), None);
    }
}
