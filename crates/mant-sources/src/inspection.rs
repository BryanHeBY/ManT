//! Read-only inspection of configured and updater-owned document sources.

use std::{fs, io, path::Path};

use serde::Deserialize;

use crate::{
    ConfiguredSource, DocumentPaths, SOURCE_METADATA_FILE, SourceConfig, SourceConfigError,
    SourceLocation, is_source_name, load_source_config,
    metadata::{MAX_METADATA_BYTES, read_source_metadata, source_fingerprint},
    registry::managed_document_count,
};

/// Transport required to update one configured source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceTransport {
    /// A shallow Git checkout.
    Git,
    /// A direct HTTPS archive.
    Archive,
}

/// Local installation state of one configured source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceInstallationStatus {
    /// Installed metadata matches the active configuration.
    Ready,
    /// No installed directory exists.
    Missing,
    /// Installed metadata is valid but belongs to an older configuration.
    Stale,
    /// The installed directory or its metadata is invalid or unreadable.
    Invalid,
}

/// Read-only state of one configured source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfiguredSourceInspection {
    /// Configured source name.
    pub source: String,
    /// Acquisition transport.
    pub transport: SourceTransport,
    /// Lookup priority relative to native manuals.
    pub priority: i32,
    /// Local installation state.
    pub status: SourceInstallationStatus,
    /// Installed revision, when valid metadata is readable.
    pub revision: Option<String>,
    /// Installed document count, when valid metadata is readable.
    pub documents: Option<u32>,
    /// Failure or stale-state explanation.
    pub detail: Option<String>,
}

/// Updater-owned or suspicious entry absent from the active configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrphanedSourceInspection {
    /// Entry name below the managed source root.
    pub source: String,
    /// Platform-native entry path.
    pub path: String,
    /// Whether ownership metadata makes explicit pruning safe.
    pub removable: bool,
    /// Installed revision, when ownership metadata is trusted.
    pub revision: Option<String>,
    /// Installed document count, when ownership metadata is trusted.
    pub documents: Option<u32>,
    /// Refusal detail for suspicious entries.
    pub error: Option<String>,
}

/// Complete read-only snapshot of configured and installed sources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSourcesInspection {
    /// Effective platform paths.
    pub paths: DocumentPaths,
    /// Whether `sources.toml` exists as a filesystem entry.
    pub config_exists: bool,
    /// Configured sources in lexical name order.
    pub sources: Vec<ConfiguredSourceInspection>,
    /// Unconfigured entries in lexical name order.
    pub orphaned: Vec<OrphanedSourceInspection>,
}

/// Inspect source state without creating directories, taking an update lock,
/// invoking external programs, or accessing the network.
///
/// # Errors
///
/// Returns an error when platform paths, configuration, or the existing source
/// directory cannot be read.
pub fn inspect_document_sources() -> Result<DocumentSourcesInspection, SourceConfigError> {
    let (paths, config) = load_source_config()?;
    inspect_document_sources_from(&paths, &config)
}

pub(crate) fn inspect_document_sources_from(
    paths: &DocumentPaths,
    config: &SourceConfig,
) -> Result<DocumentSourcesInspection, SourceConfigError> {
    let sources = config
        .sources()
        .iter()
        .map(|(name, source)| inspect_configured_source(paths, name, source))
        .collect();
    let orphaned = inspect_unconfigured_sources(paths, config)?;
    Ok(DocumentSourcesInspection {
        config_exists: fs::symlink_metadata(&paths.config).is_ok(),
        paths: paths.clone(),
        sources,
        orphaned,
    })
}

