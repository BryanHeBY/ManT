//! Updates configured sources and atomically installs selected Markdown.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;

mod archive;
mod git;
mod prune;
mod workspace;

use crate::limits::{
    MAX_DOCUMENT_BYTES, MAX_SOURCE_BYTES, MAX_SOURCE_DEPTH, MAX_SOURCE_DOCUMENTS,
    MAX_SOURCE_ENTRIES,
};
use crate::{
    metadata::{SourceMetadata, read_source_metadata, source_fingerprint},
    registry::managed_document_count,
};
use prune::discover_orphaned_sources;
#[cfg(test)]
use prune::prune_document_sources_from;
pub use prune::{
    DocumentSourcesPrune, DocumentSourcesPruneSchema, OrphanedSource, SourcePruneAction,
    SourcePruneResult, prune_document_sources,
};
use workspace::UpdateWorkspace;

use super::config::{
    ConfiguredSource, DocumentPaths, SOURCE_METADATA_FILE, SourceConfigError, SourceLocation,
    load_source_config,
};

/// Outcome for one configured repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceUpdateAction {
    /// New content was installed atomically.
    Updated,
    /// The installed revision and configuration fingerprint were current.
    Unchanged,
    /// This source failed without aborting updates for other sources.
    Failed,
}

/// Stable per-source update result printed by the native CLI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceUpdateResult {
    /// Configured source name.
    pub source: String,
    /// Outcome of this source update.
    pub action: SourceUpdateAction,
    /// Installed or observed source revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Number of installed Markdown documents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<u32>,
    /// Human-readable failure detail for [`SourceUpdateAction::Failed`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Exact schema marker for a document-source update report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum DocumentSourcesUpdateSchema {
    /// Version 2 of the native source-update report.
    #[serde(rename = "mant.sources-update/v2")]
    V2,
}

/// Complete result of one `--update-docs` run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourcesUpdate {
    /// Exact report schema discriminator.
    pub schema: DocumentSourcesUpdateSchema,
    /// Platform-native path of the configuration used by this run.
    pub config: String,
    /// Per-source results in configured precedence order.
    pub sources: Vec<SourceUpdateResult>,
    /// Updater-owned directories no longer present in configuration.
    pub orphaned: Vec<OrphanedSource>,
}

impl DocumentSourcesUpdate {
    /// Return whether at least one configured source failed.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.sources
            .iter()
            .any(|source| source.action == SourceUpdateAction::Failed)
    }
}

/// Update every configured source without exposing this operation to MCP.
///
/// # Errors
///
/// Returns an error when configuration cannot be loaded or the update lock and
/// source store cannot be prepared. Individual repository failures are kept in
/// the returned report.
pub fn update_document_sources() -> Result<DocumentSourcesUpdate, SourceConfigError> {
    let (paths, config) = load_source_config()?;
    fs::create_dir_all(&paths.sources).map_err(|error| {
        SourceConfigError::new(format!(
            "could not create document source directory '{}': {error}",
            paths.sources.display()
        ))
    })?;
    let _lock = UpdateLock::acquire(&paths.sources)?;

    let sources = config
        .sources()
        .iter()
        .map(|(name, source)| update_one_source(&paths, name, source))
        .collect();
    let orphaned = discover_orphaned_sources(&paths, &config)?;
    Ok(DocumentSourcesUpdate {
        schema: DocumentSourcesUpdateSchema::V2,
        config: paths.config.to_string_lossy().into_owned(),
        sources,
        orphaned,
    })
}

fn update_one_source(
    paths: &DocumentPaths,
    name: &str,
    source: &ConfiguredSource,
) -> SourceUpdateResult {
    match try_update_one_source(paths, name, source) {
        Ok(result) => result,
        Err(error) => SourceUpdateResult {
            source: name.to_owned(),
            action: SourceUpdateAction::Failed,
            revision: None,
            documents: None,
            error: Some(error),
        },
    }
}

