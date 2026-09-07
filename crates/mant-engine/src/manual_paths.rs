//! Native manual-root discovery without invoking a host `man` program.
//!
//! Unix manual lookup is not governed by one portable directory list.  The
//! two common Linux implementations, the BSD family, and macOS each publish
//! configuration that affects the effective path.  This module reads the
//! small, declarative subset that determines *source roots*; it deliberately
//! does not inherit pager, formatter, cache, or locale behaviour from the
//! host implementation.

mod bsd;
mod config_file;
mod macos;
mod man_db;
use bsd::mandoc_configured_manual_roots;
#[cfg(test)]
use bsd::{BsdManConfig, macos_configuration_roots, parse_bsd_man_config, parse_mandoc_manpaths};
#[cfg(test)]
use macos::developer_manual_roots;
use macos::macos_configured_manual_roots;
use man_db::linux_configured_manual_roots;
#[cfg(unix)]
use man_db::unmapped_man_db_roots;
#[cfg(test)]
use man_db::{ManDbConfig, expand_man_db_systems, man_db_manual_roots, parse_man_db_config};
mod expansion;
mod windows_config;
use config_file::read_text as read_config_text;
#[cfg(test)]
use expansion::wildcard_matches;
use expansion::{ExpansionOutcome, ScanBudget, expand_path_pattern_bounded};

use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
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
const MAX_EXPANDED_CONFIG_CANDIDATES: usize = 4096;

/// One rejected entry in the host manual-path configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualPathDiagnostic {
    /// Configuration file containing the invalid directive.
    pub config_path: PathBuf,
    /// One-based source line, or `None` for a whole-file failure.
    pub line: Option<usize>,
    /// Bounded explanation suitable for a local doctor report.
    pub message: String,
}

/// Effective manual roots plus non-fatal host-configuration findings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManualRootDiscovery {
    /// Manual hierarchy roots in effective lookup precedence.
    pub roots: Vec<PathBuf>,
    /// Invalid configuration entries omitted from `roots`.
    pub diagnostics: Vec<ManualPathDiagnostic>,
}

/// Discover effective manual hierarchy roots for the current host.
///
/// `MANT_MANPATH` is a complete `ManT` override.  Otherwise `MANPATH` follows
/// conventional empty-component insertion, with host-derived defaults at each
/// empty component.  When neither variable is set, platform configuration is
/// read without spawning `man`, `manpath`, or any other external program.
#[must_use]
pub fn discover_manual_roots() -> Vec<PathBuf> {
    inspect_manual_roots().roots
}

/// Inspect effective native-manual roots without mutating host state.
///
/// Explicit `MANT_MANPATH` and complete `MANPATH` overrides do not read or
/// report an inactive host configuration. Diagnostics describe rejected or
/// truncated BSD/mandoc and ManT-owned Windows configuration; ordinary queries use
/// only [`discover_manual_roots`].
#[must_use]
pub fn inspect_manual_roots() -> ManualRootDiscovery {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    if environment_value(&environment, "MANT_MANPATH").is_some() {
        return ManualRootDiscovery {
            roots: discover_manual_roots_from(&environment, Vec::new()),
            diagnostics: Vec::new(),
        };
    }
    if environment_value(&environment, "MANPATH")
        .is_some_and(|value| env::split_paths(value).all(|path| !path.as_os_str().is_empty()))
    {
        return ManualRootDiscovery {
            roots: discover_manual_roots_from(&environment, Vec::new()),
            diagnostics: Vec::new(),
        };
    }

    let platform = host_platform();
    let mant_config = (platform == ManualPathPlatform::Windows)
        .then(|| {
            mant_sources::document_paths()
                .ok()
                .map(|paths| paths.root.join("man.conf"))
        })
        .flatten();
    let context = DiscoveryContext {
        environment: &environment,
        platform,
        mant_config: mant_config.as_deref(),
    };
    let defaults = host_default_manual_roots(&context);
    ManualRootDiscovery {
        roots: discover_manual_roots_from(&environment, defaults.roots),
        diagnostics: defaults.diagnostics,
    }
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
    discover_manual_roots_from_for(environment, defaults, host_platform())
}

fn discover_manual_roots_from_for(
    environment: &HashMap<OsString, OsString>,
    defaults: Vec<PathBuf>,
    platform: ManualPathPlatform,
) -> Vec<PathBuf> {
    if let Some(explicit) = environment_value_for(environment, "MANT_MANPATH", platform) {
        return deduplicate_manual_paths(
            env::split_paths(explicit).filter(|path| !path.as_os_str().is_empty()),
            platform,
        );
    }

    if let Some(manpath) = environment_value_for(environment, "MANPATH", platform) {
        let mut roots = Vec::new();
        for path in env::split_paths(manpath) {
            if path.as_os_str().is_empty() {
                roots.extend(defaults.iter().cloned());
            } else {
                roots.push(path);
            }
        }
        return deduplicate_manual_paths(roots, platform);
    }
    defaults
}

