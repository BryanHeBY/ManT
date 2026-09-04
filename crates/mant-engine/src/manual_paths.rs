//! Native manual-root discovery without invoking a host `man` program.
//!
//! Unix manual lookup is not governed by one portable directory list.  The
//! two common Linux implementations, the BSD family, and macOS each publish
//! configuration that affects the effective path.  This module reads the
//! small, declarative subset that determines *source roots*; it deliberately
//! does not inherit pager, formatter, cache, or locale behaviour from the
//! host implementation.

use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Component, Path, PathBuf},
};

use crate::source::deduplicate_paths;

#[cfg(unix)]
const DEFAULT_UNIX_MANUAL_ROOTS: [&str; 4] = [
    "/usr/local/share/man",
    "/usr/local/man",
    "/usr/share/man",
    "/usr/man",
];
const MAX_MANUAL_PATH_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_EXPANDED_CONFIG_PATHS: usize = 256;

/// Discover effective manual hierarchy roots for the current host.
///
/// `MANT_MANPATH` is a complete `ManT` override.  Otherwise `MANPATH` follows
/// conventional empty-component insertion, with host-derived defaults at each
/// empty component.  When neither variable is set, platform configuration is
/// read without spawning `man`, `manpath`, or any other external program.
#[must_use]
pub fn discover_manual_roots() -> Vec<PathBuf> {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    discover_manual_roots_from(&environment, host_default_manual_roots(&environment))
}

#[cfg(test)]
pub(crate) fn discover_manual_roots_with(
    environment: &HashMap<OsString, OsString>,
) -> Vec<PathBuf> {
    discover_manual_roots_from(environment, fallback_manual_roots(environment))
}

fn discover_manual_roots_from(
    environment: &HashMap<OsString, OsString>,
    defaults: Vec<PathBuf>,
) -> Vec<PathBuf> {
    if let Some(explicit) = environment.get(OsStr::new("MANT_MANPATH")) {
        return deduplicate_paths(
            env::split_paths(explicit).filter(|path| !path.as_os_str().is_empty()),
        );
    }

    if let Some(manpath) = environment.get(OsStr::new("MANPATH")) {
        let mut roots = Vec::new();
        for path in env::split_paths(manpath) {
            if path.as_os_str().is_empty() {
                roots.extend(defaults.iter().cloned());
            } else {
                roots.push(path);
            }
        }
        return deduplicate_paths(roots);
    }
    defaults
}

fn host_default_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let native = match host_platform() {
        ManualPathPlatform::Linux => linux_configured_manual_roots(environment),
        ManualPathPlatform::Macos => macos_configured_manual_roots(environment),
        ManualPathPlatform::Windows => mant_configured_manual_roots(),
        ManualPathPlatform::OtherUnix => mandoc_configured_manual_roots(Path::new("/etc/man.conf")),
    };
    if native.is_empty() {
        fallback_manual_roots(environment)
    } else {
        let mut roots = native;
        roots.extend(supplemental_manual_roots(environment));
        deduplicate_paths(roots)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManualPathPlatform {
    Linux,
    Macos,
    Windows,
    OtherUnix,
}

const fn host_platform() -> ManualPathPlatform {
    if cfg!(windows) {
        ManualPathPlatform::Windows
    } else if cfg!(target_os = "macos") {
        ManualPathPlatform::Macos
    } else if cfg!(target_os = "linux") {
        ManualPathPlatform::Linux
    } else {
        ManualPathPlatform::OtherUnix
    }
}

#[cfg(unix)]
fn fallback_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let mut roots = supplemental_manual_roots(environment);
    roots.extend(path_derived_manual_roots(environment));
    roots.extend(DEFAULT_UNIX_MANUAL_ROOTS.map(PathBuf::from));
    deduplicate_paths(roots)
}

#[cfg(not(unix))]
fn fallback_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    deduplicate_paths(supplemental_manual_roots(environment))
}

fn supplemental_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    supplemental_manual_roots_for(environment, host_platform())
}

fn supplemental_manual_roots_for(
    environment: &HashMap<OsString, OsString>,
    platform: ManualPathPlatform,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if platform == ManualPathPlatform::Windows {
        if let Some(data_root) = environment.get(OsStr::new("APPDATA")).map(PathBuf::from) {
            roots.push(data_root.join("ManT").join("man"));
        }
        if let Some(profile) = environment
            .get(OsStr::new("USERPROFILE"))
            .map(PathBuf::from)
        {
            roots.push(profile.join(".local/share/man"));
        }
        return roots;
    }

    if let Some(home) = environment.get(OsStr::new("HOME")).map(PathBuf::from) {
        roots.push(home.join(".local/share/man"));
        roots.push(home.join(".local/man"));
        roots.push(home.join("man"));
    }
    if let Some(data_home) = environment
        .get(OsStr::new("XDG_DATA_HOME"))
        .map(PathBuf::from)
    {
        roots.push(data_home.join("man"));
    }
    if let Some(data_dirs) = environment.get(OsStr::new("XDG_DATA_DIRS")) {
        roots.extend(env::split_paths(data_dirs).map(|root| root.join("man")));
    }
    roots
}

