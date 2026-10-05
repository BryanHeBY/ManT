//! Shared, read-only application settings and Unix-like directory resolution.

use crate::SourceConfigError;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};

/// One invocation's environment, configuration and effective application paths.
#[derive(Clone, Debug)]
pub struct Settings {
    environment: BTreeMap<OsString, OsString>,
    windows: bool,
    config_home: Option<PathBuf>,
    declaration: Declaration,
}

/// Application-owned directory category.
#[derive(Clone, Copy, Debug)]
pub enum DirectoryKind {
    /// Personal configuration files.
    Config,
    /// Persistent documents and source snapshots.
    Data,
    /// Disposable private caches.
    Cache,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Declaration {
    paths: Paths,
    man: Manuals,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Paths {
    data_home: Option<String>,
    cache_home: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Manuals {
    paths: Vec<String>,
    discover: bool,
}

impl Default for Manuals {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            discover: true,
        }
    }
}

impl Settings {
    /// Describe which active input selected a category, for read-only diagnostics.
    #[must_use]
    pub fn directory_origin(&self, kind: DirectoryKind) -> String {
        let (name, configured) = match kind {
            DirectoryKind::Config => ("CONFIG", false),
            DirectoryKind::Data => ("DATA", self.declaration.paths.data_home.is_some()),
            DirectoryKind::Cache => ("CACHE", self.declaration.paths.cache_home.is_some()),
        };
        let variable = format!("MANT_{name}_HOME");
        if environment_value(&self.environment, &variable, self.windows).is_some() {
            return variable;
        }
        if configured {
            return format!("mant.toml [paths] {}_home", name.to_ascii_lowercase());
        }
        let variable = format!("XDG_{name}_HOME");
        if environment_value(&self.environment, &variable, self.windows)
            .map(PathBuf::from)
            .is_some_and(|path| absolute(&path, self.windows))
        {
            return variable;
        }
        "home default".to_owned()
    }
    #[cfg(feature = "update")]
    pub(crate) fn import_configuration(&mut self, text: &str) -> Result<(), SourceConfigError> {
        self.declaration = toml::from_str(text).map_err(|error| {
            SourceConfigError::new(format!("invalid legacy mant.toml: {error}"))
        })?;
        Ok(())
    }
    /// Load the current environment and optional, bounded `mant.toml`.
    ///
    /// # Errors
    /// Reports invalid explicit locations and unreadable or invalid configuration.
    pub fn load() -> Result<Self, SourceConfigError> {
        Self::from_environment(&env::vars_os().collect(), cfg!(windows))
    }

    /// Resolve settings from an explicit environment without creating directories.
    /// Windows environment-name matching is ASCII case-insensitive.
    ///
    /// # Errors
    /// Reports invalid explicit locations and unreadable or invalid configuration.
    pub fn from_environment(
        environment: &BTreeMap<OsString, OsString>,
        windows: bool,
    ) -> Result<Self, SourceConfigError> {
        let config_home = category(environment, windows, "CONFIG", None, ".config")?;
        let declaration = if let Some(root) = &config_home {
            let path = root.join("mant.toml");
            match crate::bounded::read_file_utf8(&path, 1024 * 1024, "mant.toml", true) {
                Ok(text) => toml::from_str(&text).map_err(|error| {
                    SourceConfigError::new(format!("invalid '{}': {error}", path.display()))
                })?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => Declaration::default(),
                Err(error) => {
                    return Err(SourceConfigError::new(format!(
                        "could not read '{}': {error}",
                        path.display()
                    )));
                }
            }
        } else {
            Declaration::default()
        };
        Ok(Self {
            environment: environment.clone(),
            windows,
            config_home,
            declaration,
        })
    }

    /// Final configuration directory, containing independently optional files.
    ///
    /// # Errors
    /// Reports that neither home nor a configuration-directory override exists.
    pub fn config_home(&self) -> Result<PathBuf, SourceConfigError> {
        self.config_home
            .clone()
            .ok_or_else(|| missing("configuration", "MANT_CONFIG_HOME or XDG_CONFIG_HOME"))
    }

    /// Final persistent-data directory.
    ///
    /// # Errors
    /// Reports invalid configuration or a missing data-directory base.
    pub fn data_home(&self) -> Result<PathBuf, SourceConfigError> {
        self.directory(
            "DATA",
            self.declaration.paths.data_home.as_deref(),
            ".local/share",
        )
    }

    /// Final disposable-cache directory.
    ///
    /// # Errors
    /// Reports invalid configuration or a missing cache-directory base.
    pub fn cache_home(&self) -> Result<PathBuf, SourceConfigError> {
        self.directory(
            "CACHE",
            self.declaration.paths.cache_home.as_deref(),
            ".cache",
        )
    }