fn inspect_configured_source(
    paths: &DocumentPaths,
    name: &str,
    configured: &ConfiguredSource,
) -> ConfiguredSourceInspection {
    let transport = match configured.location {
        SourceLocation::Git { .. } => SourceTransport::Git,
        SourceLocation::Archive { .. } => SourceTransport::Archive,
    };
    let target = paths.sources.join(name);
    let base = |status, revision, documents, detail| ConfiguredSourceInspection {
        source: name.to_owned(),
        transport,
        priority: configured.priority,
        status,
        revision,
        documents,
        detail,
    };
    let directory = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return base(SourceInstallationStatus::Missing, None, None, None);
        }
        Err(error) => {
            return base(
                SourceInstallationStatus::Invalid,
                None,
                None,
                Some(format!("could not inspect source directory: {error}")),
            );
        }
    };
    if directory.file_type().is_symlink() || !directory.file_type().is_dir() {
        return base(
            SourceInstallationStatus::Invalid,
            None,
            None,
            Some("source entry is not a regular directory".to_owned()),
        );
    }
    let metadata = match read_source_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) => {
            return base(SourceInstallationStatus::Invalid, None, None, Some(error));
        }
    };
    let revision = Some(metadata.revision().to_owned());
    let documents = Some(metadata.documents());
    let actual_documents = match managed_document_count(&target) {
        Ok(documents) => documents,
        Err(error) => {
            return base(
                SourceInstallationStatus::Invalid,
                revision,
                documents,
                Some(format!("could not verify installed documents: {error}")),
            );
        }
    };
    if actual_documents != metadata.documents() {
        return base(
            SourceInstallationStatus::Invalid,
            revision,
            documents,
            Some(format!(
                "installed metadata records {} documents but {} are present",
                metadata.documents(),
                actual_documents
            )),
        );
    }
    if metadata.matches(name, configured, &source_fingerprint(configured)) {
        base(SourceInstallationStatus::Ready, revision, documents, None)
    } else {
        base(
            SourceInstallationStatus::Stale,
            revision,
            documents,
            Some("installed metadata does not match the active configuration".to_owned()),
        )
    }
}

pub(crate) fn inspect_unconfigured_sources(
    paths: &DocumentPaths,
    config: &SourceConfig,
) -> Result<Vec<OrphanedSourceInspection>, SourceConfigError> {
    let mut entries = match fs::read_dir(&paths.sources) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>().map_err(|error| {
            SourceConfigError::new(format!("could not read source entry: {error}"))
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(SourceConfigError::new(format!(
                "could not read document source directory '{}': {error}",
                paths.sources.display()
            )));
        }
    };
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    let mut orphaned = Vec::new();
    for entry in entries {
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy().into_owned();
        if name == ".update.lock"
            || name_os
                .to_str()
                .is_some_and(|name| config.get(name).is_some())
        {
            continue;
        }
        let path = entry.path();
        let inspected = name_os
            .to_str()
            .ok_or_else(|| "source directory name is not UTF-8".to_owned())
            .and_then(|name| inspect_installed_identity(&path, name));
        match inspected {
            Ok((revision, documents)) => orphaned.push(OrphanedSourceInspection {
                source: name,
                path: path.to_string_lossy().into_owned(),
                removable: true,
                revision: Some(revision),
                documents: Some(documents),
                error: None,
            }),
            Err(error) => orphaned.push(OrphanedSourceInspection {
                source: name,
                path: path.to_string_lossy().into_owned(),
                removable: false,
                revision: None,
                documents: None,
                error: Some(error),
            }),
        }
    }
    Ok(orphaned)
}

