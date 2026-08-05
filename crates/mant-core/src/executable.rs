//! Resolves directly runnable programs using native host conventions.

use std::{collections::BTreeMap, env, ffi::OsStr, path::PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Look up an environment value while respecting Windows' case-insensitive
/// variable names.
pub(crate) fn environment_value<'a>(
    environment: &'a BTreeMap<String, String>,
    name: &str,
) -> Option<&'a str> {
    if let Some(value) = environment.get(name) {
        return Some(value.as_str());
    }

    #[cfg(windows)]
    {
        environment
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Find a program that the current host can execute directly.
pub(crate) fn find_executable(
    name: &str,
    environment: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    let path = environment_value(environment, "PATH")?;
    let names = executable_names(name, environment);
    env::split_paths(OsStr::new(path))
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn executable_names(name: &str, _environment: &BTreeMap<String, String>) -> Vec<String> {
    vec![name.to_owned()]
}

#[cfg(windows)]
fn executable_names(name: &str, environment: &BTreeMap<String, String>) -> Vec<String> {
    if std::path::Path::new(name).extension().is_some() {
        return vec![name.to_owned()];
    }
    let extensions = environment_value(environment, "PATHEXT")
        .filter(|value| !value.is_empty())
        .unwrap_or(".COM;.EXE;.BAT;.CMD");
    extensions
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| format!("{name}{extension}"))
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn executable_names(name: &str, _environment: &BTreeMap<String, String>) -> Vec<String> {
    vec![name.to_owned()]
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

#[cfg(not(any(unix, windows)))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}