/// All host input is sampled at the public boundary, not during config parsing.
struct DiscoveryContext<'a> {
    environment: &'a HashMap<OsString, OsString>,
    platform: ManualPathPlatform,
    mant_config: Option<&'a Path>,
}

fn host_default_manual_roots(context: &DiscoveryContext<'_>) -> ManualRootDiscovery {
    let environment = context.environment;
    let mut discovery = match context.platform {
        ManualPathPlatform::Linux => linux_configured_manual_roots(environment),
        ManualPathPlatform::Macos => macos_configured_manual_roots(environment),
        ManualPathPlatform::Windows => mant_configured_manual_roots(context),
        ManualPathPlatform::OtherUnix => mandoc_configured_manual_roots(Path::new("/etc/man.conf")),
    };
    if discovery.roots.is_empty() {
        discovery.roots = if context.platform == ManualPathPlatform::Windows {
            deduplicate_manual_paths(
                supplemental_manual_roots_for(environment, context.platform),
                context.platform,
            )
        } else {
            fallback_manual_roots(environment)
        };
    } else {
        discovery
            .roots
            .extend(supplemental_manual_roots_for(environment, context.platform));
        discovery.roots = deduplicate_manual_paths(discovery.roots, context.platform);
    }
    discovery
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManualPathPlatform {
    Linux,
    Macos,
    Windows,
    OtherUnix,
}

fn environment_value<'a>(
    environment: &'a HashMap<OsString, OsString>,
    name: &str,
) -> Option<&'a OsString> {
    environment_value_for(environment, name, host_platform())
}

fn environment_value_for<'a>(
    environment: &'a HashMap<OsString, OsString>,
    name: &str,
    platform: ManualPathPlatform,
) -> Option<&'a OsString> {
    environment.get(OsStr::new(name)).or_else(|| {
        (platform == ManualPathPlatform::Windows).then(|| {
            environment.iter().find_map(|(candidate, value)| {
                candidate
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
                    .then_some(value)
            })
        })?
    })
}

fn deduplicate_manual_paths(
    paths: impl IntoIterator<Item = PathBuf>,
    platform: ManualPathPlatform,
) -> Vec<PathBuf> {
    let paths = paths.into_iter().collect::<Vec<_>>();
    if platform == ManualPathPlatform::Windows {
        windows_config::deduplicate_windows_paths(paths)
    } else {
        deduplicate_paths(paths)
    }
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
    deduplicate_manual_paths(supplemental_manual_roots(environment), host_platform())
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
        if let Some(data_root) =
            environment_value_for(environment, "APPDATA", platform).map(PathBuf::from)
        {
            roots.push(data_root.join("ManT").join("man"));
        }
        if let Some(profile) =
            environment_value_for(environment, "USERPROFILE", platform).map(PathBuf::from)
        {
            roots.push(profile.join(".local/share/man"));
        }
        return roots;
    }

    if let Some(home) = environment_value_for(environment, "HOME", platform).map(PathBuf::from) {
        roots.push(home.join(".local/share/man"));
        roots.push(home.join(".local/man"));
        roots.push(home.join("man"));
    }
    if let Some(data_home) =
        environment_value_for(environment, "XDG_DATA_HOME", platform).map(PathBuf::from)
    {
        roots.push(data_home.join("man"));
    }
    if let Some(data_dirs) = environment_value_for(environment, "XDG_DATA_DIRS", platform) {
        roots.extend(env::split_paths(data_dirs).map(|root| root.join("man")));
    }
    roots
}

#[cfg(unix)]
fn path_derived_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = environment_value(environment, "PATH") {
        for binary_dir in env::split_paths(path) {
            roots.extend(unmapped_man_db_roots(&binary_dir));
        }
    }
    roots
}

fn mant_configured_manual_roots(context: &DiscoveryContext<'_>) -> ManualRootDiscovery {
    context
        .mant_config
        .map(|path| {
            let executable_paths =
                environment_value_for(context.environment, "PATH", context.platform)
                    .map(|value| env::split_paths(value).collect::<Vec<_>>())
                    .unwrap_or_default();
            windows_config::load(path, context.environment, &executable_paths)
        })
        .unwrap_or_default()
}

