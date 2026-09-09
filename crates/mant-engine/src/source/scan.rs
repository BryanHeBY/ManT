//! Local hierarchy scanning, locale precedence, and physical source identity.

use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use super::ManualPage;

const SUPPORTED_COMPRESSION_SUFFIXES: [&str; 2] = [".gz", ".zst"];
pub(crate) fn deduplicate_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let current_directory = env::current_dir().ok();
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .filter_map(|path| {
            if path.is_absolute() {
                Some(path)
            } else {
                current_directory.as_ref().map(|current| current.join(path))
            }
        })
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

pub(super) fn current_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .into_iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.is_empty()))
        .and_then(|value| normalize_locale(&value))
}

pub(super) fn normalize_locale(locale: &str) -> Option<String> {
    let locale = locale.split(['.', '@', ':']).next()?.trim();
    (!locale.is_empty() && locale != "C" && locale != "POSIX").then(|| locale.to_owned())
}

pub(super) fn scan_manual_root(root: &Path, locale: Option<&str>) -> Vec<ManualPage> {
    let mut candidates = BTreeMap::<(String, String), (u8, PathBuf)>::new();
    scan_directory(root, root, locale, &mut candidates);
    candidates
        .into_iter()
        .map(|((name, section), (_, path))| ManualPage {
            name,
            section,
            path,
            manual_root: root.to_path_buf(),
        })
        .collect()
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    locale: Option<&str>,
    candidates: &mut BTreeMap<(String, String), (u8, PathBuf)>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            scan_directory(root, &path, locale, candidates);
            continue;
        }
        if !file_type.is_file() && !file_type.is_symlink() {
            continue;
        }
        // Follow an explicit leaf link only far enough to prove it names a
        // regular file. Directory links are never traversed, and broken links
        // are not indexed.
        if !fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        let Some((name, section)) = manual_identity(root, &path) else {
            continue;
        };
        let priority = locale_priority(root, &path, locale);
        let key = (name, section);
        match candidates.get(&key) {
            Some((current_priority, current_path))
                if (*current_priority, current_path) <= (priority, &path) => {}
            _ => {
                candidates.insert(key, (priority, path));
            }
        }
    }
}

fn manual_identity(root: &Path, path: &Path) -> Option<(String, String)> {
    let relative = path.strip_prefix(root).ok()?;
    let section_directory = relative.parent()?.components().find_map(|component| {
        let component = component.as_os_str().to_str()?;
        component
            .strip_prefix("man")
            .filter(|section| !section.is_empty())
            .map(ToOwned::to_owned)
    });
    let filename = path.file_name()?.to_str()?;
    let stem = SUPPORTED_COMPRESSION_SUFFIXES
        .iter()
        .find_map(|suffix| filename.strip_suffix(suffix))
        .unwrap_or(filename);
    let (name, file_section) = stem.rsplit_once('.')?;
    let section_matches_directory = section_directory.as_ref().is_some_and(|directory| {
        file_section == directory
            || file_section
                .strip_prefix(directory)
                .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(char::is_alphabetic))
    });
    let flat_root_page = relative.components().count() == 1 && valid_flat_section(file_section);
    if name.is_empty() || (!section_matches_directory && !flat_root_page) {
        return None;
    }
    Some((name.to_owned(), file_section.to_owned()))
}

fn valid_flat_section(section: &str) -> bool {
    let mut characters = section.chars();
    match characters.next() {
        Some('1'..='9') => characters.all(char::is_alphabetic),
        Some('l' | 'n') => characters.next().is_none(),
        _ => false,
    }
}

fn locale_priority(root: &Path, path: &Path, locale: Option<&str>) -> u8 {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let localized = relative.components().next().is_some_and(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|component| !component.starts_with("man"))
    });
    let Some(locale) = locale else {
        return u8::from(localized);
    };
    if !localized {
        return 2;
    }
    let language = locale.split('_').next().unwrap_or(locale);
    let component = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .unwrap_or_default();
    if component == locale {
        0
    } else if component == language {
        1
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::{locale_priority, manual_identity};
    use std::path::Path;

    #[test]
    fn path_identity_preserves_exact_sections_compression_and_nested_hierarchies() {
        let root = Path::new("fixture-root");
        for (path, identity) in [
            ("tool.1", Some(("tool", "1"))),
            ("tool.n.zst", Some(("tool", "n"))),
            ("man3/tool.3p.gz", Some(("tool", "3p"))),
            (
                "fr/man3/deep/another/tool.3type.zst",
                Some(("tool", "3type")),
            ),
            ("man0/tool.0p", Some(("tool", "0p"))),
            ("man3/tool.30", None),
            ("man4/tool.3", None),
            ("man3/tool.3.xz", None),
            ("man3/tool.3.GZ", None),
            ("elsewhere/tool.1", None),
            ("tool.0", None),
            (".1", None),
        ] {
            assert_eq!(
                manual_identity(root, &root.join(path)),
                identity.map(|(name, section)| (name.to_owned(), section.to_owned())),
                "{path}"
            );
        }
    }

    #[test]
    fn locale_priority_keeps_exact_language_default_and_other_buckets() {
        let root = Path::new("fixture-root");
        for (path, selected, no_locale) in [
            ("zh_CN/man1/tool.1", 0, 1),
            ("zh/man1/tool.1", 1, 1),
            ("man1/tool.1", 2, 0),
            ("zh_TW/man1/tool.1", 3, 1),
        ] {
            assert_eq!(
                locale_priority(root, &root.join(path), Some("zh_CN")),
                selected
            );
            assert_eq!(locale_priority(root, &root.join(path), None), no_locale);
        }
    }
}