#[cfg(unix)]
fn path_derived_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = environment.get(OsStr::new("PATH")) {
        for binary_dir in env::split_paths(path) {
            roots.extend(unmapped_man_db_roots(&binary_dir));
        }
    }
    roots
}

fn linux_configured_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let user_config = environment
        .get(OsStr::new("HOME"))
        .map(PathBuf::from)
        .map(|home| home.join(".manpath"));
    let system_configurations = [
        PathBuf::from("/etc/man_db.conf"),
        PathBuf::from("/etc/manpath.config"),
        PathBuf::from("/usr/local/etc/man_db.conf"),
    ];
    let configuration = user_config.filter(|path| path.is_file()).or_else(|| {
        system_configurations
            .into_iter()
            .find(|path| path.is_file())
    });
    if let Some(configuration) = configuration {
        let config = read_config(&configuration)
            .map(|text| parse_man_db_config(&text))
            .unwrap_or_default();
        let roots = man_db_manual_roots(environment, &config);
        if !roots.is_empty() {
            return roots;
        }
    }
    mandoc_configured_manual_roots(Path::new("/etc/man.conf"))
}

fn macos_configured_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
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

fn macos_path_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(path) = environment.get(OsStr::new("PATH")) else {
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

fn macos_developer_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let Some(developer) = macos_developer_directory(environment) else {
        return Vec::new();
    };
    developer_manual_roots(&developer)
}

