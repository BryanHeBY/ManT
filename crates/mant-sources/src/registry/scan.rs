//! Bounded filesystem discovery for one immutable registry snapshot.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::document_path::{markdown_extension_priority, normalize_relative_document_path};
use crate::metadata::validate_source_directory;
use crate::{SOURCE_METADATA_FILE, SourceConfigError};

const MAX_DOCUMENT_DEPTH: usize = 32;
const MAX_REGISTERED_DOCUMENTS: usize = 10_000;

pub(super) fn source_directory_ready(directory: &Path) -> bool {
    if validate_source_directory(directory) != Ok(true) {
        return false;
    }
    fs::symlink_metadata(directory.join(SOURCE_METADATA_FILE)).is_ok_and(|metadata| {
        let file_type = metadata.file_type();
        file_type.is_file() && !file_type.is_symlink()
    })
}

/// Recursively scan one origin. Personal documents accept explicit leaf-file
/// links, while managed source caches never follow links. Directory links are
/// never traversed.
pub(super) fn scan_directory(
    directory: &Path,
    personal_documents: bool,
) -> Result<Vec<(String, PathBuf)>, SourceConfigError> {
    let mut candidates = BTreeMap::<String, (u8, PathBuf)>::new();
    scan_directory_into(directory, directory, personal_documents, 0, &mut candidates)?;
    Ok(candidates
        .into_iter()
        .map(|(name, (_, path))| (name, path))
        .collect())
}

pub(crate) fn managed_document_count(directory: &Path) -> Result<u32, String> {
    scan_directory(directory, false)
        .and_then(|documents| {
            u32::try_from(documents.len()).map_err(|_| {
                SourceConfigError::new("managed document count exceeds the supported range")
            })
        })
        .map_err(|error| error.to_string())
}

fn scan_directory_into(
    root: &Path,
    directory: &Path,
    allow_leaf_symlinks: bool,
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
            // Follow only the explicitly named leaf far enough to prove its
            // target is a regular file. Its logical identity still comes from
            // the link path; broken and directory links remain invisible.
            if !allow_leaf_symlinks
                || !fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_file())
            {
                continue;
            }
        }
        if file_type.is_dir() {
            scan_directory_into(
                root,
                &entry.path(),
                allow_leaf_symlinks,
                depth + 1,
                candidates,
            )?;
            continue;
        }
        if !file_type.is_file() && !file_type.is_symlink() {
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

fn markdown_document_path(root: &Path, path: &Path) -> Option<String> {
    markdown_extension_priority(path)?;
    let relative = path.strip_prefix(root).ok()?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let logical = parent.join(path.file_stem()?);
    let logical = normalize_relative_document_path(&logical)?;
    Some(logical)
}

#[cfg(test)]
mod tests;
