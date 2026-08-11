//! Loads and validates Git-backed Markdown document source configuration.

use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fmt, fs, io,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;

const APPLICATION_DIR_LINUX: &str = "mant";
const APPLICATION_DIR_OTHER: &str = "ManT";
const CONFIG_FILE: &str = "sources.toml";
const DOCUMENTS_DIR: &str = "documents";
const SOURCES_DIR: &str = "sources";
pub(crate) const SOURCE_METADATA_FILE: &str = ".mant-source.toml";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Platform-native paths used by document discovery and repository updates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub documents: PathBuf,
    pub sources: PathBuf,
}

/// One repository declared by a top-level table in `sources.toml`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositorySource {
    pub repo: String,
    pub branch: String,
    #[serde(default = "default_source_path")]
    pub path: String,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub priority: i32,
}

/// Validated local source configuration, keyed by source name.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceConfig {
    sources: BTreeMap<String, RepositorySource>,
}

impl SourceConfig {
    #[must_use]
    pub fn sources(&self) -> &BTreeMap<String, RepositorySource> {
        &self.sources
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RepositorySource> {
        self.sources.get(name)
    }

    /// Source names in fallback order: higher priority first, then name.
    #[must_use]
    pub fn precedence(&self) -> Vec<&str> {
        let mut sources = self.sources.iter().collect::<Vec<_>>();
        sources.sort_unstable_by(|(left_name, left), (right_name, right)| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left_name.cmp(right_name))
        });
        sources.into_iter().map(|(name, _)| name.as_str()).collect()
    }
}

/// Invalid or unavailable document-source configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceConfigError {
    detail: String,
}

impl SourceConfigError {
    pub(crate) fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for SourceConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for SourceConfigError {}

/// Resolve the current user's singular `ManT` data root.
///
/// # Errors
///
/// Returns an error when no absolute platform user-data root can be derived.
pub fn document_paths() -> Result<DocumentPaths, SourceConfigError> {
    let environment = env::vars_os().collect::<BTreeMap<_, _>>();
    document_paths_with(&environment)
}

/// Load and validate `sources.toml`. A missing file means no repositories.
///
/// # Errors
///
/// Returns an error when the data root, TOML syntax, or any source field is
/// invalid.
pub fn load_source_config() -> Result<(DocumentPaths, SourceConfig), SourceConfigError> {
    let paths = document_paths()?;
    let config = load_source_config_from(&paths.config)?;
    Ok((paths, config))
}

fn default_source_path() -> String {
    ".".to_owned()
}

fn document_paths_with(
    environment: &BTreeMap<OsString, OsString>,
) -> Result<DocumentPaths, SourceConfigError> {
    let root = if cfg!(windows) {
        absolute_environment_path(environment, "APPDATA")
            .map(|path| path.join(APPLICATION_DIR_OTHER))
    } else if cfg!(target_os = "macos") {
        absolute_environment_path(environment, "HOME").map(|path| {
            path.join("Library/Application Support")
                .join(APPLICATION_DIR_OTHER)
        })
    } else {
        absolute_environment_path(environment, "XDG_DATA_HOME")
            .or_else(|| {
                absolute_environment_path(environment, "HOME").map(|path| path.join(".local/share"))
            })
            .map(|path| path.join(APPLICATION_DIR_LINUX))
    }
    .ok_or_else(|| {
        SourceConfigError::new(
            "could not determine the user data directory; set HOME, XDG_DATA_HOME, or APPDATA",
        )
    })?;

    Ok(DocumentPaths {
        config: root.join(CONFIG_FILE),
        documents: root.join(DOCUMENTS_DIR),
        sources: root.join(SOURCES_DIR),
        root,
    })
}

fn absolute_environment_path(
    environment: &BTreeMap<OsString, OsString>,
    name: &str,
) -> Option<PathBuf> {
    let value = environment.get(OsStr::new(name));
    #[cfg(windows)]
    let value = value.or_else(|| {
        environment
            .iter()
            .find(|(candidate, _)| candidate.to_string_lossy().eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    });
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

fn load_source_config_from(path: &Path) -> Result<SourceConfig, SourceConfigError> {
    let text = match read_bounded_utf8(path, MAX_CONFIG_BYTES) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(SourceConfig::default()),
        Err(error) => {
            return Err(SourceConfigError::new(format!(
                "could not read '{}': {error}",
                path.display()
            )));
        }
    };
    let sources = toml::from_str::<BTreeMap<String, RepositorySource>>(&text).map_err(|error| {
        SourceConfigError::new(format!("invalid '{}': {error}", path.display()))
    })?;
    for (name, source) in &sources {
        validate_source(name, source).map_err(|detail| {
            SourceConfigError::new(format!(
                "invalid source '{name}' in '{}': {detail}",
                path.display()
            ))
        })?;
    }
    Ok(SourceConfig { sources })
}

fn validate_source(name: &str, source: &RepositorySource) -> Result<(), String> {
    if !is_source_name(name) {
        return Err("name must use lowercase ASCII letters, digits, '-' or '_', and start with a letter or digit".to_owned());
    }
    if source.repo.trim().is_empty()
        || source.repo.trim() != source.repo
        || source.repo.starts_with('-')
    {
        return Err(
            "repo must be a trimmed, non-empty Git URL or path and must not start with '-'"
                .to_owned(),
        );
    }
    if source.branch.trim().is_empty()
        || source.branch.trim() != source.branch
        || source.branch.starts_with('-')
    {
        return Err("branch must be trimmed, non-empty, and must not start with '-'".to_owned());
    }
    validate_relative_selector(&source.path, "path", true)?;
    for include in &source.include {
        validate_relative_selector(include, "include", false)?;
    }
    for exclude in &source.exclude {
        validate_relative_selector(exclude, "exclude", false)?;
    }
    Ok(())
}

pub(crate) fn is_source_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

fn validate_relative_selector(value: &str, field: &str, allow_dot: bool) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || value != trimmed || (!allow_dot && trimmed == ".") {
        return Err(format!("{field} entries must be trimmed and non-empty"));
    }
    let value = trimmed;
    if value.contains('\\') {
        return Err(format!("{field} must use '/' as its path separator"));
    }
    if value.contains(['*', '?', '[', ']', '{', '}']) {
        return Err(format!("{field} does not accept glob patterns"));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            !(matches!(component, Component::Normal(_))
                || allow_dot && value == "." && matches!(component, Component::CurDir))
        })
    {
        return Err(format!("{field} must be a relative path without '..'"));
    }
    if value != "." {
        let normalized = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(value) => value.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        if normalized != value {
            return Err(format!(
                "{field} must not contain repeated, leading, or trailing separators"
            ));
        }
    }
    Ok(())
}

