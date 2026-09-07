//! Scan and materialize selected Markdown without activating an installation.
#[cfg(unix)]
use super::transaction::sync_directory;
use super::transaction::sync_file;
use super::{
    BTreeMap, ConfiguredSource, MAX_DOCUMENT_BYTES, MAX_SOURCE_BYTES, MAX_SOURCE_DEPTH,
    MAX_SOURCE_DOCUMENTS, MAX_SOURCE_ENTRIES, Path, PathBuf, fs, markdown_extension_priority,
    normalize_relative_document_path,
};
use std::ffi::OsStr;
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

pub(super) fn markdown_logical_path(relative: &Path) -> Result<String, String> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let stem = relative
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("Markdown filename is not UTF-8: {}", relative.display()))?;
    let path = parent.join(stem);
    normalize_relative_document_path(&path).ok_or_else(|| {
        format!(
            "Markdown path contains an unsupported component: {}",
            relative.display()
        )
    })
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

pub(super) fn is_markdown_file(path: &Path) -> bool {
    markdown_extension_priority(path).is_some()
}

pub(in crate::update) fn source_selects_markdown_path(
    source: &ConfiguredSource,
    relative: &Path,
) -> bool {
    is_markdown_file(relative) && source_selects_path(source, relative)
}

pub(super) fn source_selects_path(source: &ConfiguredSource, relative: &Path) -> bool {
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

pub(super) fn selector_matches(relative: &Path, selector: &str) -> bool {
    let selector = Path::new(selector);
    relative == selector || relative.starts_with(selector)
}
