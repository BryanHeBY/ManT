//! Path identity and Windows matching over the shared settings path rules.
use mant_sources::settings::{
    WindowsPathKey, is_absolute_configuration_path, is_relative_configuration_path,
    windows_path_key,
};
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

#[derive(Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum ConfigPathKey {
    Windows(WindowsPathKey),
    Native(PathBuf),
}

pub(super) fn config_path_key(path: &Path, windows: bool) -> ConfigPathKey {
    if windows && let Some(key) = windows_path_key(path) {
        return ConfigPathKey::Windows(key);
    }
    ConfigPathKey::Native(path.to_owned())
}

pub(super) fn is_absolute_windows_path(path: &OsStr) -> bool {
    is_absolute_configuration_path(Path::new(path), true)
}

pub(super) fn windows_paths_equivalent(left: &Path, right: &Path) -> bool {
    if [left, right].iter().any(|path| {
        !is_absolute_configuration_path(path, true) && !is_relative_configuration_path(path, true)
    }) {
        return false;
    }
    let left_key = windows_path_key(left);
    (left_key.is_some() && left_key == windows_path_key(right))
        || fs::canonicalize(left)
            .ok()
            .zip(fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| {
                let left_key = windows_path_key(&left);
                left_key.is_some() && left_key == windows_path_key(&right)
            })
}

pub(super) fn deduplicate_windows_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(config_path_key(path, true)))
        .collect()
}
