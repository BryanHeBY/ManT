//! Existing macos manual-root dialect policy; no host subprocesses.
use super::{
    HashMap, OsString, Path, PathBuf, deduplicate_paths, env, environment_value, fs, read_config,
};
use super::{bsd::macos_configuration_roots, read_path_list};
pub(super) fn macos_configured_manual_roots(
    environment: &HashMap<OsString, OsString>,
) -> Vec<PathBuf> {
    let mut roots = macos_path_manual_roots(environment);
    roots.extend(macos_developer_manual_roots(environment));
    roots.extend(["/usr/share/man", "/usr/local/share/man"].map(PathBuf::from));
    roots.extend(macos_configuration_roots(Path::new("/etc/man.conf")));
    roots.retain(|path| path.is_dir());
    if !roots.is_empty() {
        return deduplicate_paths(roots);
    }

    // `path_helper` maintains these files on newer macOS installations.  They
    // are a useful fallback when a shell has not exported MANPATH yet.
    let mut fallback = read_path_list(Path::new("/etc/manpaths"));
    let directory = Path::new("/etc/manpaths.d");
    let Ok(entries) = fs::read_dir(directory) else {
        return deduplicate_paths(fallback);
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        fallback.extend(read_path_list(&entry.path()));
    }
    deduplicate_paths(fallback)
}

pub(super) fn macos_path_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(path) = environment_value(environment, "PATH") else {
        return roots;
    };
    for executable_dir in env::split_paths(path) {
        let mut candidates = vec![executable_dir.join("man"), executable_dir.join("MAN")];
        if executable_dir.file_name().is_some_and(|name| name == "bin")
            && let Some(prefix) = executable_dir.parent()
        {
            candidates.extend([prefix.join("share/man"), prefix.join("man")]);
        }
        if let Some(manual_root) = candidates.into_iter().find(|path| path.is_dir()) {
            roots.push(manual_root);
        }
    }
    roots
}

pub(super) fn macos_developer_manual_roots(
    environment: &HashMap<OsString, OsString>,
) -> Vec<PathBuf> {
    let Some(developer) = macos_developer_directory(environment) else {
        return Vec::new();
    };
    developer_manual_roots(&developer)
}

pub(super) fn developer_manual_roots(developer: &Path) -> Vec<PathBuf> {
    let mut roots = vec![developer.join("usr/share/man")];
    let platforms = developer.join("Platforms");
    let Ok(platforms) = fs::read_dir(platforms) else {
        return roots;
    };
    let mut platforms = platforms.flatten().collect::<Vec<_>>();
    platforms.sort_unstable_by_key(fs::DirEntry::file_name);
    for platform in platforms {
        let sdks = platform.path().join("Developer/SDKs");
        let Ok(sdks) = fs::read_dir(sdks) else {
            continue;
        };
        let mut sdks = sdks.flatten().collect::<Vec<_>>();
        sdks.sort_unstable_by_key(fs::DirEntry::file_name);
        roots.extend(sdks.into_iter().map(|sdk| sdk.path().join("usr/share/man")));
    }
    roots.into_iter().filter(|path| path.is_dir()).collect()
}

pub(super) fn macos_developer_directory(
    environment: &HashMap<OsString, OsString>,
) -> Option<PathBuf> {
    environment_value(environment, "DEVELOPER_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| read_selected_developer_directory(Path::new("/var/db/xcode_select_link")))
        .or_else(|| {
            read_selected_developer_directory(Path::new("/usr/share/xcode-select/xcode_dir_path"))
        })
        .or_else(|| {
            PathBuf::from("/Applications/Xcode.app/Contents/Developer")
                .is_dir()
                .then(|| PathBuf::from("/Applications/Xcode.app/Contents/Developer"))
        })
        .or_else(|| {
            PathBuf::from("/Library/Developer/CommandLineTools")
                .is_dir()
                .then(|| PathBuf::from("/Library/Developer/CommandLineTools"))
        })
}

pub(super) fn read_selected_developer_directory(path: &Path) -> Option<PathBuf> {
    if let Ok(target) = fs::read_link(path) {
        let target = if target.is_absolute() {
            target
        } else {
            path.parent()?.join(target)
        };
        return target.is_dir().then_some(target);
    }
    read_config(path)
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| path.is_dir())
}
