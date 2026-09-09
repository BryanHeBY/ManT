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
mod tests;
