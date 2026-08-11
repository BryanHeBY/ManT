//! Reports and explicitly removes updater-owned sources no longer configured.

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use super::{MAX_METADATA_BYTES, UpdateLock, sync_parent_directory};
use crate::config::DocumentPaths;
use crate::{
    SOURCE_METADATA_FILE, SourceConfig, SourceConfigError, is_source_name, load_source_config,
};

/// One updater-owned source directory absent from the active configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedSource {
    pub source: String,
    pub path: String,
    pub removable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of one explicit orphan cleanup candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourcePruneAction {
    WouldRemove,
    Removed,
    Refused,
    Failed,
}

/// Stable per-source result printed by `--prune-docs`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePruneResult {
    pub source: String,
    pub path: String,
    pub action: SourcePruneAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Exact schema marker for an orphan cleanup report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum DocumentSourcesPruneSchema {
    #[serde(rename = "mant.sources-prune/v1")]
    V1,
}

/// Complete result of one explicit source prune or dry run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourcesPrune {
    pub schema: DocumentSourcesPruneSchema,
    pub config: String,
    pub dry_run: bool,
    pub sources: Vec<SourcePruneResult>,
}

impl DocumentSourcesPrune {
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.sources.iter().any(|source| {
            matches!(
                source.action,
                SourcePruneAction::Refused | SourcePruneAction::Failed
            )
        })
    }
}

/// Report or explicitly remove updater-owned source directories absent from
/// the active configuration. This operation is intentionally unavailable to
/// the read-only MCP boundary.
///
/// # Errors
///
/// Returns an error when configuration, the source store, or the shared
/// update lock cannot be prepared. Unsafe candidates are retained as refused
/// per-source results.
pub fn prune_document_sources(dry_run: bool) -> Result<DocumentSourcesPrune, SourceConfigError> {
    let (paths, config) = load_source_config()?;
    fs::create_dir_all(&paths.sources).map_err(|error| {
        SourceConfigError::new(format!(
            "could not create document source directory '{}': {error}",
            paths.sources.display()
        ))
    })?;
    prune_document_sources_from(&paths, &config, dry_run)
}

pub(super) fn prune_document_sources_from(
    paths: &DocumentPaths,
    config: &SourceConfig,
    dry_run: bool,
) -> Result<DocumentSourcesPrune, SourceConfigError> {
    let _lock = UpdateLock::acquire(&paths.sources)?;
    let orphaned = discover_orphaned_sources(paths, config)?;
    let sources = orphaned
        .into_iter()
        .map(|orphan| prune_orphan(paths, orphan, dry_run))
        .collect();
    Ok(DocumentSourcesPrune {
        schema: DocumentSourcesPruneSchema::V1,
        config: paths.config.to_string_lossy().into_owned(),
        dry_run,
        sources,
    })
}

#[derive(Deserialize)]
struct InstalledSourceIdentity {
    source: String,
    revision: String,
    documents: u32,
}

pub(super) fn discover_orphaned_sources(
    paths: &DocumentPaths,
    config: &SourceConfig,
) -> Result<Vec<OrphanedSource>, SourceConfigError> {
    let mut entries = fs::read_dir(&paths.sources)
        .map_err(|error| {
            SourceConfigError::new(format!(
                "could not read document source directory '{}': {error}",
                paths.sources.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SourceConfigError::new(format!("could not read source entry: {error}")))?;
    entries.sort_unstable_by_key(fs::DirEntry::file_name);

    let mut orphaned = Vec::new();
    for entry in entries {
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy().into_owned();
        if name == ".update.lock" {
            continue;
        }
        if name_os
            .to_str()
            .is_some_and(|name| config.get(name).is_some())
        {
            continue;
        }
        let path = entry.path();
        let inspected = name_os
            .to_str()
            .ok_or_else(|| "source directory name is not UTF-8".to_owned())
            .and_then(|name| inspect_orphan(&path, name));
        match inspected {
            Ok(identity) => orphaned.push(OrphanedSource {
                source: name,
                path: path.to_string_lossy().into_owned(),
                removable: true,
                revision: Some(identity.revision),
                documents: Some(identity.documents),
                error: None,
            }),
            Err(error) => orphaned.push(OrphanedSource {
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

fn inspect_orphan(path: &Path, name: &str) -> Result<InstalledSourceIdentity, String> {
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
    let file = fs::File::open(&metadata_path)
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
    Ok(identity)
}

fn prune_orphan(paths: &DocumentPaths, orphan: OrphanedSource, dry_run: bool) -> SourcePruneResult {
    let mut result = SourcePruneResult {
        source: orphan.source.clone(),
        path: orphan.path,
        action: SourcePruneAction::Refused,
        revision: orphan.revision,
        documents: orphan.documents,
        error: orphan.error,
    };
    if !orphan.removable {
        return result;
    }
    if dry_run {
        result.action = SourcePruneAction::WouldRemove;
        return result;
    }

    let target = paths.sources.join(&orphan.source);
    if let Err(error) = inspect_orphan(&target, &orphan.source) {
        result.action = SourcePruneAction::Failed;
        result.error = Some(format!("source changed before removal: {error}"));
        return result;
    }
    let tombstone = paths
        .sources
        .join(format!(".prune-{}-{}", orphan.source, std::process::id()));
    if tombstone.exists() {
        result.action = SourcePruneAction::Failed;
        result.error = Some(format!(
            "temporary prune path '{}' already exists",
            tombstone.display()
        ));
        return result;
    }
    if let Err(error) = fs::rename(&target, &tombstone) {
        result.action = SourcePruneAction::Failed;
        result.error = Some(format!("could not isolate orphaned source: {error}"));
        return result;
    }
    if let Err(error) = sync_parent_directory(&target) {
        result.action = SourcePruneAction::Failed;
        result.error = Some(failed_prune_with_recovery(
            "could not persist isolated source",
            &error,
            &target,
            &tombstone,
        ));
        return result;
    }
    if let Err(error) = inspect_orphan(&tombstone, &orphan.source) {
        result.action = SourcePruneAction::Failed;
        result.error = Some(failed_prune_with_recovery(
            "source changed while being isolated",
            &error,
            &target,
            &tombstone,
        ));
        return result;
    }
    if let Err(error) = fs::remove_dir_all(&tombstone) {
        result.action = SourcePruneAction::Failed;
        result.error = Some(format!(
            "could not remove isolated source '{}': {error}",
            tombstone.display()
        ));
        return result;
    }
    if let Err(error) = sync_parent_directory(&target) {
        result.action = SourcePruneAction::Failed;
        result.error = Some(format!(
            "source was removed but its parent directory could not be synchronized: {error}"
        ));
        return result;
    }
    result.action = SourcePruneAction::Removed;
    result
}

fn failed_prune_with_recovery(
    context: &str,
    error: &str,
    target: &Path,
    tombstone: &Path,
) -> String {
    match fs::rename(tombstone, target)
        .and_then(|()| sync_parent_directory(target).map_err(std::io::Error::other))
    {
        Ok(()) => format!("{context}: {error}; restored the original source directory"),
        Err(recovery) => format!(
            "{context}: {error}; could not restore '{}' from '{}': {recovery}",
            target.display(),
            tombstone.display()
        ),
    }
}
