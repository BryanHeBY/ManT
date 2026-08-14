//! Discovers and resolves local manual sources without invoking a man program.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fmt, fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
const DEFAULT_MANUAL_ROOTS: [&str; 4] = [
    "/usr/local/share/man",
    "/usr/local/man",
    "/usr/share/man",
    "/usr/man",
];
const SUPPORTED_COMPRESSION_SUFFIXES: [&str; 2] = [".gz", ".zst"];
const DEFAULT_MANUAL_SECTIONS: [&str; 16] = [
    "1", "1p", "n", "l", "8", "3", "3p", "0", "0p", "2", "3type", "5", "4", "9", "6", "7",
];

/// One validated manual lookup independent from CLI token syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualRequest {
    /// Manual topic without its category suffix.
    pub name: String,
    /// Optional exact native category such as `1` or `3p`.
    pub manual_section: Option<String>,
}

impl ManualRequest {
    /// Construct a normalized lookup request without performing I/O.
    #[must_use]
    pub fn new(name: impl Into<String>, manual_section: Option<String>) -> Self {
        Self {
            name: name.into(),
            manual_section,
        }
    }
}

/// One effective local manual page after path and locale precedence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualPage {
    /// Indexed manual topic.
    pub name: String,
    /// Native manual category derived from the containing `man<section>` tree.
    pub section: String,
    /// Physical source path, possibly compressed.
    pub path: PathBuf,
    /// Approved hierarchy root used to resolve this page's `.so` redirects.
    ///
    /// The indexed leaf itself may be a file symlink whose target is outside
    /// this root. Redirect targets must still remain inside it.
    pub manual_root: PathBuf,
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
        let section_order = current_manual_section_order();
        Self::from_roots_with_locale_and_sections(roots, locale.as_deref(), &section_order)
    }

    #[cfg(test)]
    fn from_roots_with_locale(roots: Vec<PathBuf>, locale: Option<&str>) -> Self {
        Self::from_roots_with_locale_and_sections(roots, locale, &default_manual_section_order())
    }

    fn from_roots_with_locale_and_sections(
        roots: Vec<PathBuf>,
        locale: Option<&str>,
        section_order: &[String],
    ) -> Self {
        let roots = deduplicate_paths(roots);
        let mut effective = BTreeMap::<(String, String), ManualPage>::new();
        for root in &roots {
            for page in scan_manual_root(root, locale) {
                effective
                    .entry((manual_name_key(&page.name), page.section.clone()))
                    .or_insert(page);
            }
        }
        let mut pages = effective.into_values().collect::<Vec<_>>();
        pages.sort_by(|left, right| {
            manual_name_key(&left.name)
                .cmp(&manual_name_key(&right.name))
                .then_with(|| compare_manual_sections(&left.section, &right.section, section_order))
        });
        Self { roots, pages }
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

    /// Resolve one page using an optional exact manual category.
    #[must_use]
    pub fn find(&self, name: &str, section: Option<&str>) -> Option<&ManualPage> {
        let name = name.trim();
        let section = section.map(str::trim);
        self.pages.iter().find(|page| {
            manual_names_equal(&page.name, name)
                && section.is_none_or(|section| page.section == section)
        })
    }

    /// Exact manual categories available for one logical page name.
    #[must_use]
    pub fn available_manual_sections(&self, name: &str) -> Vec<String> {
        let name = name.trim();
        self.pages
            .iter()
            .filter(|page| manual_names_equal(&page.name, name))
            .map(|page| page.section.clone())
            .collect()
    }
}

fn manual_names_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn manual_name_key(name: &str) -> String {
    #[cfg(windows)]
    {
        name.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        name.to_owned()
    }
}

fn current_manual_section_order() -> Vec<String> {
    env::var("MANSECT")
        .ok()
        .and_then(|value| parse_manual_section_order(&value))
        .unwrap_or_else(default_manual_section_order)
}

fn default_manual_section_order() -> Vec<String> {
    DEFAULT_MANUAL_SECTIONS.map(str::to_owned).to_vec()
}

fn parse_manual_section_order(value: &str) -> Option<Vec<String>> {
    let mut sections = Vec::new();
    for section in value
        .split(':')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !sections.iter().any(|existing| existing == section) {
            sections.push(section.to_owned());
        }
    }
    (!sections.is_empty()).then_some(sections)
}