fn read_config(path: &Path) -> Option<String> {
    read_config_text(path, MAX_MANUAL_PATH_CONFIG_BYTES).ok()
}

fn read_path_list(path: &Path) -> Vec<PathBuf> {
    read_config(path)
        .map(|text| parse_path_list(&text))
        .unwrap_or_default()
}

fn parse_path_list(text: &str) -> Vec<PathBuf> {
    config_lines(text)
        .take(MAX_EXPANDED_CONFIG_CANDIDATES)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
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

#[cfg(all(test, windows))]
fn expand_path_pattern(pattern: &Path) -> Vec<PathBuf> {
    let mut budget = ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES);
    expand_path_pattern_bounded(pattern, &mut budget)
        .paths
        .into_iter()
        .take(MAX_EXPANDED_CONFIG_PATHS)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, env, ffi::OsString, fs, path::PathBuf};

    use super::{
        BsdManConfig, ManDbConfig, ManualPathPlatform, config_directive, deduplicate_manual_paths,
        developer_manual_roots, discover_manual_roots_from, discover_manual_roots_from_for,
        environment_value_for, expand_man_db_systems, parse_bsd_man_config, parse_man_db_config,
        parse_mandoc_manpaths, supplemental_manual_roots_for, wildcard_matches,
    };

    #[test]
    fn injected_windows_discovery_uses_only_its_config_and_environment() {
        let root = temporary_root("injected-windows-config");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("man.conf");
        fs::write(&config, "MANPATH C:\\isolated\\manuals\n").unwrap();
        let environment = HashMap::from([(
            OsString::from("AppData"),
            OsString::from(r"C:\isolated\roaming"),
        )]);
        let context = super::DiscoveryContext {
            environment: &environment,
            platform: ManualPathPlatform::Windows,
            mant_config: Some(&config),
        };
        let result = super::host_default_manual_roots(&context);
        assert!(result.diagnostics.is_empty());
        assert_eq!(
            result.roots,
            [
                PathBuf::from(r"C:\isolated\manuals"),
                PathBuf::from(r"C:\isolated\roaming").join("ManT/man")
            ]
        );
        let absent = super::DiscoveryContext {
            mant_config: None,
            ..context
        };
        assert_eq!(
            super::host_default_manual_roots(&absent).roots,
            [PathBuf::from(r"C:\isolated\roaming").join("ManT/man")]
        );
        fs::remove_dir_all(root).unwrap();
    }

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
            super::macos_configuration_roots(&root.join("man.conf")).roots,
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
            super::macos_configuration_roots(&root.join("man.conf")).roots,
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
            (OsString::from("AppData"), data_root.as_os_str().to_owned()),
            (
                OsString::from("UserProfile"),
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
    fn windows_environment_names_are_ascii_case_insensitive_only_on_windows() {
        let path = env::join_paths([PathBuf::from("/tools")]).expect("join PATH");
        let environment = HashMap::from([
            (OsString::from("Path"), path.clone()),
            (OsString::from("ManPath"), OsString::from("/manuals")),
            (OsString::from("Mant_ManPath"), OsString::from("/override")),
        ]);

        assert_eq!(
            environment_value_for(&environment, "PATH", ManualPathPlatform::Windows),
            Some(&path)
        );
        assert_eq!(
            environment_value_for(&environment, "PATH", ManualPathPlatform::Linux),
            None
        );
        assert_eq!(
            discover_manual_roots_from_for(
                &environment,
                vec![PathBuf::from("/default")],
                ManualPathPlatform::Windows,
            ),
            vec![PathBuf::from("/override")]
        );

        let environment = HashMap::from([(
            OsString::from("ManPath"),
            env::join_paths([PathBuf::from("/first"), PathBuf::from("/second")])
                .expect("join MANPATH"),
        )]);
        assert_eq!(
            discover_manual_roots_from_for(
                &environment,
                vec![PathBuf::from("/default")],
                ManualPathPlatform::Windows,
            ),
            vec![PathBuf::from("/first"), PathBuf::from("/second")]
        );
    }

    #[test]
    fn windows_final_roots_deduplicate_case_and_separator_variants() {
        assert_eq!(
            deduplicate_manual_paths(
                [
                    PathBuf::from(r"C:\Users\demo\ManT\man"),
                    PathBuf::from("c:/users/DEMO/mant/man/"),
                ],
                ManualPathPlatform::Windows,
            ),
            vec![PathBuf::from(r"C:\Users\demo\ManT\man")]
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
                truncated: false,
            }
        );
    }

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mant-manual-paths-{label}-{}", std::process::id()))
    }
}
