//! Resolves installed-client caches and reads tldr pages without network I/O.

use std::{
    collections::{BTreeMap, HashSet},
    env,
    error::Error,
    ffi::OsStr,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use mant_ir::TldrDocument;

use crate::executable::{environment_value, find_executable};

use super::parser::{TldrPageLocation, TldrParseError, parse_tldr_page};

const ALL_PLATFORMS: &[&str] = &[
    "common",
    "linux",
    "osx",
    "macos",
    "windows",
    "android",
    "freebsd",
    "openbsd",
    "netbsd",
    "sunos",
    "cisco-ios",
    "dos",
];

/// Native host families supported by `ManT` distributions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    /// Linux filesystem and environment conventions.
    Linux,
    /// macOS filesystem and environment conventions.
    Macos,
    /// Windows filesystem and environment conventions.
    Windows,
}

impl HostPlatform {
    /// Identify the current build target.
    ///
    /// # Errors
    ///
    /// Returns [`TldrCacheError::UnsupportedPlatform`] outside supported hosts.
    pub fn current() -> Result<Self, TldrCacheError> {
        if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else if cfg!(target_os = "macos") {
            Ok(Self::Macos)
        } else if cfg!(windows) {
            Ok(Self::Windows)
        } else {
            Err(TldrCacheError::UnsupportedPlatform)
        }
    }
}

/// Offline cache discovery or page-read failure.
#[derive(Debug)]
pub enum TldrCacheError {
    /// The build target has no defined cache convention.
    UnsupportedPlatform,
    /// A Unix-like cache path requires `HOME`, but none is available.
    MissingHomeDirectory,
    /// A Windows cache path requires `LOCALAPPDATA`, but none is available.
    MissingLocalAppData,
    /// A selected cache page could not be read.
    Read {
        /// Physical cached page path.
        path: PathBuf,
        /// Underlying filesystem failure.
        source: io::Error,
    },
    /// A selected cache page did not satisfy the tldr dialect.
    Parse {
        /// Physical cached page path.
        path: PathBuf,
        /// Structured tldr parser failure.
        source: TldrParseError,
    },
}

impl fmt::Display for TldrCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("tldr cache lookup is unsupported on this platform")
            }
            Self::MissingHomeDirectory => {
                formatter.write_str("cannot locate a tldr cache without HOME")
            }
            Self::MissingLocalAppData => {
                formatter.write_str("cannot locate a tldr cache without LOCALAPPDATA")
            }
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "cannot read cached tldr page {}: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    formatter,
                    "cannot parse cached tldr page {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for TldrCacheError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::UnsupportedPlatform | Self::MissingHomeDirectory | Self::MissingLocalAppData => {
                None
            }
        }
    }
}

/// Resolve the `ManT`-owned fallback checkout for an explicit environment.
///
/// # Errors
///
/// Returns a platform-specific location error when neither an explicit
/// override nor the native cache base (`HOME` or `LOCALAPPDATA`) is available.
pub fn get_tldr_cache_dir(
    environment: &BTreeMap<String, String>,
    platform: HostPlatform,
) -> Result<PathBuf, TldrCacheError> {
    if let Some(path) = environment_value(environment, "MANT_TLDR_DIR") {
        return Ok(PathBuf::from(path));
    }
    match platform {
        HostPlatform::Linux => {
            let home = home_dir(environment)?;
            Ok(
                environment_value(environment, "XDG_CACHE_HOME").map_or_else(
                    || home.join(".cache").join("mant").join("tldr-pages"),
                    |cache| PathBuf::from(cache).join("mant").join("tldr-pages"),
                ),
            )
        }
        HostPlatform::Macos => Ok(home_dir(environment)?
            .join("Library")
            .join("Caches")
            .join("ManT")
            .join("tldr-pages")),
        HostPlatform::Windows => Ok(local_app_data(environment)?
            .join("ManT")
            .join("cache")
            .join("tldr-pages")),
    }
}