fn compare_manual_sections(
    left: &str,
    right: &str,
    section_order: &[String],
) -> std::cmp::Ordering {
    let rank = |section: &str| {
        section_order
            .iter()
            .position(|candidate| candidate == section)
    };
    match (rank(left), rank(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.cmp(right),
    }
}

/// Minimal subprocess result shared by external data update operations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Expected source-discovery failures suitable for a user-facing CLI error.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocateError {
    /// The requested manual name was empty.
    EmptyName,
    /// An explicitly requested manual category was malformed.
    InvalidManualSection,
    /// No indexed page satisfied the request.
    NotFound {
        /// Requested manual topic.
        name: String,
        /// Exact requested native category, when supplied.
        requested_manual_section: Option<String>,
        /// Other indexed categories available for the same topic.
        available_manual_sections: Vec<String>,
    },
}

impl LocateError {
    /// Render this error as detail nested below an already identified topic.
    pub(crate) fn load_detail(&self) -> String {
        match self {
            Self::NotFound {
                requested_manual_section: Some(requested),
                available_manual_sections,
                ..
            } if !available_manual_sections.is_empty() => format!(
                "manual section '{requested}' is unavailable; available sections: {}",
                available_manual_sections.join(", ")
            ),
            Self::NotFound {
                requested_manual_section: Some(requested),
                ..
            } => format!("no source was found in manual section '{requested}'"),
            Self::NotFound { .. } => "no local manual source was found".to_owned(),
            Self::EmptyName | Self::InvalidManualSection => self.to_string(),
        }
    }
}

impl fmt::Display for LocateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("manual page name must not be empty"),
            Self::InvalidManualSection => formatter.write_str(
                "manual section must be a conventional number or the single letter 'l' or 'n'",
            ),
            Self::NotFound {
                name,
                requested_manual_section: Some(requested),
                available_manual_sections,
            } if !available_manual_sections.is_empty() => write!(
                formatter,
                "requested manual section '{requested}' is unavailable for '{name}'; available manual sections: {}",
                available_manual_sections.join(", ")
            ),
            Self::NotFound {
                name,
                requested_manual_section: Some(requested),
                ..
            } => write!(
                formatter,
                "no local manual source was found for '{name}' in manual section '{requested}'"
            ),
            Self::NotFound { name, .. } => {
                write!(formatter, "no local manual source was found for '{name}'")
            }
        }
    }
}

impl std::error::Error for LocateError {}

/// Discover manual roots from explicit variables and platform conventions.
#[must_use]
pub fn discover_manual_roots() -> Vec<PathBuf> {
    let environment = env::vars_os().collect::<HashMap<_, _>>();
    discover_manual_roots_with(&environment)
}

/// Locate a manual in an explicit immutable index.
///
/// # Errors
///
/// Returns [`LocateError`] for invalid requests and missing local sources.
pub fn locate_manual_source_in(
    request: &ManualRequest,
    index: &ManualIndex,
) -> Result<ManualPage, LocateError> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err(LocateError::EmptyName);
    }
    let section = request.manual_section.as_deref().map(str::trim);
    if section.is_some_and(|section| !crate::is_manual_section(section)) {
        return Err(LocateError::InvalidManualSection);
    }
    index
        .find(name, section)
        .cloned()
        .ok_or_else(|| LocateError::NotFound {
            name: name.to_owned(),
            requested_manual_section: section.map(ToOwned::to_owned),
            available_manual_sections: index.available_manual_sections(name),
        })
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
    #[cfg(windows)]
    if let Some(profile) = environment
        .get(OsStr::new("USERPROFILE"))
        .map(PathBuf::from)
    {
        roots.push(profile.join(".local/share/man"));
    }
    #[cfg(unix)]
    if let Some(home) = environment.get(OsStr::new("HOME")).map(PathBuf::from) {
        roots.push(home.join(".local/share/man"));
        roots.push(home.join(".local/man"));
        roots.push(home.join("man"));
    }
    #[cfg(unix)]
    if let Some(data_home) = environment
        .get(OsStr::new("XDG_DATA_HOME"))
        .map(PathBuf::from)
    {
        roots.push(data_home.join("man"));
    }
    #[cfg(unix)]
    if let Some(data_dirs) = environment.get(OsStr::new("XDG_DATA_DIRS")) {
        roots.extend(env::split_paths(data_dirs).map(|root| root.join("man")));
    }
    #[cfg(unix)]
    if let Some(path) = environment.get(OsStr::new("PATH")) {
        for binary_dir in env::split_paths(path) {
            if let Some(prefix) = binary_dir.parent() {
                roots.push(prefix.join("share/man"));
                roots.push(prefix.join("man"));
            }
        }
    }
    #[cfg(unix)]
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
            manual_root: root.to_path_buf(),
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
        // Follow an explicit leaf link only far enough to prove it names a
        // regular file. Directory links are never traversed, and broken links
        // are not indexed.
        if !fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
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
    });
    let filename = path.file_name()?.to_str()?;
    let stem = SUPPORTED_COMPRESSION_SUFFIXES
        .iter()
        .find_map(|suffix| filename.strip_suffix(suffix))
        .unwrap_or(filename);
    let (name, file_section) = stem.rsplit_once('.')?;
    let section_matches_directory = section_directory.as_ref().is_some_and(|directory| {
        file_section == directory
            || file_section
                .strip_prefix(directory)
                .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(char::is_alphabetic))
    });
    let flat_root_page = relative.components().count() == 1 && valid_flat_section(file_section);
    if name.is_empty() || (!section_matches_directory && !flat_root_page) {
        return None;
    }
    Some((name.to_owned(), file_section.to_owned()))
}