fn read_bounded_utf8(path: &Path, limit: u64) -> io::Result<String> {
    crate::bounded::read_utf8(fs::File::open(path)?, limit, "file")
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[cfg(target_os = "linux")]
    use std::{collections::BTreeMap, ffi::OsString, path::Path};

    use super::{RepositorySource, load_source_config_from};

    #[cfg(target_os = "linux")]
    use super::document_paths_with;

    fn temp(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mant-sources-{label}-{}", std::process::id()))
    }

    #[test]
    fn top_level_tables_are_sources_and_precedence_is_explicit() {
        let root = temp("config");
        fs::create_dir_all(&root).expect("create fixture");
        let config = root.join("sources.toml");
        fs::write(
            &config,
            r#"
[zeta]
repo = "https://example.invalid/zeta.git"
branch = "main"
priority = 10

[alpha]
repo = "https://example.invalid/alpha.git"
branch = "stable"
priority = 10
include = ["docs", "README.md"]
exclude = ["docs/drafts"]

[later]
repo = "https://example.invalid/later.git"
branch = "main"
priority = -1
"#,
        )
        .expect("write config");
        let loaded = load_source_config_from(&config).expect("load config");
        assert_eq!(loaded.precedence(), vec!["alpha", "zeta", "later"]);
        assert_eq!(loaded.get("alpha").expect("alpha").path, ".");
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn glob_and_parent_selectors_are_rejected() {
        let root = temp("invalid");
        fs::create_dir_all(&root).expect("create fixture");
        let config = root.join("sources.toml");
        fs::write(
            &config,
            "[bad]\nrepo = 'x'\nbranch = 'main'\ninclude = ['**/*.md']\n",
        )
        .expect("write config");
        assert!(
            load_source_config_from(&config)
                .expect_err("reject glob")
                .to_string()
                .contains("does not accept glob")
        );
        fs::write(
            &config,
            "[bad]\nrepo = 'x'\nbranch = 'main'\npath = '../docs'\n",
        )
        .expect("write config");
        assert!(
            load_source_config_from(&config)
                .expect_err("reject parent")
                .to_string()
                .contains("without '..'")
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn repository_source_priority_is_signed() {
        let source = RepositorySource {
            repo: "repo".to_owned(),
            branch: "main".to_owned(),
            path: ".".to_owned(),
            include: Vec::new(),
            exclude: Vec::new(),
            priority: -1,
        };
        assert_eq!(source.priority, -1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_paths_use_one_user_data_root() {
        let mut environment = BTreeMap::new();
        environment.insert(
            OsString::from("XDG_DATA_HOME"),
            OsString::from("/data/user"),
        );
        let paths = document_paths_with(&environment).expect("paths");
        assert_eq!(paths.root, Path::new("/data/user/mant"));
        assert_eq!(paths.config, Path::new("/data/user/mant/sources.toml"));
        assert_eq!(paths.documents, Path::new("/data/user/mant/documents"));
        assert_eq!(paths.sources, Path::new("/data/user/mant/sources"));
    }
}