pub(in crate::update) struct SourceUpdateContext<'a> {
    pub(in crate::update) paths: &'a DocumentPaths,
    pub(in crate::update) name: &'a str,
    pub(in crate::update) configured: &'a ConfiguredSource,
    pub(in crate::update) target: PathBuf,
    pub(in crate::update) fingerprint: String,
    pub(in crate::update) metadata: Option<SourceMetadata>,
}

impl<'a> SourceUpdateContext<'a> {
    fn prepare(
        paths: &'a DocumentPaths,
        name: &'a str,
        configured: &'a ConfiguredSource,
    ) -> Result<Self, String> {
        let target = paths.sources.join(name);
        recover_directory(&target)?;
        let fingerprint = source_fingerprint(configured);
        let metadata = read_source_metadata(&target)
            .ok()
            .filter(|metadata| metadata.matches(name, configured, &fingerprint))
            .filter(|metadata| {
                managed_document_count(&target)
                    .is_ok_and(|documents| documents == metadata.documents())
            });
        Ok(Self {
            paths,
            name,
            configured,
            target,
            fingerprint,
            metadata,
        })
    }

    pub(in crate::update) fn unchanged(&self, revision: String) -> SourceUpdateResult {
        unchanged_result(
            self.name,
            revision,
            self.metadata.as_ref().map_or(0, SourceMetadata::documents),
        )
    }

    pub(in crate::update) fn updated(
        &self,
        revision: String,
        documents: u32,
    ) -> SourceUpdateResult {
        SourceUpdateResult {
            source: self.name.to_owned(),
            action: SourceUpdateAction::Updated,
            revision: Some(revision),
            documents: Some(documents),
            error: None,
        }
    }
}

fn try_update_one_source(
    paths: &DocumentPaths,
    name: &str,
    source: &ConfiguredSource,
) -> Result<SourceUpdateResult, String> {
    let context = SourceUpdateContext::prepare(paths, name, source)?;

    match &source.location {
        SourceLocation::Git { repo, branch } => git::update(&context, repo, branch),
        SourceLocation::Archive { url } => archive::update(&context, url),
    }
}

fn unchanged_result(source: &str, revision: String, documents: u32) -> SourceUpdateResult {
    SourceUpdateResult {
        source: source.to_owned(),
        action: SourceUpdateAction::Unchanged,
        revision: Some(revision),
        documents: Some(documents),
        error: None,
    }
}

pub(in crate::update) fn activate_source(
    staging: &Path,
    target: &Path,
    metadata: &SourceMetadata,
) -> Result<(), String> {
    let metadata_text = toml::to_string_pretty(metadata)
        .map_err(|error| format!("could not encode source metadata: {error}"))?;
    let metadata_path = staging.join(SOURCE_METADATA_FILE);
    fs::write(&metadata_path, metadata_text)
        .map_err(|error| format!("could not write source metadata: {error}"))?;
    sync_file(&metadata_path, "source metadata")?;
    #[cfg(unix)]
    sync_directory(staging)?;
    replace_directory(staging, target)
}