fn valid_flat_section(section: &str) -> bool {
    let mut characters = section.chars();
    match characters.next() {
        Some('1'..='9') => characters.all(char::is_alphabetic),
        Some('l' | 'n') => characters.next().is_none(),
        _ => false,
    }
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
        locate_manual_source_in, normalize_locale, parse_manual_section_order,
    };

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-manual-index-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[cfg(unix)]
    fn symlink_file(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_file(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[cfg(unix)]
    fn symlink_directory(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_directory(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[cfg(any(unix, windows))]
    fn created_link(result: std::io::Result<()>) -> bool {
        match result {
            Ok(()) => true,
            Err(error) if cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied => {
                false
            }
            Err(error) => panic!("create fixture symlink: {error}"),
        }
    }

    #[test]
    fn indexes_supported_sources_and_resolves_sections_without_man() {
        let root = temporary_root("lookup");
        fs::create_dir_all(root.join("man1")).expect("manual section");
        fs::create_dir_all(root.join("man3")).expect("manual section");
        fs::write(root.join("man1/printf.1.gz"), b"gzip placeholder").expect("manual");
        fs::write(root.join("man3/printf.3.zst"), b"zstd placeholder").expect("manual");
        fs::write(root.join("flat-tool.1"), b"flat manual").expect("flat manual");
        fs::write(root.join("README.md"), b"not a manual").expect("readme");
        fs::write(root.join("man1/ignored.1.xz"), b"unsupported").expect("manual");

        let index = ManualIndex::from_roots(vec![root.clone()]);
        assert_eq!(index.pages().len(), 3);
        assert_eq!(
            locate_manual_source_in(&ManualRequest::new("printf", None), &index)
                .expect("default section")
                .path,
            root.join("man1/printf.1.gz")
        );
        assert_eq!(
            locate_manual_source_in(&ManualRequest::new("printf", Some("3".to_owned())), &index,)
                .expect("selected section")
                .path,
            root.join("man3/printf.3.zst")
        );
        assert_eq!(index.pages()[0].manual_root, root);
        assert_eq!(
            locate_manual_source_in(&ManualRequest::new("flat-tool", None), &index)
                .expect("flat root page")
                .path,
            root.join("flat-tool.1")
        );
        assert!(matches!(
            locate_manual_source_in(&ManualRequest::new("ignored", None), &index),
            Err(LocateError::NotFound { .. })
        ));

        let invalid = locate_manual_source_in(
            &ManualRequest::new("printf", Some("DESCRIPTION".to_owned())),
            &index,
        )
        .expect_err("document heading is not a manual section");
        assert_eq!(invalid, LocateError::InvalidManualSection);

        let unavailable =
            locate_manual_source_in(&ManualRequest::new("printf", Some("5".to_owned())), &index)
                .expect_err("valid but unavailable manual section");
        assert_eq!(
            unavailable.to_string(),
            "requested manual section '5' is unavailable for 'printf'; available manual sections: 1, 3"
        );
        assert_eq!(
            unavailable.load_detail(),
            "manual section '5' is unavailable; available sections: 1, 3"
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn unqualified_lookup_uses_manual_section_precedence_instead_of_lexical_order() {
        let root = temporary_root("section-precedence");
        fs::create_dir_all(root.join("man5")).expect("section 5");
        fs::create_dir_all(root.join("man8")).expect("section 8");
        fs::write(root.join("man5/btrfs.5"), b".TH BTRFS 5\n").expect("section 5 page");
        fs::write(root.join("man8/btrfs.8"), b".TH BTRFS 8\n").expect("section 8 page");

        let index = ManualIndex::from_roots_with_locale(vec![root.clone()], None);

        assert_eq!(
            index.find("btrfs", None).map(|page| page.section.as_str()),
            Some("8")
        );
        assert_eq!(index.available_manual_sections("btrfs"), ["8", "5"]);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn explicit_manual_section_order_controls_unqualified_lookup() {
        let root = temporary_root("explicit-section-precedence");
        fs::create_dir_all(root.join("man5")).expect("section 5");
        fs::create_dir_all(root.join("man8")).expect("section 8");
        fs::write(root.join("man5/btrfs.5"), b".TH BTRFS 5\n").expect("section 5 page");
        fs::write(root.join("man8/btrfs.8"), b".TH BTRFS 8\n").expect("section 8 page");
        let order = vec!["5".to_owned(), "8".to_owned()];

        let index =
            ManualIndex::from_roots_with_locale_and_sections(vec![root.clone()], None, &order);

        assert_eq!(
            index.find("btrfs", None).map(|page| page.section.as_str()),
            Some("5")
        );
        assert_eq!(index.available_manual_sections("btrfs"), ["5", "8"]);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn mansect_is_colon_separated_trimmed_and_deduplicated() {
        assert_eq!(
            parse_manual_section_order(" 8:1:8::5 "),
            Some(vec!["8".to_owned(), "1".to_owned(), "5".to_owned()])
        );
        assert_eq!(parse_manual_section_order(":: "), None);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn indexes_leaf_file_symlinks_without_traversing_linked_directories() {
        let base = temporary_root("symlink-boundary");
        let root = base.join("root");
        let man1 = root.join("man1");
        let linked_tree = base.join("linked-tree/man1");
        fs::create_dir_all(&man1).expect("manual section");
        fs::create_dir_all(&linked_tree).expect("linked manual section");
        fs::write(man1.join("target.1"), ".TH TARGET 1\n").expect("inside target");
        fs::write(base.join("outside.1"), ".TH OUTSIDE 1\n").expect("outside target");
        fs::write(linked_tree.join("nested.1"), ".TH NESTED 1\n").expect("nested target");
        if !created_link(symlink_file(&man1.join("target.1"), &man1.join("inside.1")))
            || !created_link(symlink_file(
                &base.join("outside.1"),
                &man1.join("outside.1"),
            ))
            || !created_link(symlink_file(
                &base.join("missing.1"),
                &man1.join("broken.1"),
            ))
            || !created_link(symlink_directory(
                &base.join("linked-tree"),
                &root.join("linked-tree"),
            ))
        {
            fs::remove_dir_all(base).expect("remove unsupported symlink fixture");
            return;
        }

        let index = ManualIndex::from_roots(vec![root]);
        assert!(index.find("inside", Some("1")).is_some());
        assert!(index.find("outside", Some("1")).is_some());
        assert!(index.find("broken", Some("1")).is_none());
        assert!(index.find("nested", Some("1")).is_none());
        fs::remove_dir_all(base).expect("remove fixture");
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

    #[cfg(unix)]
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

    #[cfg(windows)]
    #[test]
    fn windows_defaults_to_user_share_man_and_honors_manpath() {
        let profile = PathBuf::from(r"C:\Users\demo");
        let environment = HashMap::from([(
            OsString::from("USERPROFILE"),
            profile.as_os_str().to_owned(),
        )]);
        assert_eq!(
            discover_manual_roots_with(&environment),
            vec![profile.join(".local/share/man")]
        );

        let custom = PathBuf::from(r"D:\manuals");
        let mut environment = environment;
        environment.insert(
            OsString::from("MANPATH"),
            std::env::join_paths([&custom]).expect("join Windows MANPATH"),
        );
        assert_eq!(discover_manual_roots_with(&environment), vec![custom]);
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
            Err(LocateError::InvalidManualSection)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_manual_names_are_ascii_case_insensitive() {
        let index = ManualIndex {
            roots: vec![PathBuf::from(r"C:\man")],
            pages: vec![super::ManualPage {
                name: "cargo.exe".to_owned(),
                section: "1".to_owned(),
                path: PathBuf::from(r"C:\man\cargo.exe.1"),
                manual_root: PathBuf::from(r"C:\man"),
            }],
        };

        assert!(index.find("cargo.EXE", None).is_some());
    }
}