    fn directory(
        &self,
        kind: &str,
        configured: Option<&str>,
        default: &str,
    ) -> Result<PathBuf, SourceConfigError> {
        if environment_value(
            &self.environment,
            &format!("MANT_{kind}_HOME"),
            self.windows,
        )
        .is_some()
        {
            return category(&self.environment, self.windows, kind, None, default)?
                .ok_or_else(|| missing(kind, "a valid override"));
        }
        let configured = configured
            .map(|value| self.resolve_path(value))
            .transpose()?;
        category(
            &self.environment,
            self.windows,
            kind,
            configured.as_deref(),
            default,
        )?
        .ok_or_else(|| missing(kind, &format!("MANT_{kind}_HOME or XDG_{kind}_HOME")))
    }

    /// Explicit personal manual roots, in configured priority order.
    ///
    /// # Errors
    /// Reports invalid configured paths or unavailable home expansion.
    pub fn manual_paths(&self) -> Result<Vec<PathBuf>, SourceConfigError> {
        self.declaration
            .man
            .paths
            .iter()
            .map(|value| self.resolve_path(value))
            .collect()
    }

    /// Whether personal `man.conf`, host roots and supplemental roots are discovered.
    #[must_use]
    pub const fn discover_manuals(&self) -> bool {
        self.declaration.man.discover
    }

    /// Interpret a TOML path relative to its configuration file, with `~/` support.
    ///
    /// # Errors
    /// Reports empty/control-containing paths or unavailable home/configuration roots.
    pub fn resolve_path(&self, value: &str) -> Result<PathBuf, SourceConfigError> {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(SourceConfigError::new(
                "configuration paths must be nonempty and contain no control characters",
            ));
        }
        if let Some(rest) = value
            .strip_prefix("~/")
            .or_else(|| value.strip_prefix("~\\"))
        {
            return home(&self.environment, self.windows)
                .map(|root| root.join(rest))
                .ok_or_else(|| missing("home", "HOME or USERPROFILE"));
        }
        let path = PathBuf::from(value);
        if absolute(&path, self.windows) {
            Ok(path)
        } else {
            Ok(self.config_home()?.join(path))
        }
    }
}

fn missing(category: &str, variables: &str) -> SourceConfigError {
    SourceConfigError::new(format!(
        "could not determine the {category} directory; set {variables} or a valid home directory"
    ))
}

/// Read an environment value using the host's name-matching convention.
#[must_use]
pub fn environment_value<'a>(
    environment: &'a BTreeMap<OsString, OsString>,
    name: &str,
    windows: bool,
) -> Option<&'a OsStr> {
    environment
        .get(OsStr::new(name))
        .or_else(|| {
            windows
                .then(|| {
                    environment
                        .iter()
                        .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
                        .map(|(_, value)| value)
                })
                .flatten()
        })
        .filter(|value| !value.is_empty())
        .map(OsString::as_os_str)
}

/// Resolve the home used by every application-owned directory.
#[must_use]
pub fn home(environment: &BTreeMap<OsString, OsString>, windows: bool) -> Option<PathBuf> {
    environment_value(environment, "HOME", windows)
        .map(PathBuf::from)
        .filter(|path| absolute(path, windows))
        .or_else(|| {
            windows
                .then(|| {
                    environment_value(environment, "USERPROFILE", true)
                        .map(PathBuf::from)
                        .filter(|path| absolute(path, true))
                })
                .flatten()
        })
}

fn category(
    environment: &BTreeMap<OsString, OsString>,
    windows: bool,
    kind: &str,
    configured: Option<&Path>,
    default: &str,
) -> Result<Option<PathBuf>, SourceConfigError> {
    let variable = format!("MANT_{kind}_HOME");
    if let Some(value) = environment_value(environment, &variable, windows) {
        let path = PathBuf::from(value);
        if !absolute(&path, windows) {
            return Err(SourceConfigError::new(format!(
                "{variable} must be an absolute path"
            )));
        }
        return Ok(Some(path));
    }
    if let Some(path) = configured {
        return Ok(Some(path.to_owned()));
    }
    if let Some(path) = environment_value(environment, &format!("XDG_{kind}_HOME"), windows)
        .map(PathBuf::from)
        .filter(|path| absolute(path, windows))
    {
        return Ok(Some(path.join("mant")));
    }
    Ok(home(environment, windows).map(|root| root.join(default).join("mant")))
}

fn absolute(path: &Path, windows: bool) -> bool {
    let value = path.as_os_str().to_string_lossy();
    if windows {
        path.is_absolute()
            || value.as_bytes().get(..3).is_some_and(|prefix| {
                prefix[0].is_ascii_alphabetic()
                    && prefix[1] == b':'
                    && matches!(prefix[2], b'/' | b'\\')
            })
            || value.starts_with("\\\\")
    } else {
        path.is_absolute() || value.starts_with('/')
    }
}

#[cfg(test)]
mod tests;