pub(in crate::update) fn install_selected_documents(
    checkout: &Path,
    staging: &Path,
    source: &ConfiguredSource,
) -> Result<usize, String> {
    let requested_root = if source.path == "." {
        checkout.to_owned()
    } else {
        checkout.join(&source.path)
    };
    let checkout = fs::canonicalize(checkout)
        .map_err(|error| format!("could not resolve source checkout: {error}"))?;
    let root = fs::canonicalize(&requested_root).map_err(|error| {
        format!(
            "could not resolve configured path '{}': {error}",
            source.path
        )
    })?;
    if !root.starts_with(&checkout) || !root.is_dir() {
        return Err(format!(
            "configured path '{}' must resolve to a directory inside the source checkout",
            source.path
        ));
    }
    let mut candidates = Vec::new();
    let mut budget = SourceTreeBudget::default();
    collect_markdown(&root, &root, 0, &mut budget, &mut candidates)?;
    candidates.retain(|(relative, _)| source_selects_path(source, relative));
    candidates.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    if candidates.is_empty() {
        return Err(format!(
            "configured path '{}' selected no Markdown documents; adjust path, include, or exclude",
            source.path
        ));
    }

    let mut selected = BTreeMap::<String, (String, u8, &PathBuf, &PathBuf)>::new();
    for (relative, path) in &candidates {
        let logical = markdown_logical_path(relative)?;
        let key = logical.to_ascii_lowercase();
        let priority = markdown_extension_priority(path)
            .ok_or_else(|| format!("invalid Markdown path: {}", relative.display()))?;
        match selected.get(&key) {
            Some((existing, _, _, _)) if existing != &logical => {
                return Err(format!(
                    "selected Markdown paths '{existing}' and '{logical}' differ only by case"
                ));
            }
            Some((_, existing_priority, _, _)) if *existing_priority <= priority => {}
            _ => {
                selected.insert(key, (logical, priority, relative, path));
            }
        }
    }
    let mut installed_bytes = 0_u64;
    for (_, _, relative, path) in selected.values() {
        let size = fs::metadata(path)
            .map_err(|error| format!("could not inspect '{}': {error}", relative.display()))?
            .len();
        if size > MAX_DOCUMENT_BYTES {
            return Err(format!(
                "Markdown document '{}' exceeds the {MAX_DOCUMENT_BYTES}-byte limit",
                relative.display()
            ));
        }
        installed_bytes = installed_bytes
            .checked_add(size)
            .ok_or_else(|| "selected document size budget overflow".to_owned())?;
        if installed_bytes > MAX_SOURCE_BYTES {
            return Err(format!(
                "selected documents exceed the {MAX_SOURCE_BYTES}-byte limit at '{}'",
                relative.display()
            ));
        }
        let installed = staging.join(relative);
        let parent = installed
            .parent()
            .ok_or_else(|| format!("invalid Markdown path: {}", relative.display()))?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create installed directory '{}': {error}",
                parent.display()
            )
        })?;
        let copied = fs::copy(path, &installed)
            .map_err(|error| format!("could not install '{}': {error}", relative.display()))?;
        if copied != size {
            return Err(format!(
                "source document '{}' changed while it was being installed",
                relative.display()
            ));
        }
        sync_file(&installed, "installed document")?;
        #[cfg(unix)]
        sync_directory(parent)?;
    }
    Ok(selected.len())
}

fn markdown_logical_path(relative: &Path) -> Result<String, String> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let stem = relative
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("Markdown filename is not UTF-8: {}", relative.display()))?;
    let path = parent.join(stem);
    let components = path
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("Markdown path is not UTF-8: {}", relative.display()))?;
    Ok(components.join("/"))
}

