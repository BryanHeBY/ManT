//! Discovers hierarchical local Markdown documents from the user data directory.

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use super::{
    SOURCE_METADATA_FILE, SourceConfig, SourceConfigError, is_source_name, load_source_config,
};

const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];
const MAX_DOCUMENT_DEPTH: usize = 32;
const MAX_REGISTERED_DOCUMENTS: usize = 10_000;

/// Storage class for one registered Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisteredDocumentOrigin {
    /// A file inside the singular user `documents` tree.
    Documents,
    /// A file installed from one configured source.
    Source(String),
}

/// One Markdown document registered in `ManT`'s document namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredDocument {
    /// Extension-free path relative to this document's origin.
    pub logical_path: String,
    /// Absolute path of the readable Markdown file.
    pub path: PathBuf,
    /// Storage namespace used for precedence and explicit selection.
    pub origin: RegisteredDocumentOrigin,
    /// Configured priority relative to native manuals, or `None` for `documents/`.
    pub source_priority: Option<i32>,
}

/// Immutable snapshot of the configured Markdown document namespace.
///
/// Loading the snapshot reads `sources.toml` and scans each eligible
/// directory exactly once. Candidate fallback therefore changes only lookup
/// order; it never repeats filesystem discovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisteredDocumentIndex {
    config: SourceConfig,
    documents: Vec<RegisteredDocument>,
    ready_sources: std::collections::BTreeSet<String>,
}

impl RegisteredDocumentIndex {
    /// Load the current user's registered Markdown namespace.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform data root or `sources.toml` is invalid.
    pub fn load() -> Result<Self, SourceConfigError> {
        let (paths, config) = load_source_config()?;
        let mut documents = scan_directory(&paths.documents, true)?
            .into_iter()
            .map(|(logical_path, path)| RegisteredDocument {
                logical_path,
                path,
                origin: RegisteredDocumentOrigin::Documents,
                source_priority: None,
            })
            .collect::<Vec<_>>();
        let mut ready_sources = std::collections::BTreeSet::new();
        for source in config.precedence() {
            let Some(priority) = config.get(source).map(|source| source.priority) else {
                continue;
            };
            let directory = paths.sources.join(source);
            if !source_directory_ready(&directory) {
                continue;
            }
            ready_sources.insert(source.to_owned());
            documents.extend(scan_directory(&directory, false)?.into_iter().map(
                |(logical_path, path)| RegisteredDocument {
                    logical_path,
                    path,
                    origin: RegisteredDocumentOrigin::Source(source.to_owned()),
                    source_priority: Some(priority),
                },
            ));
        }
        Ok(Self {
            config,
            documents,
            ready_sources,
        })
    }

    /// Resolve ordered path or component-suffix candidates using origin precedence.
    ///
    /// # Errors
    ///
    /// Returns an error when an explicit source is not configured.
    pub fn find(
        &self,
        candidates: &[String],
        source: Option<&str>,
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        if let Some(source) = source
            && (!is_source_name(source) || self.config.get(source).is_none())
        {
            return Err(SourceConfigError::new(format!(
                "document source '{source}' is not configured"
            )));
        }
        if let Some(source) = source
            && !self.ready_sources.contains(source)
        {
            return Err(SourceConfigError::new(format!(
                "document source '{source}' is not installed; run 'mant --update-docs' first"
            )));
        }
        self.find_matching(candidates, |document| {
            source.is_none_or(|source| {
                matches!(
                    &document.origin,
                    RegisteredDocumentOrigin::Source(candidate) if candidate == source
                )
            })
        })
    }

    /// Resolve personal documents and positive-priority configured sources.
    ///
    /// This is the portion of the namespace that precedes native manuals.
    /// Lower-priority ambiguities are deliberately not observed in this phase.
    ///
    /// # Errors
    ///
    /// Returns an error when a matching origin contains an ambiguous suffix.
    pub fn find_before_manual(
        &self,
        candidates: &[String],
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        self.find_matching(candidates, |document| {
            document.source_priority.is_none_or(|priority| priority > 0)
        })
    }

    /// Resolve configured sources at priority zero or below.
    ///
    /// This phase is consulted only after native manual lookup fails.
    ///
    /// # Errors
    ///
    /// Returns an error when a matching origin contains an ambiguous suffix.
    pub fn find_after_manual(
        &self,
        candidates: &[String],
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        self.find_matching(candidates, |document| {
            document
                .source_priority
                .is_some_and(|priority| priority <= 0)
        })
    }

