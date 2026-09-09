//! Resolves directly runnable programs using native host conventions.

use std::{collections::BTreeMap, env, ffi::OsStr, path::PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Read-only executable lookup against a caller-owned environment snapshot.
///
/// Construction borrows the map without reading the process environment.
/// Lookup applies the native `PATH`/`PATHEXT` rules and inspects candidate file
/// metadata; it never starts a program or creates cache state. The snapshot
/// does not freeze the filesystem, and a returned path grants no execution
/// authority to a caller.
pub struct ExecutableLookup<'env> {
    environment: &'env BTreeMap<String, String>,
}

impl<'env> ExecutableLookup<'env> {
    /// Borrow an existing environment without copying or refreshing it.
    #[must_use]
    pub const fn new(environment: &'env BTreeMap<String, String>) -> Self {
        Self { environment }
    }

    /// Read one borrowed value with native environment-name case rules.
    ///
    /// Exact spelling wins. Windows additionally accepts ASCII case variants;
    /// other platforms retain case-sensitive lookup.
    #[must_use]
    pub fn environment_value(&self, name: &str) -> Option<&'env str> {
        environment_value(self.environment, name)
    }

    /// Find the first directly runnable candidate without executing it.
    ///
    /// Candidate precedence and executable checks use the current platform's
    /// conventions. A missing `PATH` or eligible file returns `None`.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        find_executable(name, self.environment)
    }
}

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

/// Locate one directly runnable program using the current host's `PATH` and
/// native executable-suffix rules without spawning it.
#[must_use]
pub fn find_host_executable(name: &str) -> Option<PathBuf> {
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    ExecutableLookup::new(&environment).find(name)
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
        windows_name_candidates(name, env::var("PATHEXT").ok().as_deref())
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
    fn lookup_borrows_values_and_keeps_native_environment_case_rules() {
        use std::collections::BTreeMap;

        let mut environment = BTreeMap::from([("Path".to_owned(), "mixed".to_owned())]);
        let lookup = super::ExecutableLookup::new(&environment);
        let value = lookup.environment_value("Path").unwrap();
        assert!(std::ptr::eq(value.as_ptr(), environment["Path"].as_ptr()));
        assert_eq!(
            lookup.environment_value("PATH"),
            cfg!(windows).then_some("mixed")
        );
        assert_eq!(lookup.environment_value("missing"), None);

        environment.insert("PATH".to_owned(), "exact".to_owned());
        assert_eq!(
            super::ExecutableLookup::new(&environment).environment_value("PATH"),
            Some("exact"),
            "exact spelling retains precedence even with a case-colliding map"
        );
        assert_eq!(
            super::ExecutableLookup::new(&BTreeMap::new()).find("tool"),
            None,
            "lookup must not fall back to the process PATH"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn executable_probe_inspects_candidates_without_running_them() {
        use std::{collections::BTreeMap, fs, path::PathBuf, time::SystemTime};

        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        );
        fs::create_dir_all(&target).unwrap();
        let directory = target.join(format!("executable-probe-{}-{nonce}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let fixture = Fixture(directory);
        #[cfg(unix)]
        let (name, script) = ("probe", "#!/bin/sh\n: > \"$0.was-run\"\nexit 1\n");
        #[cfg(windows)]
        let (name, script) = (
            "probe.CMD",
            "@echo off\r\necho ran>\"%~f0.was-run\"\r\nexit /b 1\r\n",
        );
        let candidate = fixture.0.join(name);
        fs::write(&candidate, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&candidate, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let environment = BTreeMap::from([(
            "PATH".to_owned(),
            std::env::join_paths([&fixture.0])
                .unwrap()
                .into_string()
                .unwrap(),
        )]);
        assert_eq!(
            super::ExecutableLookup::new(&environment).find(name),
            Some(candidate.clone())
        );
        assert!(!fixture.0.join(format!("{name}.was-run")).exists());
        assert_eq!(fs::read_to_string(candidate).unwrap(), script);
    }

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