pub(crate) fn inspect_installed_identity(path: &Path, name: &str) -> Result<(String, u32), String> {
    if name.starts_with(".prune-") {
        return Err(
            "incomplete prune transaction; inspect and remove it manually after verifying its contents"
                .to_owned(),
        );
    }
    if !is_source_name(name) {
        return Err("source directory name is invalid".to_owned());
    }
    let directory = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect source directory: {error}"))?;
    if directory.file_type().is_symlink() || !directory.file_type().is_dir() {
        return Err("source entry is not a regular directory".to_owned());
    }
    let metadata_path = path.join(SOURCE_METADATA_FILE);
    let metadata = fs::symlink_metadata(&metadata_path)
        .map_err(|error| format!("could not inspect source metadata: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("source metadata is not a regular file".to_owned());
    }
    let file = fs::File::open(metadata_path)
        .map_err(|error| format!("could not open source metadata: {error}"))?;
    let text = crate::bounded::read_utf8(file, MAX_METADATA_BYTES, "source metadata")
        .map_err(|error| error.to_string())?;
    let identity = toml::from_str::<InstalledSourceIdentity>(&text)
        .map_err(|error| format!("source metadata identity is invalid: {error}"))?;
    if identity.source != name {
        return Err(format!(
            "source metadata names '{}' instead of '{name}'",
            identity.source
        ));
    }
    Ok((identity.revision, identity.documents))
}

#[derive(Deserialize)]
struct InstalledSourceIdentity {
    source: String,
    revision: String,
    documents: u32,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::{DocumentPaths, config::load_source_config_from, metadata::source_fingerprint};

    use super::{SourceInstallationStatus, inspect_document_sources_from};

    fn temp(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-source-inspection-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn inspection_is_read_only_and_classifies_configured_sources() {
        let root = temp("configured");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let paths = DocumentPaths {
            config: root.join("sources.toml"),
            documents: root.join("documents"),
            sources: root.join("sources"),
            root: root.clone(),
        };
        fs::write(
            &paths.config,
            "[archive]\nurl = 'https://example.invalid/docs.zip'\n\n[git]\nrepo = 'https://example.invalid/docs.git'\nbranch = 'main'\n",
        )
        .expect("config");
        let config = load_source_config_from(&paths.config).expect("source config");

        let inspection = inspect_document_sources_from(&paths, &config).expect("inspection");
        assert!(inspection.config_exists);
        assert_eq!(inspection.sources.len(), 2);
        assert!(
            inspection
                .sources
                .iter()
                .all(|source| source.status == SourceInstallationStatus::Missing)
        );
        assert!(
            !paths.sources.exists(),
            "inspection must not create storage"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn inspection_reports_owned_and_suspicious_orphans() {
        let root = temp("orphans");
        let _ = fs::remove_dir_all(&root);
        let paths = DocumentPaths {
            config: root.join("sources.toml"),
            documents: root.join("documents"),
            sources: root.join("sources"),
            root: root.clone(),
        };
        let owned = paths.sources.join("owned");
        let suspicious = paths.sources.join("Bad");
        fs::create_dir_all(&owned).expect("owned");
        fs::create_dir_all(&suspicious).expect("suspicious");
        fs::write(
            owned.join(".mant-source.toml"),
            "version = 3\nsource = 'owned'\nrevision = 'abc'\nconfig_fingerprint = 'x'\ndocuments = 2\n\n[location]\nkind = 'archive'\nurl = 'https://example.invalid/docs.zip'\n",
        )
        .expect("metadata");
        let config = load_source_config_from(&paths.config).expect("empty config");

        let inspection = inspect_document_sources_from(&paths, &config).expect("inspection");
        assert_eq!(inspection.orphaned.len(), 2);
        assert!(inspection.orphaned[0].error.is_some());
        assert!(inspection.orphaned[1].removable);
        assert_eq!(inspection.orphaned[1].revision.as_deref(), Some("abc"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn configured_source_metadata_must_match_materialized_documents() {
        let root = temp("document-count");
        let _ = fs::remove_dir_all(&root);
        let paths = DocumentPaths {
            config: root.join("sources.toml"),
            documents: root.join("documents"),
            sources: root.join("sources"),
            root: root.clone(),
        };
        fs::create_dir_all(&paths.sources).expect("sources");
        fs::write(
            &paths.config,
            "[team]\nrepo = 'https://example.invalid/docs.git'\nbranch = 'main'\n",
        )
        .expect("config");
        let config = load_source_config_from(&paths.config).expect("source config");
        let configured = config.get("team").expect("team source");
        let directory = paths.sources.join("team");
        fs::create_dir_all(&directory).expect("installed source");
        fs::write(directory.join("only.md"), "# only").expect("document");
        fs::write(
            directory.join(crate::SOURCE_METADATA_FILE),
            format!(
                "version = 3\nsource = 'team'\nrevision = 'abc123'\nconfig_fingerprint = {:?}\ndocuments = 2\n\n[location]\nkind = 'git'\nrepo = 'https://example.invalid/docs.git'\nbranch = 'main'\n",
                source_fingerprint(configured)
            ),
        )
        .expect("metadata");

        let inspection = inspect_document_sources_from(&paths, &config).expect("inspection");
        assert_eq!(
            inspection.sources[0].status,
            SourceInstallationStatus::Invalid
        );
        assert!(
            inspection.sources[0]
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("records 2 documents but 1 are present"))
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
