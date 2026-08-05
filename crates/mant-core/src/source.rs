//! Discovers and resolves local manual sources without invoking a man program.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fmt, fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

const DEFAULT_MANUAL_ROOTS: [&str; 4] = [
    "/usr/local/share/man",
    "/usr/local/man",
    "/usr/share/man",
    "/usr/man",
];
const SUPPORTED_COMPRESSION_SUFFIXES: [&str; 2] = [".gz", ".zst"];

static SYSTEM_INDEX: OnceLock<ManualIndex> = OnceLock::new();

/// One validated manual lookup independent from CLI token syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualRequest {
    pub name: String,
    pub section: Option<String>,
}

impl ManualRequest {
    #[must_use]
    pub fn new(name: impl Into<String>, section: Option<String>) -> Self {
        Self {
            name: name.into(),
            section,
        }
    }
}

/// One effective local manual page after path and locale precedence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualPage {
    pub name: String,
    pub section: String,
    pub path: PathBuf,
}

/// Immutable index shared by discovery and exact manual lookup.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManualIndex {
    roots: Vec<PathBuf>,
    pages: Vec<ManualPage>,
}

impl ManualIndex {
    /// Scan explicit roots in precedence order.
    #[must_use]
    pub fn from_roots(roots: Vec<PathBuf>) -> Self {
        let locale = current_locale();
        Self::from_roots_with_locale(roots, locale.as_deref())
    }

    fn from_roots_with_locale(roots: Vec<PathBuf>, locale: Option<&str>) -> Self {
        let roots = deduplicate_paths(roots);
        let mut effective = BTreeMap::<(String, String), ManualPage>::new();
        for root in &roots {
            for page in scan_manual_root(root, locale) {
                effective
                    .entry((page.name.clone(), page.section.clone()))
                    .or_insert(page);
            }
        }
        Self {
            roots,
            pages: effective.into_values().collect(),
        }
    }

    /// Roots searched by this index, in precedence order.
    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Effective pages sorted by name and section.
    #[must_use]
    pub fn pages(&self) -> &[ManualPage] {
        &self.pages
    }

    /// Resolve one page using an optional exact section selector.
    #[must_use]
    pub fn find(&self, name: &str, section: Option<&str>) -> Option<&ManualPage> {
        let name = name.trim();
        let section = section.map(str::trim);
        self.pages
            .iter()
            .find(|page| page.name == name && section.is_none_or(|section| page.section == section))
    }
}

/// Minimal subprocess result retained for the opt-in groff compatibility path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Injectable boundary around process execution.
pub trait CommandRunner {
    /// Run one executable with already-separated arguments.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the executable cannot be started or waited.
    fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput>;
}

/// Production subprocess runner used only by explicit compatibility features.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput> {
        let output = Command::new(program).args(arguments).output()?;
        Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

/// Expected source-discovery failures suitable for a user-facing CLI error.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocateError {
    EmptyName,
    InvalidSection,
    NotFound { name: String },
}

impl fmt::Display for LocateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("manual page name must not be empty"),
            Self::InvalidSection => formatter.write_str("manual section must not be empty"),
            Self::NotFound { name } => {
                write!(formatter, "no local manual source was found for '{name}'")
            }
        }
    }
}

impl std::error::Error for LocateError {}

/// Build or retrieve the process-wide native manual index.
#[must_use]
pub fn system_manual_index() -> &'static ManualIndex {
    SYSTEM_INDEX.get_or_init(|| ManualIndex::from_roots(discover_manual_roots()))
}

/// Discover manual roots from explicit variables and platform conventions.
#[must_use]
pub fn discover_manual_roots() -> Vec<PathBuf> {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    discover_manual_roots_with(&environment)
}

/// Locate a manual through the process-wide native index.
///
/// # Errors
///
/// Returns [`LocateError`] for invalid requests and missing local sources.
pub fn locate_manual_source(request: &ManualRequest) -> Result<PathBuf, LocateError> {
    locate_manual_source_in(request, system_manual_index())
}

