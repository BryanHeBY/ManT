//! Updates configured sources and atomically installs selected Markdown.

use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use super::{
    archive::extract_archive,
    config::{
        ConfiguredSource, DocumentPaths, SOURCE_METADATA_FILE, SourceConfigError, SourceLocation,
        load_source_config,
    },
    download::{DownloadOutcome, Validators, download_archive},
};

const METADATA_VERSION: u8 = 2;
const MAX_METADATA_BYTES: u64 = 64 * 1024;
const MAX_SOURCE_DOCUMENTS: usize = 10_000;
const MAX_SOURCE_DEPTH: usize = 32;

/// Outcome for one configured repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceUpdateAction {
    Updated,
    Unchanged,
    Failed,
}

/// Stable per-source update result printed by the native CLI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceUpdateResult {
    pub source: String,
    pub action: SourceUpdateAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Exact schema marker for a document-source update report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum DocumentSourcesUpdateSchema {
    #[serde(rename = "mant.sources-update/v1")]
    V1,
}

/// Complete result of one `--update-docs` run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourcesUpdate {
    pub schema: DocumentSourcesUpdateSchema,
    pub config: String,
    pub sources: Vec<SourceUpdateResult>,
}

impl DocumentSourcesUpdate {
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.sources
            .iter()
            .any(|source| source.action == SourceUpdateAction::Failed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceMetadata {
    version: u8,
    source: String,
    kind: SourceMetadataKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    revision: String,
    config_fingerprint: String,
    documents: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SourceMetadataKind {
    Git,
    Archive,
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
    Ok(DocumentSourcesUpdate {
        schema: DocumentSourcesUpdateSchema::V1,
        config: paths.config.to_string_lossy().into_owned(),
        sources,
    })
}

fn source_fingerprint(source: &ConfiguredSource) -> String {
    let mut include = source
        .include
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut exclude = source
        .exclude
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    include.sort_unstable();
    exclude.sort_unstable();
    include.dedup();
    exclude.dedup();
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    let location = match &source.location {
        SourceLocation::Git { repo, branch } => format!("git\0{repo}\0{branch}"),
        SourceLocation::Archive { url } => format!("archive\0{url}"),
    };
    for byte in location
        .bytes()
        .chain([0])
        .chain(source.path.bytes())
        .chain([0])
        .chain(
            include
                .into_iter()
                .flat_map(|value| value.bytes().chain([0])),
        )
        .chain([0xff])
        .chain(
            exclude
                .into_iter()
                .flat_map(|value| value.bytes().chain([0])),
        )
    {
        state ^= u64::from(byte);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{state:016x}")
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

fn try_update_one_source(
    paths: &DocumentPaths,
    name: &str,
    source: &ConfiguredSource,
) -> Result<SourceUpdateResult, String> {
    let target = paths.sources.join(name);
    recover_directory(&target)?;
    let fingerprint = source_fingerprint(source);
    let metadata = read_metadata(&target)
        .filter(|metadata| metadata_matches(metadata, name, source, &fingerprint));

    match &source.location {
        SourceLocation::Git { repo, branch } => update_git_source(
            paths,
            name,
            source,
            repo,
            branch,
            &target,
            &fingerprint,
            metadata.as_ref(),
        ),
        SourceLocation::Archive { url } => update_archive_source(
            paths,
            name,
            source,
            url,
            &target,
            &fingerprint,
            metadata.as_ref(),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn update_git_source(
    paths: &DocumentPaths,
    name: &str,
    source: &ConfiguredSource,
    repo: &str,
    branch: &str,
    target: &Path,
    fingerprint: &str,
    metadata: Option<&SourceMetadata>,
) -> Result<SourceUpdateResult, String> {
    let revision = remote_revision(&paths.root, repo, branch)?;
    if let Some(metadata) = metadata
        && metadata.revision == revision
    {
        return Ok(unchanged_result(name, revision, metadata.documents));
    }

    let checkout = paths.sources.join(format!(".{name}.checkout"));
    let staging = paths.sources.join(format!(".{name}.staging"));
    remove_internal_dir(&checkout);
    remove_internal_dir(&staging);

    let result = (|| {
        run_git(
            &paths.root,
            [
                OsStr::new("clone"),
                OsStr::new("--depth"),
                OsStr::new("1"),
                OsStr::new("--single-branch"),
                OsStr::new("--no-local"),
                OsStr::new("--no-tags"),
                OsStr::new("--branch"),
                OsStr::new(branch),
                OsStr::new("--"),
                OsStr::new(repo),
                checkout.as_os_str(),
            ],
        )?;
        let checked_out = git_revision(&paths.root, &checkout)?;
        if checked_out != revision {
            return Err(format!(
                "remote branch moved while updating (expected {revision}, checked out {checked_out}); retry"
            ));
        }
        let commit_count = run_git(
            &paths.root,
            [
                OsStr::new("-C"),
                checkout.as_os_str(),
                OsStr::new("rev-list"),
                OsStr::new("--count"),
                OsStr::new("HEAD"),
            ],
        )?;
        if commit_count.trim() != "1" {
            return Err("git did not produce the required single-commit checkout".to_owned());
        }
        fs::create_dir(&staging)
            .map_err(|error| format!("could not create staging directory: {error}"))?;
        let documents = install_selected_documents(&checkout, &staging, source)?;
        let document_count = u32::try_from(documents).unwrap_or(u32::MAX);
        let metadata = SourceMetadata {
            version: METADATA_VERSION,
            source: name.to_owned(),
            kind: SourceMetadataKind::Git,
            repo: Some(repo.to_owned()),
            branch: Some(branch.to_owned()),
            url: None,
            revision: revision.clone(),
            config_fingerprint: fingerprint.to_owned(),
            documents: document_count,
            etag: None,
            last_modified: None,
        };
        activate_source(&staging, target, &metadata)?;
        Ok(SourceUpdateResult {
            source: name.to_owned(),
            action: SourceUpdateAction::Updated,
            revision: Some(revision),
            documents: Some(document_count),
            error: None,
        })
    })();

    remove_internal_dir(&checkout);
    remove_internal_dir(&staging);
    result
}

#[allow(clippy::too_many_arguments)]
fn update_archive_source(
    paths: &DocumentPaths,
    name: &str,
    source: &ConfiguredSource,
    url: &str,
    target: &Path,
    fingerprint: &str,
    metadata: Option<&SourceMetadata>,
) -> Result<SourceUpdateResult, String> {
    let download = paths.sources.join(format!(".{name}.download"));
    let checkout = paths.sources.join(format!(".{name}.checkout"));
    let staging = paths.sources.join(format!(".{name}.staging"));
    remove_internal_dir(&download);
    remove_internal_dir(&checkout);
    remove_internal_dir(&staging);

    let validators = metadata.and_then(|metadata| {
        if metadata.etag.is_none() && metadata.last_modified.is_none() {
            None
        } else {
            Some(Validators {
                etag: metadata.etag.clone(),
                last_modified: metadata.last_modified.clone(),
            })
        }
    });
    let result = (|| match download_archive(url, &download, validators.as_ref())? {
        DownloadOutcome::NotModified => {
            let metadata = metadata.ok_or_else(|| {
                "archive server returned not-modified without installed metadata".to_owned()
            })?;
            Ok(unchanged_result(
                name,
                metadata.revision.clone(),
                metadata.documents,
            ))
        }
        DownloadOutcome::Downloaded {
            revision,
            validators,
        } => {
            if let Some(metadata) = metadata
                && metadata.revision == revision
            {
                return Ok(unchanged_result(name, revision, metadata.documents));
            }
            extract_archive(&download, &checkout)?;
            fs::create_dir(&staging)
                .map_err(|error| format!("could not create staging directory: {error}"))?;
            let documents = install_selected_documents(&checkout, &staging, source)?;
            let document_count = u32::try_from(documents).unwrap_or(u32::MAX);
            let metadata = SourceMetadata {
                version: METADATA_VERSION,
                source: name.to_owned(),
                kind: SourceMetadataKind::Archive,
                repo: None,
                branch: None,
                url: Some(url.to_owned()),
                revision: revision.clone(),
                config_fingerprint: fingerprint.to_owned(),
                documents: document_count,
                etag: validators.etag,
                last_modified: validators.last_modified,
            };
            activate_source(&staging, target, &metadata)?;
            Ok(SourceUpdateResult {
                source: name.to_owned(),
                action: SourceUpdateAction::Updated,
                revision: Some(revision),
                documents: Some(document_count),
                error: None,
            })
        }
    })();

    remove_internal_dir(&download);
    remove_internal_dir(&checkout);
    remove_internal_dir(&staging);
    result
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

fn metadata_matches(
    metadata: &SourceMetadata,
    name: &str,
    source: &ConfiguredSource,
    fingerprint: &str,
) -> bool {
    metadata.version == METADATA_VERSION
        && metadata.source == name
        && metadata.config_fingerprint == fingerprint
        && match &source.location {
            SourceLocation::Git { repo, branch } => {
                metadata.kind == SourceMetadataKind::Git
                    && metadata.repo.as_deref() == Some(repo)
                    && metadata.branch.as_deref() == Some(branch)
                    && metadata.url.is_none()
            }
            SourceLocation::Archive { url } => {
                metadata.kind == SourceMetadataKind::Archive
                    && metadata.url.as_deref() == Some(url)
                    && metadata.repo.is_none()
                    && metadata.branch.is_none()
            }
        }
}

fn activate_source(staging: &Path, target: &Path, metadata: &SourceMetadata) -> Result<(), String> {
    let metadata_text = toml::to_string_pretty(metadata)
        .map_err(|error| format!("could not encode source metadata: {error}"))?;
    fs::write(staging.join(SOURCE_METADATA_FILE), metadata_text)
        .map_err(|error| format!("could not write source metadata: {error}"))?;
    replace_directory(staging, target)
}

fn remote_revision(working_directory: &Path, repo: &str, branch: &str) -> Result<String, String> {
    let reference = format!("refs/heads/{branch}");
    let output = run_git(
        working_directory,
        [
            OsStr::new("ls-remote"),
            OsStr::new("--exit-code"),
            OsStr::new("--refs"),
            OsStr::new("--"),
            OsStr::new(repo),
            OsStr::new(&reference),
        ],
    )?;
    let line = output
        .lines()
        .next()
        .ok_or_else(|| format!("branch '{branch}' was not found in '{repo}'"))?;
    let revision = line.split_whitespace().next().unwrap_or_default();
    if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("git returned an invalid remote revision".to_owned());
    }
    Ok(revision.to_ascii_lowercase())
}

fn git_revision(working_directory: &Path, checkout: &Path) -> Result<String, String> {
    let output = run_git(
        working_directory,
        [
            OsStr::new("-C"),
            checkout.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("HEAD"),
        ],
    )?;
    Ok(output.trim().to_ascii_lowercase())
}

fn run_git<'a>(
    working_directory: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<String, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(working_directory)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            format!("git failed: {detail}")
        });
    }
    String::from_utf8(output.stdout).map_err(|_| "git output was not UTF-8".to_owned())
}

fn install_selected_documents(
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
    collect_markdown(&root, &root, 0, &mut candidates)?;
    candidates.retain(|(relative, _)| {
        (source.include.is_empty()
            || source
                .include
                .iter()
                .any(|selector| selector_matches(relative, selector)))
            && !source
                .exclude
                .iter()
                .any(|selector| selector_matches(relative, selector))
    });
    candidates.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let mut names = BTreeSet::new();
    for (relative, path) in &candidates {
        let public_name = path
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format!("Markdown filename is not UTF-8: {}", relative.display()))?
            .to_ascii_lowercase();
        if !names.insert(public_name.clone()) {
            return Err(format!(
                "multiple selected files use the document name '{public_name}'; adjust include/exclude"
            ));
        }
        let filename = path
            .file_name()
            .ok_or_else(|| format!("invalid Markdown path: {}", relative.display()))?;
        fs::copy(path, staging.join(filename))
            .map_err(|error| format!("could not install '{}': {error}", relative.display()))?;
    }
    Ok(candidates.len())
}

fn collect_markdown(
    root: &Path,
    directory: &Path,
    depth: usize,
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
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if entry.file_name() != OsStr::new(".git") {
                collect_markdown(root, &entry.path(), depth + 1, output)?;
            }
        } else if file_type.is_file() && is_markdown_file(&entry.path()) {
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
    Ok(())
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

fn selector_matches(relative: &Path, selector: &str) -> bool {
    let selector = Path::new(selector);
    relative == selector || relative.starts_with(selector)
}

fn read_metadata(directory: &Path) -> Option<SourceMetadata> {
    let path = directory.join(SOURCE_METADATA_FILE);
    let file = fs::File::open(path).ok()?;
    let text = crate::bounded::read_utf8(file, MAX_METADATA_BYTES, "source metadata").ok()?;
    toml::from_str(&text).ok()
}

fn replace_directory(staging: &Path, target: &Path) -> Result<(), String> {
    recover_directory(target)?;
    let backup = target.with_extension("backup");
    let had_target = target.exists();
    if had_target {
        fs::rename(target, &backup)
            .map_err(|error| format!("could not preserve previous source: {error}"))?;
    }
    if let Err(error) = fs::rename(staging, target) {
        if had_target {
            let _ = fs::rename(&backup, target);
        }
        return Err(format!("could not activate updated source: {error}"));
    }
    remove_internal_dir(&backup);
    Ok(())
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
        thread,
    };

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{
        ConfiguredSource, DocumentPaths, SourceLocation, SourceUpdateAction,
        install_selected_documents, recover_directory, source_fingerprint, try_update_one_source,
    };

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

    fn zip_document() -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("tool.md", SimpleFileOptions::default())
            .expect("start ZIP file");
        writer.write_all(b"# tool").expect("write ZIP file");
        writer.finish().expect("finish ZIP").into_inner()
    }

    #[test]
    fn install_is_flat_and_rejects_public_name_collisions() {
        let root = temp("collision");
        let checkout = root.join("checkout");
        let staging = root.join("staging");
        fs::create_dir_all(checkout.join("one")).expect("create first directory");
        fs::create_dir_all(checkout.join("two")).expect("create second directory");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(checkout.join("one/tool.md"), "# one").expect("write first");
        fs::write(checkout.join("two/tool.markdown"), "# two").expect("write second");
        assert!(
            install_selected_documents(&checkout, &staging, &source("."))
                .expect_err("reject duplicate public name")
                .contains("document name 'tool'")
        );
        fs::remove_dir_all(root).expect("remove fixture");
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
            sources: root.join("documents/sources"),
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
            sources: root.join("documents/sources"),
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
