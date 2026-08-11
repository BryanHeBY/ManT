//! Discovers flat local Markdown documents from the user data directory.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use super::{SOURCE_METADATA_FILE, SourceConfigError, is_source_name, load_source_config};

const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

/// Storage class for one registered Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisteredDocumentOrigin {
    /// A file directly inside the singular user `documents` directory.
    Documents,
    /// A file installed from one configured source.
    Source(String),
}

/// One Markdown document registered in `ManT`'s document namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredDocument {
    pub name: String,
    pub path: PathBuf,
    pub origin: RegisteredDocumentOrigin,
}

/// Find a registered document using root-first or explicit-source resolution.
///
/// # Errors
///
/// Returns an error when the platform data root or `sources.toml` is invalid.
pub fn find_registered_document(
    document: &str,
    source: Option<&str>,
) -> Result<Option<RegisteredDocument>, SourceConfigError> {
    let document = document.trim();
    if !is_safe_document_name(document) {
        return Ok(None);
    }
    let (paths, config) = load_source_config()?;
    if let Some(source) = source {
        if !is_source_name(source) || config.get(source).is_none() {
            return Err(SourceConfigError::new(format!(
                "document source '{source}' is not configured"
            )));
        }
        let directory = paths.sources.join(source);
        if !source_directory_ready(&directory) {
            return Ok(None);
        }
        return Ok(
            find_in_directory(&directory, document).map(|path| RegisteredDocument {
                name: document.to_owned(),
                path,
                origin: RegisteredDocumentOrigin::Source(source.to_owned()),
            }),
        );
    }

    if let Some(path) = find_in_directory(&paths.documents, document) {
        return Ok(Some(RegisteredDocument {
            name: document.to_owned(),
            path,
            origin: RegisteredDocumentOrigin::Documents,
        }));
    }
    for source in config.precedence() {
        let directory = paths.sources.join(source);
        if source_directory_ready(&directory)
            && let Some(path) = find_in_directory(&directory, document)
        {
            return Ok(Some(RegisteredDocument {
                name: document.to_owned(),
                path,
                origin: RegisteredDocumentOrigin::Source(source.to_owned()),
            }));
        }
    }
    Ok(None)
}

/// List every root and configured-source candidate in fallback order.
///
/// Documents with the same public name remain visible so callers can select a
/// source explicitly instead of losing shadowed candidates.
///
/// # Errors
///
/// Returns an error when the platform data root or `sources.toml` is invalid.
pub fn list_registered_documents() -> Result<Vec<RegisteredDocument>, SourceConfigError> {
    let (paths, config) = load_source_config()?;
    let mut documents = scan_directory(&paths.documents)
        .into_iter()
        .map(|(name, path)| RegisteredDocument {
            name,
            path,
            origin: RegisteredDocumentOrigin::Documents,
        })
        .collect::<Vec<_>>();
    for source in config.precedence() {
        let directory = paths.sources.join(source);
        if !source_directory_ready(&directory) {
            continue;
        }
        documents.extend(scan_directory(&directory).into_iter().map(|(name, path)| {
            RegisteredDocument {
                name,
                path,
                origin: RegisteredDocumentOrigin::Source(source.to_owned()),
            }
        }));
    }
    Ok(documents)
}

fn source_directory_ready(directory: &Path) -> bool {
    fs::symlink_metadata(directory.join(SOURCE_METADATA_FILE))
        .is_ok_and(|metadata| metadata.file_type().is_file())
}

fn find_in_directory(directory: &Path, document: &str) -> Option<PathBuf> {
    scan_directory(directory)
        .into_iter()
        .find_map(|(name, path)| document_names_equal(&name, document).then_some(path))
}

fn document_names_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// Scan exactly one directory. Directories and symbolic links are ignored.
fn scan_directory(directory: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut candidates = BTreeMap::<String, (u8, PathBuf)>::new();
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = markdown_document_name(&path) else {
            continue;
        };
        let priority = markdown_extension_priority(&path).expect("name checks extension");
        let candidate = candidates.entry(name).or_insert((priority, path.clone()));
        if priority < candidate.0 {
            *candidate = (priority, path);
        }
    }
    candidates
        .into_iter()
        .map(|(name, (_, path))| (name, path))
        .collect()
}

fn is_safe_document_name(document: &str) -> bool {
    !document.is_empty()
        && Path::new(document)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && Path::new(document).file_name() == Some(OsStr::new(document))
}

fn markdown_document_name(path: &Path) -> Option<String> {
    markdown_extension_priority(path)?;
    let name = path.file_stem()?.to_str()?;
    is_safe_document_name(name).then(|| name.to_owned())
}

fn markdown_extension_priority(path: &Path) -> Option<u8> {
    let extension = path.extension()?.to_str()?;
    MARKDOWN_EXTENSIONS
        .iter()
        .position(|candidate| extension.eq_ignore_ascii_case(candidate))
        .and_then(|index| u8::try_from(index).ok())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf};

    use super::super::{ConfiguredSource, SourceConfig, SourceLocation};
    use super::{RegisteredDocumentOrigin, scan_directory};

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-document-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn discovery_is_flat_and_markdown_only() {
        let root = temporary_root("flat");
        fs::create_dir_all(root.join("nested")).expect("create directories");
        fs::write(root.join("alpha.md"), "# alpha").expect("write alpha");
        fs::write(root.join("beta.markdown"), "# beta").expect("write beta");
        fs::write(root.join("ignored.txt"), "ignored").expect("write text");
        fs::write(root.join("nested/hidden.md"), "# hidden").expect("write nested");

        let documents = scan_directory(&root);
        assert_eq!(
            documents
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn md_wins_over_markdown_for_the_same_public_name() {
        let root = temporary_root("extension");
        fs::create_dir_all(&root).expect("create directory");
        fs::write(root.join("tool.markdown"), "long").expect("write markdown");
        fs::write(root.join("tool.md"), "short").expect("write md");
        let documents = scan_directory(&root);
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].1, root.join("tool.md"));
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn source_origin_carries_the_selector_name() {
        let origin = RegisteredDocumentOrigin::Source("rust".to_owned());
        assert_eq!(origin, RegisteredDocumentOrigin::Source("rust".to_owned()));
    }

    // Keep the imported schema types exercised here so changes to their shape
    // remain deliberate alongside discovery precedence tests.
    #[test]
    fn source_priority_is_signed() {
        let mut values = BTreeMap::new();
        values.insert(
            "docs".to_owned(),
            ConfiguredSource {
                location: SourceLocation::Git {
                    repo: "repo".to_owned(),
                    branch: "main".to_owned(),
                },
                path: ".".to_owned(),
                include: Vec::new(),
                exclude: Vec::new(),
                priority: -1,
            },
        );
        let _ = std::mem::size_of::<SourceConfig>();
        assert_eq!(values["docs"].priority, -1);
    }

    #[cfg(windows)]
    #[test]
    fn windows_registered_names_are_ascii_case_insensitive() {
        assert!(super::document_names_equal("cargo.exe", "cargo.EXE"));
    }
}