/// Locate a manual in an explicit immutable index.
///
/// # Errors
///
/// Returns [`LocateError`] for invalid requests and missing local sources.
pub fn locate_manual_source_in(
    request: &ManualRequest,
    index: &ManualIndex,
) -> Result<PathBuf, LocateError> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err(LocateError::EmptyName);
    }
    let section = request.section.as_deref().map(str::trim);
    if section.is_some_and(str::is_empty) {
        return Err(LocateError::InvalidSection);
    }
    index
        .find(name, section)
        .map(|page| page.path.clone())
        .ok_or_else(|| LocateError::NotFound {
            name: name.to_owned(),
        })
}

pub(crate) fn push_section_filter(arguments: &mut Vec<OsString>, section: &str) {
    arguments.push(OsString::from("-S"));
    arguments.push(OsString::from(section));
}

fn discover_manual_roots_with(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    if let Some(explicit) = environment.get(OsStr::new("MANT_MANPATH")) {
        return deduplicate_paths(
            env::split_paths(explicit).filter(|path| !path.as_os_str().is_empty()),
        );
    }

    let defaults = conventional_manual_roots(environment);
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

fn conventional_manual_roots(environment: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
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
    if let Some(path) = environment.get(OsStr::new("PATH")) {
        for binary_dir in env::split_paths(path) {
            if let Some(prefix) = binary_dir.parent() {
                roots.push(prefix.join("share/man"));
                roots.push(prefix.join("man"));
            }
        }
    }
    roots.extend(DEFAULT_MANUAL_ROOTS.map(PathBuf::from));
    deduplicate_paths(roots)
}

fn deduplicate_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let current_directory = env::current_dir().ok();
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .filter_map(|path| {
            if path.is_absolute() {
                Some(path)
            } else {
                current_directory.as_ref().map(|current| current.join(path))
            }
        })
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn current_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .into_iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.is_empty()))
        .and_then(|value| normalize_locale(&value))
}

fn normalize_locale(locale: &str) -> Option<String> {
    let locale = locale.split(['.', '@', ':']).next()?.trim();
    (!locale.is_empty() && locale != "C" && locale != "POSIX").then(|| locale.to_owned())
}

fn scan_manual_root(root: &Path, locale: Option<&str>) -> Vec<ManualPage> {
    let mut candidates = BTreeMap::<(String, String), (u8, PathBuf)>::new();
    scan_directory(root, root, locale, &mut candidates);
    candidates
        .into_iter()
        .map(|((name, section), (_, path))| ManualPage {
            name,
            section,
            path,
        })
        .collect()
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    locale: Option<&str>,
    candidates: &mut BTreeMap<(String, String), (u8, PathBuf)>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            scan_directory(root, &path, locale, candidates);
            continue;
        }
        if !file_type.is_file() && !file_type.is_symlink() {
            continue;
        }
        let Some((name, section)) = manual_identity(root, &path) else {
            continue;
        };
        let priority = locale_priority(root, &path, locale);
        let key = (name, section);
        match candidates.get(&key) {
            Some((current_priority, current_path))
                if (*current_priority, current_path) <= (priority, &path) => {}
            _ => {
                candidates.insert(key, (priority, path));
            }
        }
    }
}

fn manual_identity(root: &Path, path: &Path) -> Option<(String, String)> {
    let relative = path.strip_prefix(root).ok()?;
    let section_directory = relative.parent()?.components().find_map(|component| {
        let component = component.as_os_str().to_str()?;
        component
            .strip_prefix("man")
            .filter(|section| !section.is_empty())
            .map(ToOwned::to_owned)
    })?;
    let filename = path.file_name()?.to_str()?;
    let stem = SUPPORTED_COMPRESSION_SUFFIXES
        .iter()
        .find_map(|suffix| filename.strip_suffix(suffix))
        .unwrap_or(filename);
    let (name, file_section) = stem.rsplit_once('.')?;
    let section_matches_directory = file_section == section_directory
        || file_section
            .strip_prefix(&section_directory)
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(char::is_alphabetic));
    if name.is_empty() || file_section.is_empty() || !section_matches_directory {
        return None;
    }
    Some((name.to_owned(), file_section.to_owned()))
}