/// Return known installed-client cache roots in priority order.
///
/// # Errors
///
/// Returns [`TldrCacheError::MissingHomeDirectory`] when `HOME` is absent.
pub fn get_system_tldr_cache_dirs(
    environment: &BTreeMap<String, String>,
    platform: HostPlatform,
) -> Result<Vec<PathBuf>, TldrCacheError> {
    if platform == HostPlatform::Windows {
        let local = local_app_data(environment)?;
        let roaming = environment_value(environment, "APPDATA").map(PathBuf::from);
        let mut candidates = vec![
            local.join("tldr"),
            local.join("tlrc"),
            local.join("tealdeer").join("tldr-pages"),
        ];
        if let Some(roaming) = roaming {
            candidates.push(roaming.join("tldr"));
            candidates.push(roaming.join("tlrc"));
        }
        if let Some(home) = optional_home_dir(environment) {
            candidates.push(home.join(".tldrc").join("tldr"));
            candidates.push(home.join(".tldr").join("cache"));
            candidates.push(home.join(".tldr"));
        }
        return Ok(deduplicate_paths(candidates));
    }

    let home = home_dir(environment)?;
    let portable_cache = environment_value(environment, "XDG_CACHE_HOME")
        .map_or_else(|| home.join(".cache"), PathBuf::from);
    let native_cache = match platform {
        HostPlatform::Linux => portable_cache.clone(),
        HostPlatform::Macos => home.join("Library").join("Caches"),
        HostPlatform::Windows => unreachable!("Windows returned above"),
    };
    let mut candidates = vec![
        portable_cache.join("tldr"),
        native_cache.join("tlrc"),
        portable_cache.join("tlrc"),
        native_cache.join("tealdeer").join("tldr-pages"),
        portable_cache.join("tealdeer").join("tldr-pages"),
        // Homebrew's `tldr` formula installs tldr-c-client, which extracts
        // the upstream repository below this root on every supported host.
        home.join(".tldrc").join("tldr"),
        // The official Node client adds one private `cache` layer beneath its
        // configured root (which defaults to ~/.tldr).
        home.join(".tldr").join("cache"),
        home.join(".tldr"),
    ];

    if let Some(value) = environment_value(environment, "XDG_DATA_DIRS") {
        candidates.extend(
            env::split_paths(OsStr::new(value))
                .filter(|path| !path.as_os_str().is_empty())
                .map(|path| path.join("tldr")),
        );
    } else {
        candidates.extend(
            ["/usr/local/share", "/usr/share"]
                .into_iter()
                .map(|path| PathBuf::from(path).join("tldr")),
        );
    }
    Ok(deduplicate_paths(candidates))
}

/// Select installed-client caches followed by `ManT`'s private fallback.
///
/// # Errors
///
/// Propagates cache path resolution failures.
pub fn get_tldr_read_cache_dirs(
    environment: &BTreeMap<String, String>,
    platform: HostPlatform,
    tldr_installed: bool,
) -> Result<Vec<PathBuf>, TldrCacheError> {
    if environment_value(environment, "MANT_TLDR_DIR").is_some() {
        return get_tldr_cache_dir(environment, platform).map(|path| vec![path]);
    }
    let private_cache = get_tldr_cache_dir(environment, platform)?;
    if !tldr_installed {
        return Ok(vec![private_cache]);
    }
    let mut caches = get_system_tldr_cache_dirs(environment, platform)?;
    caches.push(private_cache);
    Ok(deduplicate_paths(caches))
}

/// Resolve locale candidates, retaining first occurrence priority.
#[must_use]
pub fn get_tldr_languages(environment: &BTreeMap<String, String>) -> Vec<String> {
    let mut languages = Vec::new();
    if environment
        .get("LANG")
        .is_some_and(|lang| !matches!(lang.as_str(), "C" | "POSIX"))
    {
        if let Some(language) = environment.get("LANGUAGE") {
            for locale in language.split(':') {
                languages.extend(normalize_locale(locale));
            }
        }
        if let Some(locale) = environment.get("LANG") {
            languages.extend(normalize_locale(locale));
        }
    }
    languages.push("en".to_owned());
    deduplicate_strings(languages)
}