fn collect_markdown(
    root: &Path,
    directory: &Path,
    depth: usize,
    budget: &mut SourceTreeBudget,
    output: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    if depth > MAX_SOURCE_DEPTH {
        return Err(format!(
            "source tree exceeds the maximum depth of {MAX_SOURCE_DEPTH}"
        ));
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("could not read '{}': {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not read source entry: {error}"))?;
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect '{}': {error}", entry.path().display()))?;
        if entry.file_name() == OsStr::new(".git") && file_type.is_dir() {
            continue;
        }
        budget.entries += 1;
        if budget.entries > MAX_SOURCE_ENTRIES {
            return Err(format!(
                "source tree contains more than {MAX_SOURCE_ENTRIES} entries"
            ));
        }
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_markdown(root, &entry.path(), depth + 1, budget, output)?;
        } else if file_type.is_file() {
            let size = entry
                .metadata()
                .map_err(|error| {
                    format!("could not inspect '{}': {error}", entry.path().display())
                })?
                .len();
            budget.bytes = budget
                .bytes
                .checked_add(size)
                .ok_or_else(|| "source tree size budget overflow".to_owned())?;
            if budget.bytes > MAX_SOURCE_BYTES {
                return Err(format!(
                    "source tree exceeds the {MAX_SOURCE_BYTES}-byte limit at '{}'",
                    entry.path().display()
                ));
            }
            if is_markdown_file(&entry.path()) {
                if output.len() >= MAX_SOURCE_DOCUMENTS {
                    return Err(format!(
                        "source contains more than {MAX_SOURCE_DOCUMENTS} Markdown files"
                    ));
                }
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .expect("walk remains below source root")
                    .to_owned();
                output.push((relative, entry.path()));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct SourceTreeBudget {
    entries: usize,
    bytes: u64,
}

fn is_markdown_file(path: &Path) -> bool {
    markdown_extension_priority(path).is_some()
}

pub(in crate::update) fn source_selects_markdown_path(
    source: &ConfiguredSource,
    relative: &Path,
) -> bool {
    is_markdown_file(relative) && source_selects_path(source, relative)
}

fn source_selects_path(source: &ConfiguredSource, relative: &Path) -> bool {
    (source.include.is_empty()
        || source
            .include
            .iter()
            .any(|selector| selector_matches(relative, selector)))
        && !source
            .exclude
            .iter()
            .any(|selector| selector_matches(relative, selector))
}

fn markdown_extension_priority(path: &Path) -> Option<u8> {
    let extension = path.extension()?.to_str()?;
    ["md", "markdown"]
        .iter()
        .position(|candidate| extension.eq_ignore_ascii_case(candidate))
        .and_then(|index| u8::try_from(index).ok())
}

fn selector_matches(relative: &Path, selector: &str) -> bool {
    let selector = Path::new(selector);
    relative == selector || relative.starts_with(selector)
}

fn replace_directory(staging: &Path, target: &Path) -> Result<(), String> {
    recover_directory(target)?;
    let backup = target.with_extension("backup");
    let had_target = target.exists();
    if had_target {
        fs::rename(target, &backup)
            .map_err(|error| format!("could not preserve previous source: {error}"))?;
        sync_parent_directory(target)?;
    }
    if let Err(error) = fs::rename(staging, target) {
        if had_target {
            let _ = fs::rename(&backup, target);
            let _ = sync_parent_directory(target);
        }
        return Err(format!("could not activate updated source: {error}"));
    }
    sync_parent_directory(target)?;
    remove_internal_dir(&backup);
    sync_parent_directory(target)?;
    Ok(())
}

fn sync_file(path: &Path, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    let file = fs::OpenOptions::new().write(true).open(path);
    #[cfg(not(windows))]
    let file = fs::File::open(path);
    file.and_then(|file| file.sync_all())
        .map_err(|error| format!("could not sync {label}: {error}"))
}

fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "source target has no parent directory".to_owned())?;
    #[cfg(unix)]
    {
        sync_directory(parent)
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not sync directory '{}': {error}", path.display()))
}

fn recover_directory(target: &Path) -> Result<(), String> {
    let backup = target.with_extension("backup");
    if !backup.exists() {
        return Ok(());
    }
    if target.exists() {
        remove_internal_dir(&backup);
        Ok(())
    } else {
        fs::rename(&backup, target)
            .map_err(|error| format!("could not recover previous source: {error}"))
    }
}

fn remove_internal_dir(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}

struct UpdateLock {
    path: PathBuf,
}

impl UpdateLock {
    fn acquire(sources: &Path) -> Result<Self, SourceConfigError> {
        let path = sources.join(".update.lock");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                let detail = if error.kind() == io::ErrorKind::AlreadyExists {
                    format!(
                        "another document source update is already running; if no update process remains, remove '{}'",
                        path.display()
                    )
                } else {
                    format!("could not acquire document source update lock: {error}")
                };
                SourceConfigError::new(detail)
            })?;
        Ok(Self { path })
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Read as _, Write as _},
        net::{Shutdown, TcpListener},
        process::Command,
        thread,
    };

    use zip::{ZipWriter, write::SimpleFileOptions};

    #[cfg(any(unix, windows))]
    use super::sync_file;
    use super::{
        ConfiguredSource, DocumentPaths, SourceLocation, SourceMetadata, SourcePruneAction,
        SourceUpdateAction, SourceUpdateContext, UpdateLock, discover_orphaned_sources,
        install_selected_documents, prune_document_sources_from, recover_directory,
        source_fingerprint, try_update_one_source,
    };
    use crate::config::load_source_config_from;

    fn temp(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mant-sources-{label}-{}", std::process::id()))
    }

    fn source(path: &str) -> ConfiguredSource {
        ConfiguredSource {
            location: SourceLocation::Git {
                repo: "repo".to_owned(),
                branch: "main".to_owned(),
            },
            path: path.to_owned(),
            include: Vec::new(),
            exclude: Vec::new(),
            priority: 0,
        }
    }

    fn archive_source(url: String) -> ConfiguredSource {
        ConfiguredSource {
            location: SourceLocation::Archive { url },
            path: ".".to_owned(),
            include: Vec::new(),
            exclude: Vec::new(),
            priority: 0,
        }
    }

    fn paths(root: &std::path::Path) -> DocumentPaths {
        DocumentPaths {
            config: root.join("sources.toml"),
            documents: root.join("documents"),
            sources: root.join("sources"),
            root: root.to_owned(),
        }
    }

    fn write_installed_identity(directory: &std::path::Path, source: &str, documents: u32) {
        fs::create_dir_all(directory).expect("create installed source");
        fs::write(
            directory.join(".mant-source.toml"),
            format!(
                "version = 1\nsource = {source:?}\nrevision = \"abc123\"\ndocuments = {documents}\n"
            ),
        )
        .expect("write installed identity");
        fs::write(directory.join("tool.md"), "# tool").expect("write installed document");
    }

    #[test]
    fn update_does_not_trust_metadata_after_installed_documents_disappear() {
        let root = temp("missing-installed-document");
        let paths = paths(&root);
        let configured = source(".");
        let target = paths.sources.join("team");
        fs::create_dir_all(&target).expect("installed source");
        fs::write(target.join("remaining.md"), "# remaining").expect("remaining document");
        let metadata = SourceMetadata::git(
            "team",
            "repo",
            "main",
            "abc123".to_owned(),
            &source_fingerprint(&configured),
            2,
        );
        fs::write(
            target.join(crate::SOURCE_METADATA_FILE),
            toml::to_string_pretty(&metadata).expect("metadata text"),
        )
        .expect("metadata");

        let context =
            SourceUpdateContext::prepare(&paths, "team", &configured).expect("update context");
        assert!(
            context.metadata.is_none(),
            "an incomplete installation must force reacquisition"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn zip_document() -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("tool.md", SimpleFileOptions::default())
            .expect("start ZIP file");
        writer.write_all(b"# tool").expect("write ZIP file");
        writer.finish().expect("finish ZIP").into_inner()
    }

    #[test]
    fn prune_reports_then_removes_only_verified_orphaned_sources() {
        let root = temp("prune");
        let paths = paths(&root);
        fs::create_dir_all(&paths.sources).expect("create source store");
        fs::write(
            &paths.config,
            "[active]\nurl = 'https://example.invalid/active.zip'\n",
        )
        .expect("write source config");
        let config = load_source_config_from(&paths.config).expect("load source config");
        write_installed_identity(&paths.sources.join("active"), "active", 2);
        write_installed_identity(&paths.sources.join("removed"), "removed", 7);
        fs::create_dir_all(&paths.documents).expect("create personal documents");
        fs::write(paths.documents.join("personal.md"), "# personal")
            .expect("write personal document");

        let update_orphans = discover_orphaned_sources(&paths, &config).expect("discover orphans");
        assert!(matches!(
            update_orphans.as_slice(),
            [orphan]
                if orphan.source == "removed"
                    && orphan.removable
                    && orphan.revision.as_deref() == Some("abc123")
                    && orphan.documents == Some(7)
        ));

        let dry_run =
            prune_document_sources_from(&paths, &config, true).expect("dry-run source prune");
        assert!(!dry_run.has_failures());
        assert!(dry_run.dry_run);
        assert_eq!(dry_run.sources[0].action, SourcePruneAction::WouldRemove);
        assert!(paths.sources.join("removed").is_dir());

        let applied =
            prune_document_sources_from(&paths, &config, false).expect("apply source prune");
        assert!(!applied.has_failures());
        assert_eq!(applied.sources[0].action, SourcePruneAction::Removed);
        assert!(!paths.sources.join("removed").exists());
        assert!(paths.sources.join("active/tool.md").is_file());
        assert!(paths.documents.join("personal.md").is_file());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn prune_refuses_unverified_entries_and_obeys_the_update_lock() {
        let root = temp("prune-hostile");
        let paths = paths(&root);
        fs::create_dir_all(&paths.sources).expect("create source store");
        fs::write(&paths.config, "").expect("write empty config");
        let config = load_source_config_from(&paths.config).expect("load source config");
        write_installed_identity(&paths.sources.join("mismatch"), "other", 1);
        write_installed_identity(&paths.sources.join("Invalid"), "Invalid", 1);
        write_installed_identity(&paths.sources.join(".prune-old-123"), "old", 1);
        #[cfg(unix)]
        let denied_metadata = {
            use std::os::unix::fs::PermissionsExt as _;

            let directory = paths.sources.join("denied");
            write_installed_identity(&directory, "denied", 1);
            let metadata = directory.join(".mant-source.toml");
            fs::set_permissions(&metadata, fs::Permissions::from_mode(0o0))
                .expect("deny source metadata access");
            metadata
        };
        fs::write(paths.sources.join("ordinary-file"), "not a source")
            .expect("write unexpected file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, paths.sources.join("linked"))
            .expect("create source symlink");

        let dry_run = prune_document_sources_from(&paths, &config, true).expect("hostile dry run");
        assert!(dry_run.has_failures());
        for name in ["mismatch", "Invalid", ".prune-old-123", "ordinary-file"] {
            assert!(dry_run.sources.iter().any(|source| {
                source.source == name && source.action == SourcePruneAction::Refused
            }));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            if fs::File::open(&denied_metadata).is_err() {
                assert!(dry_run.sources.iter().any(|source| {
                    source.source == "denied" && source.action == SourcePruneAction::Refused
                }));
            }
            fs::set_permissions(&denied_metadata, fs::Permissions::from_mode(0o600))
                .expect("restore source metadata access");
        }
        assert!(paths.sources.join("mismatch").is_dir());
        assert!(paths.sources.join("Invalid").is_dir());
        assert!(paths.sources.join(".prune-old-123").is_dir());
        assert!(paths.sources.join("ordinary-file").is_file());

        let lock = UpdateLock::acquire(&paths.sources).expect("hold update lock");
        let error = prune_document_sources_from(&paths, &config, true)
            .expect_err("concurrent prune must fail");
        assert!(error.to_string().contains("another document source update"));
        drop(lock);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn install_preserves_hierarchy_and_allows_repeated_leaf_names() {
        let root = temp("collision");
        let checkout = root.join("checkout");
        let staging = root.join("staging");
        fs::create_dir_all(checkout.join("one")).expect("create first directory");
        fs::create_dir_all(checkout.join("two")).expect("create second directory");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(checkout.join("one/tool.md"), "# one").expect("write first");
        fs::write(checkout.join("two/tool.markdown"), "# two").expect("write second");
        assert_eq!(
            install_selected_documents(&checkout, &staging, &source("."))
                .expect("install hierarchical documents"),
            2
        );
        assert!(staging.join("one/tool.md").is_file());
        assert!(staging.join("two/tool.markdown").is_file());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn install_rejects_oversized_documents_before_copying() {
        let root = temp("oversized-document");
        let checkout = root.join("checkout");
        let staging = root.join("staging");
        fs::create_dir_all(&checkout).expect("create checkout");
        fs::create_dir_all(&staging).expect("create staging");
        fs::File::create(checkout.join("tool.md"))
            .and_then(|file| file.set_len(crate::limits::MAX_DOCUMENT_BYTES + 1))
            .expect("create sparse oversized document");

        let error = install_selected_documents(&checkout, &staging, &source("."))
            .expect_err("oversized document must fail");
        assert!(
            error.contains("Markdown document 'tool.md' exceeds"),
            "{error}"
        );
        assert!(!staging.join("tool.md").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn install_rejects_oversized_materialized_source_trees() {
        let root = temp("oversized-source-tree");
        let checkout = root.join("checkout");
        let staging = root.join("staging");
        fs::create_dir_all(&checkout).expect("create checkout");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(checkout.join("tool.md"), "# tool").expect("write document");
        fs::File::create(checkout.join("payload.bin"))
            .and_then(|file| file.set_len(crate::limits::MAX_SOURCE_BYTES + 1))
            .expect("create sparse oversized payload");

        let error = install_selected_documents(&checkout, &staging, &source("."))
            .expect_err("oversized source tree must fail");
        assert!(error.contains("source tree exceeds"), "{error}");
        assert!(!staging.join("tool.md").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn empty_selection_fails_before_replacing_an_installed_source() {
        let root = temp("empty-selection");
        let checkout = root.join("checkout");
        let staging = root.join("staging");
        fs::create_dir_all(&checkout).expect("create checkout");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(checkout.join("README.txt"), "not Markdown").expect("write ignored file");

        let error = install_selected_documents(&checkout, &staging, &source("."))
            .expect_err("an empty document package must not activate");
        assert!(error.contains("selected no Markdown documents"), "{error}");
        assert!(staging.read_dir().expect("read staging").next().is_none());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[cfg(windows)]
    #[test]
    fn syncing_an_installed_file_uses_a_write_capable_handle() {
        let root = temp("sync-file");
        fs::create_dir_all(&root).expect("create sync fixture");
        let path = root.join("tool.md");
        fs::write(&path, "# tool").expect("write sync fixture");

        sync_file(&path, "test document").expect("sync file on Windows");

        fs::remove_dir_all(root).expect("remove sync fixture");
    }

    #[cfg(unix)]
    #[test]
    fn syncing_an_installed_file_accepts_read_only_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = temp("sync-read-only-file");
        fs::create_dir_all(&root).expect("create sync fixture");
        let path = root.join("tool.md");
        fs::write(&path, "# tool").expect("write sync fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))
            .expect("make sync fixture read-only");

        sync_file(&path, "test document").expect("sync read-only file on Unix");

        fs::remove_dir_all(root).expect("remove sync fixture");
    }

    #[cfg(unix)]
    #[test]
    fn configured_path_symlink_cannot_escape_checkout() {
        use std::os::unix::fs::symlink;

        let root = temp("path-symlink");
        let checkout = root.join("checkout");
        let outside = root.join("outside");
        let staging = root.join("staging");
        fs::create_dir_all(&checkout).expect("create checkout");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(outside.join("private.md"), "# private").expect("write outside document");
        symlink(&outside, checkout.join("docs")).expect("link configured path outside checkout");

        let error = install_selected_documents(&checkout, &staging, &source("docs"))
            .expect_err("configured path must remain inside checkout");
        assert!(error.contains("inside the source checkout"), "{error}");
        assert!(!staging.join("private.md").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn fingerprint_normalizes_selector_order_and_duplicates() {
        let source = ConfiguredSource {
            path: "docs".to_owned(),
            include: vec!["b".to_owned(), "a".to_owned(), "a".to_owned()],
            exclude: vec!["z".to_owned(), "y".to_owned()],
            priority: 3,
            ..source(".")
        };
        let equivalent = ConfiguredSource {
            include: vec!["a".to_owned(), "b".to_owned()],
            exclude: vec!["y".to_owned(), "z".to_owned()],
            priority: 99,
            ..source.clone()
        };
        assert_eq!(source_fingerprint(&source), source_fingerprint(&equivalent));
    }

    #[test]
    fn interrupted_replacement_recovers_the_previous_directory() {
        let root = temp("recover");
        fs::create_dir_all(&root).expect("create root");
        let target = root.join("team");
        let backup = target.with_extension("backup");
        fs::create_dir_all(&backup).expect("create backup");
        fs::write(backup.join("tool.md"), "# old").expect("write backup document");

        recover_directory(&target).expect("recover backup");
        assert!(target.join("tool.md").is_file());
        assert!(!backup.exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn archive_source_installs_root_and_uses_conditional_updates() {
        let body = zip_document();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let url = format!(
            "http://{}/docs.zip",
            listener.local_addr().expect("server address")
        );
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for index in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).expect("read request");
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                }
                if index == 1 {
                    stream
                        .write_all(
                            b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .expect("write not-modified response");
                } else {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                        body.len(),
                        if index == 0 { "ETag: \"v1\"\r\n" } else { "" },
                    )
                    .expect("write response headers");
                    stream.write_all(&body).expect("write response body");
                }
                stream.flush().expect("flush response");
                stream.shutdown(Shutdown::Write).expect("finish response");
                requests.push(String::from_utf8(request).expect("UTF-8 request"));
            }
            requests
        });

        let root = temp("archive-update");
        let paths = DocumentPaths {
            config: root.join("sources.toml"),
            documents: root.join("documents"),
            sources: root.join("sources"),
            root: root.clone(),
        };
        fs::create_dir_all(&paths.sources).expect("create source store");
        let source = archive_source(url);
        let first = try_update_one_source(&paths, "release", &source).expect("first update");
        assert_eq!(first.action, SourceUpdateAction::Updated);
        assert_eq!(
            fs::read_to_string(paths.sources.join("release/tool.md")).expect("installed document"),
            "# tool"
        );
        let second = try_update_one_source(&paths, "release", &source).expect("second update");
        assert_eq!(second.action, SourceUpdateAction::Unchanged);
        let third = try_update_one_source(&paths, "release", &source).expect("third update");
        assert_eq!(third.action, SourceUpdateAction::Unchanged);
        let requests = server.join().expect("join server");
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("if-none-match: \"v1\"")
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn git_source_materializes_and_installs_only_its_configured_path() {
        let root = temp("git-path-update");
        let repository = root.join("repository");
        let data = root.join("data");
        fs::create_dir_all(repository.join("docs")).expect("create document directory");
        fs::create_dir_all(repository.join("website")).expect("create unrelated directory");
        fs::write(repository.join("docs/tool.md"), "# tool").expect("write document");
        fs::write(repository.join("website/index.md"), "# website")
            .expect("write unrelated document");
        for arguments in [
            vec!["init", "--quiet", "--initial-branch=main"],
            vec!["config", "user.name", "ManT tests"],
            vec!["config", "user.email", "mant-tests@example.invalid"],
            vec!["config", "uploadpack.allowFilter", "true"],
            vec!["add", "--", "."],
            vec!["commit", "--quiet", "-m", "fixture"],
        ] {
            let status = Command::new("git")
                .args(arguments)
                .current_dir(&repository)
                .status()
                .expect("run fixture git command");
            assert!(status.success(), "fixture git command failed");
        }
        let paths = paths(&data);
        fs::create_dir_all(&paths.sources).expect("create source store");
        let configured = ConfiguredSource {
            location: SourceLocation::Git {
                repo: repository.to_string_lossy().into_owned(),
                branch: "main".to_owned(),
            },
            path: "docs".to_owned(),
            include: Vec::new(),
            exclude: Vec::new(),
            priority: 1,
        };

        let result = try_update_one_source(&paths, "team", &configured)
            .expect("install configured Git path");
        assert_eq!(result.action, SourceUpdateAction::Updated);
        assert_eq!(
            fs::read_to_string(paths.sources.join("team/tool.md")).expect("read installed doc"),
            "# tool"
        );
        assert!(!paths.sources.join("team/index.md").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn invalid_archive_preserves_the_installed_source() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let url = format!(
            "http://{}/broken.tar",
            listener.local_addr().expect("server address")
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 14\r\nConnection: close\r\n\r\nnot an archive",
                )
                .expect("write response");
            stream.flush().expect("flush response");
            stream.shutdown(Shutdown::Write).expect("finish response");
        });
        let root = temp("archive-failure");
        let paths = DocumentPaths {
            config: root.join("sources.toml"),
            documents: root.join("documents"),
            sources: root.join("sources"),
            root: root.clone(),
        };
        fs::create_dir_all(paths.sources.join("release")).expect("create installed source");
        fs::write(paths.sources.join("release/tool.md"), "# old").expect("write old document");
        let error = try_update_one_source(&paths, "release", &archive_source(url))
            .expect_err("reject invalid archive");
        assert!(error.contains("tar entry"), "{error}");
        assert_eq!(
            fs::read_to_string(paths.sources.join("release/tool.md")).expect("old document"),
            "# old"
        );
        server.join().expect("join server");
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