    fn find_matching(
        &self,
        candidates: &[String],
        include: impl Fn(&RegisteredDocument) -> bool,
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        let origins = self
            .documents
            .iter()
            .filter(|document| include(document))
            .fold(
                Vec::<&RegisteredDocumentOrigin>::new(),
                |mut origins, document| {
                    if !origins.contains(&&document.origin) {
                        origins.push(&document.origin);
                    }
                    origins
                },
            );
        for origin in origins {
            for candidate in candidates
                .iter()
                .filter_map(|value| normalize_document_path(value))
            {
                let in_origin = self
                    .documents
                    .iter()
                    .filter(|document| include(document) && &document.origin == origin);
                if let Some(exact) = in_origin
                    .clone()
                    .find(|document| document_paths_equal(&document.logical_path, &candidate))
                {
                    return Ok(Some(exact));
                }
                let suffix = in_origin
                    .filter(|document| component_suffix_matches(&document.logical_path, &candidate))
                    .collect::<Vec<_>>();
                match suffix.as_slice() {
                    [] => {}
                    [document] => return Ok(Some(*document)),
                    _ => {
                        let choices = suffix
                            .iter()
                            .map(|document| document.logical_path.as_str())
                            .collect::<Vec<_>>()
                            .join("', '");
                        return Err(SourceConfigError::new(format!(
                            "document selector '{candidate}' is ambiguous in {}: '{choices}'",
                            origin_label(origin)
                        )));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Resolve one complete address without component-suffix fallback.
    ///
    /// # Errors
    ///
    /// Returns an error when an explicitly addressed source is not configured
    /// or has not been installed yet.
    pub fn find_address(
        &self,
        logical_path: &str,
        origin: &RegisteredDocumentOrigin,
    ) -> Result<Option<&RegisteredDocument>, SourceConfigError> {
        if let RegisteredDocumentOrigin::Source(source) = origin {
            if !is_source_name(source) || self.config.get(source).is_none() {
                return Err(SourceConfigError::new(format!(
                    "document source '{source}' is not configured"
                )));
            }
            if !self.ready_sources.contains(source) {
                return Err(SourceConfigError::new(format!(
                    "document source '{source}' is not installed; run 'mant --update-docs' first"
                )));
            }
        }
        let Some(logical_path) = normalize_document_path(logical_path) else {
            return Ok(None);
        };
        Ok(self.documents.iter().find(|document| {
            &document.origin == origin
                && document_paths_equal(&document.logical_path, &logical_path)
        }))
    }

    /// Documents in root-first and configured-source precedence order.
    #[must_use]
    pub fn documents(&self) -> &[RegisteredDocument] {
        &self.documents
    }
}

/// Find a registered document using root-first or explicit-source resolution.
///
/// # Errors
///
/// Returns an error when the platform data root or `sources.toml` is invalid.
pub fn find_registered_document_candidates(
    candidates: &[String],
    source: Option<&str>,
) -> Result<Option<RegisteredDocument>, SourceConfigError> {
    let index = RegisteredDocumentIndex::load()?;
    Ok(index.find(candidates, source)?.cloned())
}

/// List every root and configured-source candidate in fallback order.
///
/// Documents with the same logical path remain visible so callers can select
/// a source explicitly instead of losing shadowed candidates.
///
/// # Errors
///
/// Returns an error when the platform data root or `sources.toml` is invalid.
pub fn list_registered_documents() -> Result<Vec<RegisteredDocument>, SourceConfigError> {
    Ok(RegisteredDocumentIndex::load()?.documents)
}

fn source_directory_ready(directory: &Path) -> bool {
    fs::symlink_metadata(directory.join(SOURCE_METADATA_FILE))
        .is_ok_and(|metadata| metadata.file_type().is_file())
}

fn document_paths_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn component_suffix_matches(path: &str, selector: &str) -> bool {
    document_paths_equal(path, selector)
        || path.len() > selector.len()
            && path.as_bytes().get(path.len() - selector.len() - 1) == Some(&b'/')
            && document_paths_equal(&path[path.len() - selector.len()..], selector)
}

fn origin_label(origin: &RegisteredDocumentOrigin) -> String {
    match origin {
        RegisteredDocumentOrigin::Documents => "personal documents".to_owned(),
        RegisteredDocumentOrigin::Source(name) => format!("source '{name}'"),
    }
}

/// Recursively scan one origin. Symbolic links and the managed `sources`
/// subtree beneath personal documents are ignored.
fn scan_directory(
    directory: &Path,
    skip_managed_sources: bool,
) -> Result<Vec<(String, PathBuf)>, SourceConfigError> {
    let mut candidates = BTreeMap::<String, (u8, PathBuf)>::new();
    scan_directory_into(
        directory,
        directory,
        skip_managed_sources,
        0,
        &mut candidates,
    )?;
    Ok(candidates
        .into_iter()
        .map(|(name, (_, path))| (name, path))
        .collect())
}

fn scan_directory_into(
    root: &Path,
    directory: &Path,
    skip_managed_sources: bool,
    depth: usize,
    candidates: &mut BTreeMap<String, (u8, PathBuf)>,
) -> Result<(), SourceConfigError> {
    if depth > MAX_DOCUMENT_DEPTH {
        return Err(SourceConfigError::new(format!(
            "document hierarchy exceeds {MAX_DOCUMENT_DEPTH} directory levels below '{}'",
            root.display()
        )));
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if !(skip_managed_sources && entry.path() == root.join("sources")) {
                scan_directory_into(root, &entry.path(), false, depth + 1, candidates)?;
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = markdown_document_path(root, &path) else {
            continue;
        };
        let priority = markdown_extension_priority(&path).expect("name checks extension");
        if !candidates.contains_key(&name) && candidates.len() == MAX_REGISTERED_DOCUMENTS {
            return Err(SourceConfigError::new(format!(
                "document hierarchy exceeds {MAX_REGISTERED_DOCUMENTS} Markdown files below '{}'",
                root.display()
            )));
        }
        let candidate = candidates.entry(name).or_insert((priority, path.clone()));
        if priority < candidate.0 {
            *candidate = (priority, path);
        }
    }
    Ok(())
}

fn normalize_document_path(document: &str) -> Option<String> {
    let path = Path::new(document.trim());
    let components = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().filter(|value| {
                !value.is_empty()
                    && !value.contains(['/', '\\'])
                    && !value.chars().any(char::is_control)
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!components.is_empty()).then(|| components.join("/"))
}

fn markdown_document_path(root: &Path, path: &Path) -> Option<String> {
    markdown_extension_priority(path)?;
    let relative = path.strip_prefix(root).ok()?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let logical = parent.join(path.file_stem()?);
    let logical = normalize_document_path(logical.to_str()?)?;
    Some(logical)
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
    use super::{
        RegisteredDocument, RegisteredDocumentIndex, RegisteredDocumentOrigin, scan_directory,
    };

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-document-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn discovery_is_hierarchical_and_markdown_only() {
        let root = temporary_root("flat");
        fs::create_dir_all(root.join("nested")).expect("create directories");
        fs::write(root.join("alpha.md"), "# alpha").expect("write alpha");
        fs::write(root.join("beta.markdown"), "# beta").expect("write beta");
        fs::write(root.join("ignored.txt"), "ignored").expect("write text");
        fs::write(root.join("nested/hidden.md"), "# hidden").expect("write nested");

        let documents = scan_directory(&root, false).expect("scan documents");
        assert_eq!(
            documents
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta", "nested/hidden"]
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn md_wins_over_markdown_for_the_same_logical_path() {
        let root = temporary_root("extension");
        fs::create_dir_all(&root).expect("create directory");
        fs::write(root.join("tool.markdown"), "long").expect("write markdown");
        fs::write(root.join("tool.md"), "short").expect("write md");
        let documents = scan_directory(&root, false).expect("scan documents");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].1, root.join("tool.md"));
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn source_origin_carries_the_selector_name() {
        let origin = RegisteredDocumentOrigin::Source("rust".to_owned());
        assert_eq!(origin, RegisteredDocumentOrigin::Source("rust".to_owned()));
    }

    #[test]
    fn exact_paths_precede_unique_suffixes_and_collisions_are_explicit() {
        let index = RegisteredDocumentIndex {
            config: SourceConfig::default(),
            documents: vec![
                RegisteredDocument {
                    logical_path: "languages/en/tool".to_owned(),
                    path: PathBuf::from("/documents/languages/en/tool.md"),
                    origin: RegisteredDocumentOrigin::Documents,
                    source_priority: None,
                },
                RegisteredDocument {
                    logical_path: "languages/zh/tool".to_owned(),
                    path: PathBuf::from("/documents/languages/zh/tool.md"),
                    origin: RegisteredDocumentOrigin::Documents,
                    source_priority: None,
                },
            ],
            ready_sources: std::collections::BTreeSet::default(),
        };
        let exact = index
            .find(&["languages/en/tool".to_owned()], None)
            .expect("exact lookup")
            .expect("exact document");
        assert_eq!(exact.logical_path, "languages/en/tool");
        let error = index
            .find(&["tool".to_owned()], None)
            .expect_err("leaf selector must be ambiguous");
        assert!(error.to_string().contains("languages/en/tool"));
        assert!(error.to_string().contains("languages/zh/tool"));
    }

    #[test]
    fn fallback_ambiguity_does_not_leak_into_the_before_manual_phase() {
        let index = RegisteredDocumentIndex {
            config: SourceConfig::default(),
            documents: ["languages/en/tool", "languages/zh/tool"]
                .into_iter()
                .map(|logical_path| RegisteredDocument {
                    logical_path: logical_path.to_owned(),
                    path: PathBuf::from(format!("/sources/fallback/{logical_path}.md")),
                    origin: RegisteredDocumentOrigin::Source("fallback".to_owned()),
                    source_priority: Some(-1),
                })
                .collect(),
            ready_sources: std::collections::BTreeSet::default(),
        };

        assert_eq!(
            index
                .find_before_manual(&["tool".to_owned()])
                .expect("preferred phase"),
            None
        );
        assert!(
            index
                .find_after_manual(&["tool".to_owned()])
                .expect_err("fallback remains ambiguous")
                .to_string()
                .contains("source 'fallback'")
        );
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
        assert!(super::document_paths_equal("cargo.exe", "cargo.EXE"));
    }
}