fn developer_manual_roots(developer: &Path) -> Vec<PathBuf> {
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

fn macos_developer_directory(environment: &HashMap<OsString, OsString>) -> Option<PathBuf> {
    environment
        .get(OsStr::new("DEVELOPER_DIR"))
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

fn read_selected_developer_directory(path: &Path) -> Option<PathBuf> {
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

fn mant_configured_manual_roots() -> Vec<PathBuf> {
    mant_sources::document_paths()
        .ok()
        .map(|paths| bsd_configured_manual_roots(&paths.root.join("man.conf")))
        .unwrap_or_default()
}

fn mandoc_configured_manual_roots(path: &Path) -> Vec<PathBuf> {
    read_config(path)
        .map(|text| parse_mandoc_manpaths(&text))
        .map(deduplicate_paths)
        .unwrap_or_default()
}

fn bsd_configured_manual_roots(path: &Path) -> Vec<PathBuf> {
    let Some(text) = read_config(path) else {
        return Vec::new();
    };
    let configuration = parse_bsd_man_config(&text);
    let mut roots = configuration.paths;
    if let Some(pattern) = configuration.include_pattern {
        for included in expand_path_pattern(&pattern) {
            let Some(text) = read_config(&included) else {
                continue;
            };
            roots.extend(parse_bsd_man_config(&text).paths);
        }
    }
    deduplicate_paths(roots)
}

fn macos_configuration_roots(path: &Path) -> Vec<PathBuf> {
    let Some(text) = read_config(path) else {
        return Vec::new();
    };
    let configuration = parse_bsd_man_config(&text);
    let mut roots = configuration.paths;
    let pattern = configuration
        .include_pattern
        .unwrap_or_else(|| PathBuf::from("/usr/local/etc/man.d/*.conf"));
    for included in expand_path_pattern(&pattern) {
        let Some(text) = read_config(&included) else {
            continue;
        };
        roots.extend(parse_bsd_man_config(&text).paths);
    }
    deduplicate_paths(roots)
}

fn read_config(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    (metadata.is_file() && metadata.len() <= MAX_MANUAL_PATH_CONFIG_BYTES)
        .then(|| fs::read_to_string(path).ok())
        .flatten()
}

fn read_path_list(path: &Path) -> Vec<PathBuf> {
    read_config(path)
        .map(|text| parse_path_list(&text))
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ManDbConfig {
    mappings: Vec<(PathBuf, PathBuf)>,
    mandatory: Vec<PathBuf>,
}

fn parse_man_db_config(text: &str) -> ManDbConfig {
    let mut configuration = ManDbConfig::default();
    for line in config_lines(text) {
        let Some((directive, value)) = config_directive(line) else {
            continue;
        };
        match directive {
            "MANPATH_MAP" => {
                let mut fields = value.split_whitespace();
                if let Some((binary, manual)) = fields.next().zip(fields.next()) {
                    configuration
                        .mappings
                        .push((PathBuf::from(binary), PathBuf::from(manual)));
                }
            }
            "MANDATORY_MANPATH" => {
                if let Some(manual) = value.split_whitespace().next() {
                    configuration.mandatory.push(PathBuf::from(manual));
                }
            }
            _ => {}
        }
    }
    configuration
}

fn man_db_manual_roots(
    environment: &HashMap<OsString, OsString>,
    configuration: &ManDbConfig,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = environment.get(OsStr::new("PATH")) {
        for binary in env::split_paths(path) {
            let mapped = configuration
                .mappings
                .iter()
                .filter(|(configured, _)| paths_equivalent(configured, &binary))
                .map(|(_, manual)| manual.clone())
                .collect::<Vec<_>>();
            if mapped.is_empty() {
                roots.extend(
                    unmapped_man_db_roots(&binary)
                        .into_iter()
                        .filter(|candidate| candidate.is_dir()),
                );
            } else {
                roots.extend(mapped);
            }
        }
    }
    roots.extend(configuration.mandatory.iter().cloned());
    expand_man_db_systems(roots, environment)
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    left == right
        || fs::canonicalize(left)
            .ok()
            .zip(fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}

fn unmapped_man_db_roots(binary: &Path) -> Vec<PathBuf> {
    let mut roots = vec![binary.join("man"), binary.join("share/man")];
    if let Some(prefix) = binary.parent() {
        roots.insert(0, prefix.join("man"));
        roots.insert(2, prefix.join("share/man"));
    }
    roots
}

fn expand_man_db_systems(
    roots: Vec<PathBuf>,
    environment: &HashMap<OsString, OsString>,
) -> Vec<PathBuf> {
    let Some(systems) = environment.get(OsStr::new("SYSTEM")) else {
        return deduplicate_paths(roots);
    };
    let systems_value = systems.to_string_lossy();
    let systems = systems_value
        .split([',', ':'])
        .filter(|system| !system.is_empty())
        .collect::<Vec<_>>();
    if systems.is_empty() {
        return deduplicate_paths(roots);
    }

    let mut expanded = Vec::new();
    for root in roots {
        for system in &systems {
            if *system == "man" {
                expanded.push(root.clone());
            } else {
                let candidate = root.join(system);
                if candidate.is_dir() {
                    expanded.push(candidate);
                }
            }
        }
    }
    deduplicate_paths(expanded)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct BsdManConfig {
    paths: Vec<PathBuf>,
    include_pattern: Option<PathBuf>,
}

fn parse_bsd_man_config(text: &str) -> BsdManConfig {
    let mut configuration = BsdManConfig::default();
    for line in config_lines(text) {
        let Some((directive, value)) = config_directive(line) else {
            continue;
        };
        match directive {
            "MANPATH" | "manpath" => configuration
                .paths
                .extend(expand_path_pattern(Path::new(value))),
            "MANCONFIG" => configuration.include_pattern = Some(PathBuf::from(value)),
            _ => {}
        }
    }
    configuration
}

fn parse_mandoc_manpaths(text: &str) -> Vec<PathBuf> {
    config_lines(text)
        .filter_map(config_directive)
        .filter_map(|(directive, value)| (directive == "manpath").then_some(value))
        .flat_map(|path| expand_path_pattern(Path::new(path)))
        .collect()
}

fn parse_path_list(text: &str) -> Vec<PathBuf> {
    config_lines(text)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .flat_map(|path| expand_path_pattern(Path::new(path)))
        .collect()
}

fn config_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

fn config_directive(line: &str) -> Option<(&str, &str)> {
    let (directive, value) = line.split_once(char::is_whitespace)?;
    let value = value.trim();
    (!value.is_empty()).then_some((directive, value))
}

fn expand_path_pattern(pattern: &Path) -> Vec<PathBuf> {
    if !pattern.as_os_str().to_string_lossy().contains(['*', '?']) {
        return vec![pattern.to_path_buf()];
    }
    let mut candidates = vec![PathBuf::new()];
    for component in pattern.components() {
        match component {
            Component::Prefix(prefix) => {
                for candidate in &mut candidates {
                    candidate.push(prefix.as_os_str());
                }
            }
            Component::RootDir => {
                // On Windows, the root must be appended after a drive or UNC
                // prefix. Dropping it turns `C:\path` into the drive-relative
                // `C:path`, whose result depends on process-global drive state.
                for candidate in &mut candidates {
                    candidate.push(std::path::MAIN_SEPARATOR.to_string());
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                for candidate in &mut candidates {
                    candidate.push("..");
                }
            }
            Component::Normal(component) => {
                let Some(component) = component.to_str() else {
                    return Vec::new();
                };
                if !component.contains(['*', '?']) {
                    for candidate in &mut candidates {
                        candidate.push(component);
                    }
                    continue;
                }

                let mut expanded = Vec::new();
                for candidate in candidates {
                    let Ok(entries) = fs::read_dir(&candidate) else {
                        continue;
                    };
                    let mut entries = entries.flatten().collect::<Vec<_>>();
                    entries.sort_unstable_by_key(fs::DirEntry::file_name);
                    for entry in entries {
                        let name = entry.file_name();
                        if name
                            .to_str()
                            .is_some_and(|name| wildcard_matches(component, name))
                        {
                            expanded.push(entry.path());
                            if expanded.len() >= MAX_EXPANDED_CONFIG_PATHS {
                                return expanded;
                            }
                        }
                    }
                }
                candidates = expanded;
            }
        }
    }
    candidates
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for token in pattern {
        let mut current = vec![false; value.len() + 1];
        match token {
            b'*' => {
                current[0] = previous[0];
                for index in 1..=value.len() {
                    current[index] = previous[index] || current[index - 1];
                }
            }
            b'?' => {
                current[1..].copy_from_slice(&previous[..value.len()]);
            }
            token => {
                for index in 1..=value.len() {
                    current[index] = previous[index - 1] && value[index - 1] == *token;
                }
            }
        }
        previous = current;
    }
    previous[value.len()]
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, env, ffi::OsString, fs, path::PathBuf};

    use super::{
        BsdManConfig, ManDbConfig, ManualPathPlatform, config_directive, developer_manual_roots,
        discover_manual_roots_from, expand_man_db_systems, parse_bsd_man_config,
        parse_man_db_config, parse_mandoc_manpaths, supplemental_manual_roots_for,
        wildcard_matches,
    };

    #[test]
    fn macos_man_conf_reads_paths_and_imports_port_fragments() {
        let root = temporary_root("macos-man-conf");
        let fragments = root.join("man.d");
        fs::create_dir_all(&fragments).expect("create fragment root");
        let primary = root.join("primary");
        let port = root.join("port");
        fs::create_dir_all(&primary).expect("create primary root");
        fs::create_dir_all(&port).expect("create port root");
        let fragment = fragments.join("tool.conf");
        fs::write(&fragment, format!("MANPATH {}\n", port.display())).expect("write fragment");
        let configuration = parse_bsd_man_config(&format!(
            "MANPATH {}\nMANCONFIG {}/*.conf\n",
            primary.display(),
            fragments.display()
        ));
        assert_eq!(configuration.paths, vec![primary]);
        assert_eq!(
            configuration.include_pattern,
            Some(fragments.join("*.conf"))
        );
        assert_eq!(
            super::bsd_configured_manual_roots(&root.join("man.conf")),
            Vec::<PathBuf>::new()
        );
        fs::write(
            root.join("man.conf"),
            format!(
                "MANPATH {}\nMANCONFIG {}/*.conf\n",
                root.join("primary").display(),
                fragments.display()
            ),
        )
        .expect("write man.conf");
        assert_eq!(
            super::bsd_configured_manual_roots(&root.join("man.conf")),
            vec![root.join("primary"), port]
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn mandoc_man_conf_uses_lowercase_manpath_only() {
        let paths = parse_mandoc_manpaths(
            "# comment\nmanpath /usr/share/man\nMANPATH /not-mandoc\noutput style css\n",
        );
        assert_eq!(paths, vec![PathBuf::from("/usr/share/man")]);
    }

    #[test]
    fn directive_parser_preserves_internal_path_whitespace() {
        assert_eq!(
            config_directive("manpath   C:\\Program Files\\Tool\\man   "),
            Some(("manpath", "C:\\Program Files\\Tool\\man"))
        );
        assert_eq!(config_directive("manpath   "), None);
        assert_eq!(config_directive("manpath"), None);
    }

    #[test]
    fn active_macos_developer_tree_contributes_tool_and_sdk_manuals() {
        let developer = temporary_root("macos-developer");
        let tool = developer.join("usr/share/man");
        let sdk =
            developer.join("Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/usr/share/man");
        fs::create_dir_all(&tool).expect("create tool manuals");
        fs::create_dir_all(&sdk).expect("create SDK manuals");
        assert_eq!(developer_manual_roots(&developer), vec![tool, sdk]);
        fs::remove_dir_all(developer).expect("remove fixture");
    }

    #[test]
    fn man_db_maps_path_then_appends_mandatory_roots_and_systems() {
        let root = temporary_root("man-db-mappings");
        let binary = root.join("tool/bin");
        let manual = root.join("tool/man");
        let shared = root.join("tool/share/man");
        let mandatory = root.join("usr/share/man");
        let configuration = parse_man_db_config(&format!(
            "MANPATH_MAP {} {}\nMANPATH_MAP {} {}\nMANDATORY_MANPATH {}\n",
            binary.display(),
            manual.display(),
            binary.display(),
            shared.display(),
            mandatory.display(),
        ));
        assert_eq!(
            configuration,
            ManDbConfig {
                mappings: vec![
                    (binary.clone(), manual.clone()),
                    (binary.clone(), shared.clone()),
                ],
                mandatory: vec![mandatory.clone()],
            }
        );
        let environment = HashMap::from([
            (
                OsString::from("PATH"),
                env::join_paths([binary]).expect("join PATH"),
            ),
            (OsString::from("SYSTEM"), OsString::from("man")),
        ]);
        assert_eq!(
            super::man_db_manual_roots(&environment, &configuration),
            vec![manual, shared, mandatory]
        );
    }

    #[test]
    fn empty_manpath_components_insert_one_native_default_sequence() {
        let root = temporary_root("empty-manpath");
        let first = root.join("first");
        let empty = PathBuf::new();
        let last = root.join("last");
        let environment = HashMap::from([(
            OsString::from("MANPATH"),
            env::join_paths([&first, &empty, &last]).expect("join MANPATH"),
        )]);
        let native_a = root.join("native/a");
        let native_b = root.join("native/b");
        assert_eq!(
            discover_manual_roots_from(&environment, vec![native_a.clone(), native_b.clone()],),
            vec![first, native_a, native_b, last]
        );
    }

    #[test]
    fn windows_supplemental_roots_prefer_mant_data_before_profile_compatibility() {
        let data_root = PathBuf::from(r"C:\Users\demo\AppData\Roaming");
        let profile = PathBuf::from(r"C:\Users\demo");
        let environment = HashMap::from([
            (OsString::from("APPDATA"), data_root.as_os_str().to_owned()),
            (
                OsString::from("USERPROFILE"),
                profile.as_os_str().to_owned(),
            ),
        ]);

        assert_eq!(
            supplemental_manual_roots_for(&environment, ManualPathPlatform::Windows),
            vec![
                data_root.join("ManT").join("man"),
                profile.join(".local/share/man")
            ]
        );
    }

    #[test]
    fn windows_supplemental_roots_do_not_require_a_profile_fallback() {
        let data_root = PathBuf::from(r"D:\Roaming");
        let environment =
            HashMap::from([(OsString::from("APPDATA"), data_root.as_os_str().to_owned())]);

        assert_eq!(
            supplemental_manual_roots_for(&environment, ManualPathPlatform::Windows),
            vec![data_root.join("ManT").join("man")]
        );
    }

    #[test]
    fn wildcard_expansion_matches_configuration_file_globs_deterministically() {
        assert!(wildcard_matches("*.conf", "perl.conf"));
        assert!(wildcard_matches("?.conf", "x.conf"));
        assert!(!wildcard_matches("?.conf", "xy.conf"));
    }

    #[cfg(windows)]
    #[test]
    fn wildcard_expansion_preserves_an_absolute_windows_root() {
        let root = temporary_root("windows-absolute-glob");
        fs::create_dir_all(&root).expect("create fragment root");
        let fragment = root.join("tool.conf");
        fs::write(&fragment, "MANPATH C:\\manuals\n").expect("write fragment");

        assert_eq!(super::expand_path_pattern(&root.join("*.conf")), [fragment]);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn systems_without_man_omit_the_native_root() {
        let environment = HashMap::from([(OsString::from("SYSTEM"), OsString::from("other"))]);
        let root = temporary_root("man-db-system");
        fs::create_dir_all(root.join("other")).expect("create system root");
        assert_eq!(
            expand_man_db_systems(vec![root.clone()], &environment),
            vec![root.join("other")]
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn bsd_config_keeps_only_path_related_directives() {
        assert_eq!(
            parse_bsd_man_config("MANPATH /usr/share/man\nMANLOCALE ja_JP\n"),
            BsdManConfig {
                paths: vec![PathBuf::from("/usr/share/man")],
                include_pattern: None,
            }
        );
    }

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mant-manual-paths-{label}-{}", std::process::id()))
    }
}
