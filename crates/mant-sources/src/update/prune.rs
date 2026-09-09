//! Reports and explicitly removes updater-owned sources no longer configured.

use std::{fs, path::Path};

use super::{UpdateLock, sync_parent_directory};
use crate::config::DocumentPaths;
use crate::{
    DocumentSourcesPrune, DocumentSourcesPruneSchema, OrphanedSource, SourceConfig,
    SourceConfigError, SourcePruneAction, SourcePruneResult,
    inspection::{
        OrphanedSourceInspection, inspect_installed_identity, inspect_unconfigured_sources,
    },
    load_source_config,
};

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

pub(super) fn discover_orphaned_sources(
    paths: &DocumentPaths,
    config: &SourceConfig,
) -> Result<Vec<OrphanedSource>, SourceConfigError> {
    inspect_unconfigured_sources(paths, config).map(|sources| {
        sources
            .into_iter()
            .map(|source| {
                let OrphanedSourceInspection {
                    source,
                    path,
                    removable,
                    revision,
                    documents,
                    error,
                } = source;
                OrphanedSource {
                    source,
                    path,
                    removable,
                    revision,
                    documents,
                    error,
                }
            })
            .collect()
    })
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
    if let Err(error) = inspect_installed_identity(&target, &orphan.source) {
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
    if let Err(error) = inspect_installed_identity(&tombstone, &orphan.source) {
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
