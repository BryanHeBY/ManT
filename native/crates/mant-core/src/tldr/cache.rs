//! Resolves installed-client caches and reads tldr pages without network I/O.

use std::{
    collections::{BTreeMap, HashSet},
    env,
    error::Error,
    ffi::OsStr,
    fmt, fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use mant_ast::TldrDocument;

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

/// Native host families supported by Mant distributions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    Macos,
}

impl HostPlatform {
    /// Identify the current build target.
    ///
    /// # Errors
    ///
    /// Returns [`TldrCacheError::UnsupportedPlatform`] outside Linux and macOS.
    pub fn current() -> Result<Self, TldrCacheError> {
        if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else if cfg!(target_os = "macos") {
            Ok(Self::Macos)
        } else {
            Err(TldrCacheError::UnsupportedPlatform)
        }
    }
}

/// Offline cache discovery or page-read failure.
#[derive(Debug)]
pub enum TldrCacheError {
    UnsupportedPlatform,
    MissingHomeDirectory,
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: TldrParseError,
    },
}

impl fmt::Display for TldrCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("tldr cache lookup is supported only on Linux and macOS")
            }
            Self::MissingHomeDirectory => {
                formatter.write_str("cannot locate a tldr cache without HOME")
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
            Self::UnsupportedPlatform | Self::MissingHomeDirectory => None,
        }
    }
}

/// Resolve the Mant-owned fallback checkout for an explicit environment.
///
/// # Errors
///
/// Returns [`TldrCacheError::MissingHomeDirectory`] when neither an explicit
/// override nor `HOME` is available.
pub fn get_tldr_cache_dir(
    environment: &BTreeMap<String, String>,
    platform: HostPlatform,
) -> Result<PathBuf, TldrCacheError> {
    if let Some(path) = environment.get("MANT_TLDR_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = home_dir(environment)?;
    Ok(match platform {
        HostPlatform::Linux => environment.get("XDG_CACHE_HOME").map_or_else(
            || home.join(".cache").join("mant").join("tldr-pages"),
            |cache| PathBuf::from(cache).join("mant").join("tldr-pages"),
        ),
        HostPlatform::Macos => home
            .join("Library")
            .join("Caches")
            .join("mant")
            .join("tldr-pages"),
    })
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
    let home = home_dir(environment)?;
    let portable_cache = environment
        .get("XDG_CACHE_HOME")
        .map_or_else(|| home.join(".cache"), PathBuf::from);
    let native_cache = match platform {
        HostPlatform::Linux => portable_cache.clone(),
        HostPlatform::Macos => home.join("Library").join("Caches"),
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

    if let Some(value) = environment.get("XDG_DATA_DIRS") {
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

/// Select installed-client caches or Mant's private fallback checkout.
///
/// # Errors
///
/// Propagates cache path resolution failures.
pub fn get_tldr_read_cache_dirs(
    environment: &BTreeMap<String, String>,
    platform: HostPlatform,
    tldr_installed: bool,
) -> Result<Vec<PathBuf>, TldrCacheError> {
    if environment.contains_key("MANT_TLDR_DIR") {
        return get_tldr_cache_dir(environment, platform).map(|path| vec![path]);
    }
    if tldr_installed {
        get_system_tldr_cache_dirs(environment, platform)
    } else {
        get_tldr_cache_dir(environment, platform).map(|path| vec![path])
    }
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
        fs::read_to_string(path)
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
    if page_name.is_empty() {
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
    environment
        .get("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or(TldrCacheError::MissingHomeDirectory)
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

fn find_executable(name: &str, environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = environment.get("PATH")?;
    env::split_paths(OsStr::new(path))
        .map(|directory| directory.join(name))
        .find(|candidate| {
            candidate.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
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
        io,
        path::{Path, PathBuf},
    };

    use super::{
        HostPlatform, TldrFileReader, get_system_tldr_cache_dirs, get_tldr_cache_dir,
        get_tldr_languages, get_tldr_platforms, get_tldr_read_cache_dirs, normalize_tldr_topic,
        read_cached_tldr_page_with,
    };

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
            PathBuf::from("/home/test/Library/Caches/mant/tldr-pages")
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

            assert_eq!(page.source_path, source.to_string_lossy());
        }
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
        assert_eq!(page.source_path, english_linux.to_string_lossy());
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
        assert_eq!(page.source_path, source.to_string_lossy());
    }
}