/// Resolve host, common, then cross-platform fallback page directories.
#[must_use]
pub fn get_tldr_platforms(platform: HostPlatform) -> Vec<String> {
    let mut platforms = match platform {
        HostPlatform::Linux => vec!["linux".to_owned()],
        HostPlatform::Macos => vec!["osx".to_owned(), "macos".to_owned()],
        HostPlatform::Windows => vec!["windows".to_owned()],
    };
    platforms.extend(ALL_PLATFORMS.iter().map(ToString::to_string));
    deduplicate_strings(platforms)
}

/// Convert a multi-word query to the tldr filename convention.
#[must_use]
pub fn normalize_tldr_topic(topic: &str) -> String {
    topic
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// Reject a normalized topic that would escape the platform page directory.
///
/// The topic becomes a single `<page>.md` filename joined onto a cache root,
/// so it must be exactly one ordinary path component. Anything containing a
/// path separator, a `.`/`..` segment, or an absolute or prefix component is
/// refused before it reaches the filesystem, which prevents an untrusted topic
/// (for example one supplied over MCP) from reading files outside the cache.
fn is_safe_page_name(page_name: &str) -> bool {
    let mut components = Path::new(page_name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

/// Read one cached tldr page using current host conventions; never updates it.
///
/// # Errors
///
/// Returns a cache path, I/O, or parser error. A missing page is `Ok(None)`.
pub fn read_cached_tldr_page(topic: &str) -> Result<Option<TldrDocument>, TldrCacheError> {
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    let platform = HostPlatform::current()?;
    let cache_dirs = get_tldr_read_cache_dirs(
        &environment,
        platform,
        find_executable("tldr", &environment).is_some(),
    )?;
    read_cached_tldr_page_with(
        topic,
        &cache_dirs,
        &get_tldr_languages(&environment),
        &get_tldr_platforms(platform),
        &SystemFileReader,
    )
}

trait TldrFileReader {
    fn is_file(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
}

struct SystemFileReader;

impl TldrFileReader for SystemFileReader {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        // A tldr page is markdown, so hold it to the same byte ceiling as any
        // other markdown source. Reading unbounded here lets a corrupt cache
        // entry or a device file streamed in its place exhaust memory.
        let file = fs::File::open(path)?;
        crate::bounded::read_utf8(file, crate::query::MAX_MARKDOWN_BYTES, "Markdown document")
    }
}

fn read_cached_tldr_page_with(
    topic: &str,
    cache_dirs: &[PathBuf],
    languages: &[String],
    platforms: &[String],
    files: &dyn TldrFileReader,
) -> Result<Option<TldrDocument>, TldrCacheError> {
    let page_name = normalize_tldr_topic(topic);
    if page_name.is_empty() || !is_safe_page_name(&page_name) {
        return Ok(None);
    }

    // The client specification gives host platform precedence over language.
    for platform in platforms {
        for language in languages {
            let page_directories = if language == "en" {
                vec!["pages".to_owned(), "pages.en".to_owned()]
            } else {
                vec![format!("pages.{language}")]
            };
            for cache_dir in cache_dirs {
                for pages in &page_directories {
                    let source_path = cache_dir
                        .join(pages)
                        .join(platform)
                        .join(format!("{page_name}.md"));
                    if !files.is_file(&source_path) {
                        continue;
                    }
                    let markdown = files.read_to_string(&source_path).map_err(|source| {
                        TldrCacheError::Read {
                            path: source_path.clone(),
                            source,
                        }
                    })?;
                    let page = parse_tldr_page(
                        &markdown,
                        TldrPageLocation {
                            platform: platform.clone(),
                            language: language.clone(),
                            source_path: source_path.to_string_lossy().into_owned(),
                        },
                    )
                    .map_err(|source| TldrCacheError::Parse {
                        path: source_path,
                        source,
                    })?;
                    return Ok(Some(page));
                }
            }
        }
    }
    Ok(None)
}

fn home_dir(environment: &BTreeMap<String, String>) -> Result<PathBuf, TldrCacheError> {
    optional_home_dir(environment).ok_or(TldrCacheError::MissingHomeDirectory)
}

fn optional_home_dir(environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    environment_value(environment, "HOME")
        .or_else(|| environment_value(environment, "USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

fn local_app_data(environment: &BTreeMap<String, String>) -> Result<PathBuf, TldrCacheError> {
    environment_value(environment, "LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or(TldrCacheError::MissingLocalAppData)
}

fn normalize_locale(locale: &str) -> Vec<String> {
    let normalized = locale
        .split('.')
        .next()
        .unwrap_or_default()
        .replace('-', "_");
    if normalized.is_empty() || matches!(normalized.as_str(), "C" | "POSIX") {
        return Vec::new();
    }
    let language = normalized.split('_').next().unwrap_or_default().to_owned();
    if normalized == language {
        vec![language]
    } else {
        vec![normalized, language]
    }
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn deduplicate_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap},
        fs, io,
        path::{Path, PathBuf},
    };

    use super::{
        HostPlatform, SystemFileReader, TldrFileReader, get_system_tldr_cache_dirs,
        get_tldr_cache_dir, get_tldr_languages, get_tldr_platforms, get_tldr_read_cache_dirs,
        normalize_tldr_topic, read_cached_tldr_page_with,
    };
    use crate::query::MAX_MARKDOWN_BYTES;

    const PAGE: &str = "# tar\n\n> Archiving utility.\n\n- List: `tar --list`\n";

    #[derive(Default)]
    struct MemoryFiles {
        files: HashMap<PathBuf, String>,
    }

    impl TldrFileReader for MemoryFiles {
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }

        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "memory fixture is missing"))
        }
    }

    fn env(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn resolves_mant_and_installed_client_cache_conventions() {
        let environment = env(&[("HOME", "/home/test"), ("XDG_CACHE_HOME", "/cache")]);
        assert_eq!(
            get_tldr_cache_dir(&environment, HostPlatform::Linux).expect("cache dir"),
            PathBuf::from("/cache/mant/tldr-pages")
        );
        assert_eq!(
            get_tldr_cache_dir(&environment, HostPlatform::Macos).expect("cache dir"),
            PathBuf::from("/home/test/Library/Caches/ManT/tldr-pages")
        );
        assert_eq!(
            get_system_tldr_cache_dirs(&environment, HostPlatform::Linux).expect("system caches"),
            [
                "/cache/tldr",
                "/cache/tlrc",
                "/cache/tealdeer/tldr-pages",
                "/home/test/.tldrc/tldr",
                "/home/test/.tldr/cache",
                "/home/test/.tldr",
                "/usr/local/share/tldr",
                "/usr/share/tldr",
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            get_tldr_read_cache_dirs(&environment, HostPlatform::Linux, false)
                .expect("fallback cache"),
            [PathBuf::from("/cache/mant/tldr-pages")]
        );
        assert_eq!(
            get_tldr_read_cache_dirs(&environment, HostPlatform::Linux, true)
                .expect("client caches and fallback"),
            [
                "/cache/tldr",
                "/cache/tlrc",
                "/cache/tealdeer/tldr-pages",
                "/home/test/.tldrc/tldr",
                "/home/test/.tldr/cache",
                "/home/test/.tldr",
                "/usr/local/share/tldr",
                "/usr/share/tldr",
                "/cache/mant/tldr-pages",
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn windows_uses_local_application_data_for_private_and_client_caches() {
        let environment = env(&[
            ("LOCALAPPDATA", r"C:\Users\test\AppData\Local"),
            ("APPDATA", r"C:\Users\test\AppData\Roaming"),
            ("USERPROFILE", r"C:\Users\test"),
        ]);
        assert_eq!(
            get_tldr_cache_dir(&environment, HostPlatform::Windows).expect("private cache"),
            PathBuf::from(r"C:\Users\test\AppData\Local").join("ManT/cache/tldr-pages")
        );
        assert_eq!(
            get_system_tldr_cache_dirs(&environment, HostPlatform::Windows).expect("client caches"),
            [
                PathBuf::from(r"C:\Users\test\AppData\Local").join("tldr"),
                PathBuf::from(r"C:\Users\test\AppData\Local").join("tlrc"),
                PathBuf::from(r"C:\Users\test\AppData\Local").join("tealdeer/tldr-pages"),
                PathBuf::from(r"C:\Users\test\AppData\Roaming").join("tldr"),
                PathBuf::from(r"C:\Users\test\AppData\Roaming").join("tlrc"),
                PathBuf::from(r"C:\Users\test").join(".tldrc/tldr"),
                PathBuf::from(r"C:\Users\test").join(".tldr/cache"),
                PathBuf::from(r"C:\Users\test").join(".tldr"),
            ]
        );
        assert_eq!(
            &get_tldr_platforms(HostPlatform::Windows)[..2],
            ["windows", "common"]
        );
    }

    #[test]
    fn reads_homebrew_c_client_and_node_client_cache_layouts_on_macos() {
        let environment = env(&[("HOME", "/Users/test")]);
        let cache_dirs = get_system_tldr_cache_dirs(&environment, HostPlatform::Macos)
            .expect("macOS client caches");

        for source in [
            PathBuf::from("/Users/test/.tldrc/tldr/pages/common/tar.md"),
            PathBuf::from("/Users/test/.tldr/cache/pages/common/tar.md"),
        ] {
            let files = MemoryFiles {
                files: [(source.clone(), PAGE.to_owned())].into_iter().collect(),
            };
            let page = read_cached_tldr_page_with(
                "tar",
                &cache_dirs,
                &["en".to_owned()],
                &["osx".to_owned(), "common".to_owned()],
                &files,
            )
            .expect("cache read")
            .expect("page");

            assert_eq!(Path::new(&page.source_path), source);
        }
    }

    #[test]
    fn installed_client_miss_falls_back_to_mant_private_cache() {
        let environment = env(&[("HOME", "/home/test"), ("XDG_CACHE_HOME", "/cache")]);
        let cache_dirs = get_tldr_read_cache_dirs(&environment, HostPlatform::Linux, true)
            .expect("client and private caches");
        let private_page = PathBuf::from("/cache/mant/tldr-pages/pages/common/tar.md");
        let files = MemoryFiles {
            files: [(private_page.clone(), PAGE.to_owned())]
                .into_iter()
                .collect(),
        };

        let page = read_cached_tldr_page_with(
            "tar",
            &cache_dirs,
            &["en".to_owned()],
            &["linux".to_owned(), "common".to_owned()],
            &files,
        )
        .expect("cache read")
        .expect("private fallback page");

        assert_eq!(Path::new(&page.source_path), private_page);
    }

    #[test]
    fn explicit_cache_is_independent_from_an_installed_client() {
        let environment = env(&[("HOME", "/home/test"), ("MANT_TLDR_DIR", "/custom/tldr")]);
        assert_eq!(
            get_tldr_read_cache_dirs(&environment, HostPlatform::Linux, true)
                .expect("explicit cache"),
            [PathBuf::from("/custom/tldr")]
        );
    }

    #[test]
    fn normalizes_topic_locale_and_platform_priority() {
        let environment = env(&[("LANG", "pt_BR.UTF-8"), ("LANGUAGE", "zh_TW:pt_BR")]);
        assert_eq!(
            get_tldr_languages(&environment),
            ["zh_TW", "zh", "pt_BR", "pt", "en"]
        );
        assert_eq!(
            &get_tldr_platforms(HostPlatform::Linux)[..3],
            ["linux", "common", "osx"]
        );
        assert_eq!(normalize_tldr_topic(" Git Commit "), "git-commit");
    }

    #[test]
    fn host_platform_precedes_a_translated_common_page() {
        let root = PathBuf::from("/cache");
        let english_linux = root.join("pages/linux/tar.md");
        let translated_common = root.join("pages.zh/common/tar.md");
        let files = MemoryFiles {
            files: [
                (english_linux.clone(), PAGE.to_owned()),
                (translated_common, PAGE.to_owned()),
            ]
            .into_iter()
            .collect(),
        };
        let page = read_cached_tldr_page_with(
            "tar",
            &[root],
            &["zh".to_owned(), "en".to_owned()],
            &["linux".to_owned(), "common".to_owned()],
            &files,
        )
        .expect("cache read")
        .expect("page");
        assert_eq!(Path::new(&page.source_path), english_linux);
        assert_eq!(page.language, "en");
        assert_eq!(page.platform, "linux");
    }

    #[test]
    fn reads_pages_dot_en_layout_after_repository_layout() {
        let root = PathBuf::from("/cache/tlrc");
        let source = root.join("pages.en/linux/tar.md");
        let files = MemoryFiles {
            files: [(source.clone(), PAGE.to_owned())].into_iter().collect(),
        };
        let page = read_cached_tldr_page_with(
            "tar",
            &[root],
            &["en".to_owned()],
            &["linux".to_owned()],
            &files,
        )
        .expect("cache read")
        .expect("page");
        assert_eq!(Path::new(&page.source_path), source);
    }

    #[test]
    fn refuses_topics_that_escape_the_platform_page_directory() {
        let root = PathBuf::from("/cache");
        // A page planted where a naive join of a traversal topic would land.
        let escaped = PathBuf::from("/etc/hostname.md");
        let files = MemoryFiles {
            files: [(escaped, PAGE.to_owned())].into_iter().collect(),
        };

        for topic in ["../../../../etc/hostname", "/etc/hostname", "..", "a/b"] {
            let result = read_cached_tldr_page_with(
                topic,
                std::slice::from_ref(&root),
                &["en".to_owned()],
                &["linux".to_owned()],
                &files,
            )
            .expect("cache read must not error");
            assert!(
                result.is_none(),
                "traversal topic {topic:?} must not resolve a page"
            );
        }
    }

    #[test]
    fn only_single_ordinary_components_are_safe_page_names() {
        assert!(super::is_safe_page_name("tar"));
        assert!(super::is_safe_page_name("git-commit"));
        assert!(!super::is_safe_page_name("../etc/passwd"));
        assert!(!super::is_safe_page_name("/etc/passwd"));
        assert!(!super::is_safe_page_name(".."));
        assert!(!super::is_safe_page_name("a/b"));
    }

    fn temporary_page(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mant-tldr-cap-{label}-{}-{:?}.md",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn system_reader_reads_ordinary_pages_and_rejects_oversized_ones() {
        // The disk-backed reader must enforce the same byte ceiling as every
        // other markdown source; a small page reads through, one past the limit
        // is refused as invalid data rather than buffered whole.
        let ordinary = temporary_page("ordinary");
        fs::write(&ordinary, PAGE).expect("write ordinary page");
        assert_eq!(
            SystemFileReader
                .read_to_string(&ordinary)
                .expect("ordinary page reads"),
            PAGE
        );
        fs::remove_file(&ordinary).expect("remove ordinary fixture");

        let oversized = temporary_page("oversized");
        let bytes = usize::try_from(MAX_MARKDOWN_BYTES).expect("limit fits usize") + 1;
        fs::write(&oversized, vec![b'a'; bytes]).expect("write oversized page");
        let error = SystemFileReader
            .read_to_string(&oversized)
            .expect_err("oversized page is refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        fs::remove_file(&oversized).expect("remove oversized fixture");
    }
}
