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
    windows_name_candidates(name, environment_value(environment, "PATHEXT"))
        .into_iter()
        .skip(1)
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

/// Names that preserve an exact document lookup before applying native host
/// command-suffix conventions.
pub(crate) fn query_name_candidates(name: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        return windows_name_candidates(name, env::var("PATHEXT").ok().as_deref());
    }
    #[cfg(not(windows))]
    {
        vec![name.to_owned()]
    }
}

/// Model Windows command-name elision without depending on the build host.
///
/// The exact name remains first because registered documents may intentionally
/// have no executable suffix. Only extensionless names are expanded.
#[cfg(any(windows, test))]
fn windows_name_candidates(name: &str, pathext: Option<&str>) -> Vec<String> {
    let mut candidates = vec![name.to_owned()];
    if std::path::Path::new(name).extension().is_some() {
        return candidates;
    }

    let extensions = pathext
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".COM;.EXE;.BAT;.CMD");
    for extension in extensions.split(';').map(str::trim) {
        if extension.is_empty() || extension.contains(['/', '\\']) {
            continue;
        }
        let extension = if extension.starts_with('.') {
            extension.to_owned()
        } else {
            format!(".{extension}")
        };
        let candidate = format!("{name}{extension}");
        if !candidates
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&candidate))
        {
            candidates.push(candidate);
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::windows_name_candidates;

    #[test]
    fn windows_candidates_keep_exact_names_before_pathext_order() {
        assert_eq!(
            windows_name_candidates("cargo", Some(".EXE;.CMD;.PS1")),
            ["cargo", "cargo.EXE", "cargo.CMD", "cargo.PS1"]
        );
    }

    #[test]
    fn windows_candidates_do_not_expand_an_explicit_suffix() {
        assert_eq!(
            windows_name_candidates("where.exe", Some(".EXE;.CMD")),
            ["where.exe"]
        );
    }

    #[test]
    fn windows_candidates_use_the_native_default_when_pathext_is_absent() {
        assert_eq!(
            windows_name_candidates("tool", None),
            ["tool", "tool.COM", "tool.EXE", "tool.BAT", "tool.CMD"]
        );
    }

    #[test]
    fn windows_candidates_normalise_and_deduplicate_extensions() {
        assert_eq!(
            windows_name_candidates("tool", Some(" EXE ;.exe;.CMD;bad/path;.PS1 ")),
            ["tool", "tool.EXE", "tool.CMD", "tool.PS1"]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_queries_never_elide_executable_suffixes() {
        assert_eq!(super::query_name_candidates("tool"), ["tool"]);
    }
}

#[cfg(not(any(unix, windows)))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}