fn locale_priority(root: &Path, path: &Path, locale: Option<&str>) -> u8 {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let localized = relative.components().next().is_some_and(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|component| !component.starts_with("man"))
    });
    let Some(locale) = locale else {
        return u8::from(localized);
    };
    if !localized {
        return 2;
    }
    let language = locale.split('_').next().unwrap_or(locale);
    let component = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .unwrap_or_default();
    if component == locale {
        0
    } else if component == language {
        1
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ffi::OsString, fs, path::PathBuf};

    use super::{
        LocateError, ManualIndex, ManualRequest, deduplicate_paths, discover_manual_roots_with,
        locate_manual_source_in, normalize_locale,
    };

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-manual-index-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn indexes_supported_sources_and_resolves_sections_without_man() {
        let root = temporary_root("lookup");
        fs::create_dir_all(root.join("man1")).expect("manual section");
        fs::create_dir_all(root.join("man3")).expect("manual section");
        fs::write(root.join("man1/printf.1.gz"), b"gzip placeholder").expect("manual");
        fs::write(root.join("man3/printf.3.zst"), b"zstd placeholder").expect("manual");
        fs::write(root.join("man1/ignored.1.xz"), b"unsupported").expect("manual");

        let index = ManualIndex::from_roots(vec![root.clone()]);
        assert_eq!(index.pages().len(), 2);
        assert_eq!(
            locate_manual_source_in(&ManualRequest::new("printf", None), &index)
                .expect("default section"),
            root.join("man1/printf.1.gz")
        );
        assert_eq!(
            locate_manual_source_in(&ManualRequest::new("printf", Some("3".to_owned())), &index,)
                .expect("selected section"),
            root.join("man3/printf.3.zst")
        );
        assert!(matches!(
            locate_manual_source_in(&ManualRequest::new("ignored", None), &index),
            Err(LocateError::NotFound { .. })
        ));

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn root_and_locale_precedence_are_deterministic() {
        let root = temporary_root("precedence");
        let first = root.join("first");
        let second = root.join("second");
        for path in [
            first.join("man1/tool.1"),
            first.join("zh/man1/tool.1"),
            first.join("zh_CN/man1/tool.1"),
            second.join("man1/tool.1"),
        ] {
            fs::create_dir_all(path.parent().expect("manual parent")).expect("manual section");
            fs::write(path, b"manual").expect("manual");
        }

        let index = ManualIndex::from_roots_with_locale(vec![first.clone(), second], Some("zh_CN"));
        assert_eq!(index.pages()[0].path, first.join("zh_CN/man1/tool.1"));

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn locale_names_drop_encodings_modifiers_and_language_fallbacks() {
        assert_eq!(
            normalize_locale("zh_CN.UTF-8@variant"),
            Some("zh_CN".to_owned())
        );
        assert_eq!(normalize_locale("de_DE:en_US"), Some("de_DE".to_owned()));
        assert_eq!(normalize_locale("C"), None);
    }

    #[test]
    fn explicit_manpath_overrides_conventions_and_empty_components_restore_them() {
        let explicit = PathBuf::from("/opt/manuals");
        let mut environment = HashMap::from([
            (OsString::from("HOME"), OsString::from("/home/demo")),
            (OsString::from("PATH"), OsString::from("/opt/bin:/usr/bin")),
            (
                OsString::from("MANT_MANPATH"),
                explicit.as_os_str().to_owned(),
            ),
        ]);
        assert_eq!(discover_manual_roots_with(&environment), vec![explicit]);

        environment.remove(&OsString::from("MANT_MANPATH"));
        environment.insert(OsString::from("MANPATH"), OsString::from(":"));
        let roots = discover_manual_roots_with(&environment);
        assert!(roots.contains(&PathBuf::from("/home/demo/.local/share/man")));
        assert!(roots.contains(&PathBuf::from("/opt/share/man")));
        assert!(roots.contains(&PathBuf::from("/usr/share/man")));
    }

    #[test]
    fn relative_manual_roots_are_resolved_for_stable_catalog_paths() {
        let roots = deduplicate_paths([PathBuf::from("project-man")]);
        assert_eq!(roots.len(), 1);
        assert!(roots[0].is_absolute());
        assert!(roots[0].ends_with("project-man"));
    }

    #[test]
    fn invalid_requests_fail_before_lookup() {
        let index = ManualIndex::default();
        assert_eq!(
            locate_manual_source_in(&ManualRequest::new(" ", None), &index),
            Err(LocateError::EmptyName)
        );
        assert_eq!(
            locate_manual_source_in(&ManualRequest::new("git", Some(" ".to_owned())), &index,),
            Err(LocateError::InvalidSection)
        );
    }
}
