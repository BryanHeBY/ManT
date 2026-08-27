//! Caller-owned source identity and explicit include resolution.

use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use crate::Limits;

/// Stable logical name for one source, independent of a host filesystem path.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceName(Box<str>);

impl SourceName {
    /// Construct a non-empty, NUL-free logical source name.
    ///
    /// Names are labels, not authorization to access a host path.  A resolver
    /// receives raw include targets separately and decides whether they map to
    /// a logical name.
    ///
    /// # Errors
    ///
    /// Returns [`SourceNameError`] for empty names or embedded NUL bytes.
    pub fn new(value: impl AsRef<str>) -> Result<Self, SourceNameError> {
        let value = value.as_ref();
        (!value.is_empty() && !value.contains('\0'))
            .then(|| Self(value.into()))
            .ok_or(SourceNameError)
    }

    /// Borrow the caller-facing logical identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Invalid [`SourceName`] input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceNameError;

impl fmt::Display for SourceNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("source names must be non-empty and NUL-free")
    }
}

impl std::error::Error for SourceNameError {}

/// Opaque document-local identity for one root or resolved source.
///
/// A `SourceId` is issued by the parser while it builds a [`crate::Document`].
/// It is not a host path and must be resolved through that document's source
/// map before presentation to a user.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceId(pub(crate) u32);

impl fmt::Debug for SourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SourceId(..)")
    }
}

/// One-based physical source position derived from a byte offset.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourcePosition {
    /// One-based physical input line.
    pub line: u32,
    /// One-based byte column in that physical line.
    pub column: u32,
}

/// Borrowed bytes and their logical identity passed to the parser core.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    /// Stable identity used in diagnostics and resolver requests.
    pub name: &'a SourceName,
    /// Original byte sequence; it is never pre-decoded or NUL-truncated.
    pub bytes: &'a [u8],
}

impl<'a> Source<'a> {
    /// Create one borrowed parsing input.
    #[must_use]
    pub const fn new(name: &'a SourceName, bytes: &'a [u8]) -> Self {
        Self { name, bytes }
    }
}

/// Context supplied to a resolver for one `.so` or future include request.
#[derive(Clone, Copy, Debug)]
pub struct IncludeRequest<'a> {
    /// Source containing the request.
    pub including: &'a SourceName,
    /// Uninterpreted bytes after the request name.
    pub raw_target: &'a [u8],
    /// Remaining source-depth budget.
    pub remaining_depth: usize,
    /// Remaining aggregate source-byte budget.
    pub remaining_bytes: usize,
}

/// Source returned by an explicit resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSource {
    /// Logical identity assigned by the resolver.
    pub name: SourceName,
    /// Complete uncompressed source bytes owned by the resolver result.
    pub bytes: Vec<u8>,
}

/// Resolver failure that has no coherent source to parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolveError {
    /// Stable implementation-defined category.
    pub kind: ResolveErrorKind,
    /// Human explanation; callers should use `kind` for branching.
    pub message: Box<str>,
}

/// Stable categories for explicit resolver failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolveErrorKind {
    /// A target cannot be represented by the resolver's logical path policy.
    InvalidTarget,
    /// A configured root or backing store could not be read.
    Read,
    /// A resolver-specific containment or cycle policy rejected the target.
    Denied,
    /// A resolver-level byte or graph budget was exhausted.
    Limit,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ResolveError {}

/// Explicit authority to turn an include target into owned source bytes.
///
/// The core parser has no filesystem fallback.  Returning `Ok(None)` means the
/// target is unavailable and lets parser recovery emit a non-fatal diagnostic.
pub trait SourceResolver {
    /// Resolve one requested source without hidden host access.
    ///
    /// # Errors
    ///
    /// Returns a resolver failure when resolution itself cannot continue.
    fn resolve(
        &mut self,
        request: IncludeRequest<'_>,
    ) -> Result<Option<ResolvedSource>, ResolveError>;
}

/// Filesystem-backed `.so` resolver confined to one canonical directory.
///
/// It never examines a process working directory.  The resolver canonicalizes
/// its root at construction and every accepted target before opening it.  A
/// target may use `.` or `..` while it remains inside that root; absolute
/// paths, non-UTF-8 logical names, and paths (including symlinks) resolving
/// outside the root are rejected.  The canonical root-relative path becomes
/// the [`SourceName`], so aliases through in-root symlinks cannot evade the
/// parser's source-name cycle detection.
///
/// The standard library cannot make a canonicalize-then-open sequence safe
/// against a malicious concurrent filesystem mutation.  Callers needing that
/// stronger OS capability boundary should provide a custom [`SourceResolver`]
/// backed by a directory-handle API; this adapter is deliberately explicit
/// about its portable, best-effort filesystem authority.
#[derive(Debug)]
pub struct ContainedRootResolver {
    root: PathBuf,
    maximum_source_bytes: usize,
}

impl ContainedRootResolver {
    /// Construct a resolver rooted at an existing directory.
    ///
    /// `limits.max_root_source_bytes` caps each root or resolved file before
    /// its bytes are allocated.  The parser independently enforces aggregate
    /// source-graph limits using [`IncludeRequest::remaining_bytes`].
    ///
    /// # Errors
    ///
    /// Returns a typed read failure when `root` cannot be canonicalized or is
    /// not a directory.
    pub fn new(root: impl AsRef<Path>, limits: &Limits) -> Result<Self, ResolveError> {
        let root = fs::canonicalize(root.as_ref()).map_err(|error| ResolveError {
            kind: ResolveErrorKind::Read,
            message: format!("cannot canonicalize contained source root: {error}").into(),
        })?;
        if !root.is_dir() {
            return Err(ResolveError {
                kind: ResolveErrorKind::Read,
                message: format!(
                    "contained source root is not a directory: {}",
                    root.display()
                )
                .into(),
            });
        }
        Ok(Self {
            root,
            maximum_source_bytes: limits.max_root_source_bytes,
        })
    }

    /// Return the canonical directory that bounds all resolver authority.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Canonicalize and read the caller-authorized root file.
    ///
    /// The returned [`SourceName`] is the canonical root-relative POSIX
    /// identity used by subsequent `.so` lookups.
    pub(crate) fn read_root(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<(SourceName, Vec<u8>), ResolveError> {
        let path = canonicalize_contained_path(&self.root, path.as_ref())?;
        let name = logical_name_from_canonical_path(&self.root, &path)?;
        let bytes = read_source_file(&path, self.maximum_source_bytes)?;
        Ok((name, bytes))
    }
}

impl SourceResolver for ContainedRootResolver {
    fn resolve(
        &mut self,
        request: IncludeRequest<'_>,
    ) -> Result<Option<ResolvedSource>, ResolveError> {
        let target = std::str::from_utf8(request.raw_target).map_err(|_| ResolveError {
            kind: ResolveErrorKind::InvalidTarget,
            message: "include target is not UTF-8 logical path text".into(),
        })?;
        let logical =
            normalize_include_target(request.including.as_str(), target).ok_or_else(|| {
                ResolveError {
                    kind: ResolveErrorKind::InvalidTarget,
                    message: "include target escapes its logical source directory".into(),
                }
            })?;
        let requested = logical_path_under_root(&self.root, &logical)?;
        let path = match fs::canonicalize(&requested) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(ResolveError {
                    kind: ResolveErrorKind::Read,
                    message: format!("cannot canonicalize include {logical:?}: {error}").into(),
                });
            }
        };
        if !path.starts_with(&self.root) {
            return Err(ResolveError {
                kind: ResolveErrorKind::Denied,
                message: format!("include {logical:?} resolves outside the configured root").into(),
            });
        }
        let name = logical_name_from_canonical_path(&self.root, &path)?;
        let maximum = self.maximum_source_bytes.min(request.remaining_bytes);
        let bytes = read_source_file(&path, maximum)?;
        Ok(Some(ResolvedSource { name, bytes }))
    }
}

fn canonicalize_contained_path(root: &Path, path: &Path) -> Result<PathBuf, ResolveError> {
    let path = fs::canonicalize(path).map_err(|error| ResolveError {
        kind: ResolveErrorKind::Read,
        message: format!("cannot canonicalize contained source: {error}").into(),
    })?;
    path.starts_with(root)
        .then_some(path)
        .ok_or_else(|| ResolveError {
            kind: ResolveErrorKind::Denied,
            message: "source path resolves outside the configured root".into(),
        })
}

fn logical_path_under_root(root: &Path, logical: &str) -> Result<PathBuf, ResolveError> {
    if logical.is_empty() || logical.starts_with('/') || logical.contains(['\\', '\0']) {
        return Err(ResolveError {
            kind: ResolveErrorKind::InvalidTarget,
            message: "include target is not a relative POSIX path".into(),
        });
    }
    Ok(logical
        .split('/')
        .filter(|component| !component.is_empty())
        .fold(root.to_path_buf(), |path, component| path.join(component)))
}

fn logical_name_from_canonical_path(root: &Path, path: &Path) -> Result<SourceName, ResolveError> {
    let relative = path.strip_prefix(root).map_err(|_| ResolveError {
        kind: ResolveErrorKind::Denied,
        message: "source path resolves outside the configured root".into(),
    })?;
    let mut components = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(ResolveError {
                kind: ResolveErrorKind::InvalidTarget,
                message: "canonical source path has no relative logical identity".into(),
            });
        };
        let component = component.to_str().ok_or_else(|| ResolveError {
            kind: ResolveErrorKind::InvalidTarget,
            message: "source path is not valid UTF-8 logical path text".into(),
        })?;
        components.push(component);
    }
    SourceName::new(components.join("/")).map_err(|error| ResolveError {
        kind: ResolveErrorKind::InvalidTarget,
        message: format!("source path cannot become a logical identity: {error}").into(),
    })
}

fn read_source_file(path: &Path, maximum: usize) -> Result<Vec<u8>, ResolveError> {
    let maximum_plus_one = maximum.checked_add(1).ok_or_else(|| ResolveError {
        kind: ResolveErrorKind::Limit,
        message: "source-byte limit cannot be represented for bounded I/O".into(),
    })?;
    let mut file = File::open(path).map_err(|error| ResolveError {
        kind: ResolveErrorKind::Read,
        message: format!("cannot read {}: {error}", path.display()).into(),
    })?;
    let mut bytes = Vec::with_capacity(maximum_plus_one.min(64 * 1024));
    file.by_ref()
        .take(u64::try_from(maximum_plus_one).expect("usize fits u64 on supported targets"))
        .read_to_end(&mut bytes)
        .map_err(|error| ResolveError {
            kind: ResolveErrorKind::Read,
            message: format!("cannot read {}: {error}", path.display()).into(),
        })?;
    if bytes.len() > maximum {
        return Err(ResolveError {
            kind: ResolveErrorKind::Limit,
            message: format!(
                "{} has {} bytes; configured source limit is {maximum}",
                path.display(),
                bytes.len()
            )
            .into(),
        });
    }
    Ok(bytes)
}

/// Stable reason a virtual source could not be added to a [`SourceBundle`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BundleErrorKind {
    /// The logical path is empty, absolute, non-POSIX, or contains unsafe components.
    InvalidPath,
    /// One source is larger than the configured per-source limit.
    SourceTooLarge,
    /// The source-count limit would be exceeded.
    TooManySources,
    /// The aggregate source-byte limit would be exceeded.
    BundleTooLarge,
}

/// Failed virtual-source insertion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleError {
    path: Box<str>,
    kind: BundleErrorKind,
    message: Box<str>,
}

impl BundleError {
    /// Return the rejected logical path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the stable rejection category.
    #[must_use]
    pub const fn kind(&self) -> BundleErrorKind {
        self.kind
    }
}

impl fmt::Display for BundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BundleError {}

/// Bounded in-memory source graph with normalized relative POSIX identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBundle {
    limits: Limits,
    sources: BTreeMap<Box<str>, Vec<u8>>,
    total_bytes: usize,
}

impl Default for SourceBundle {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

impl SourceBundle {
    /// Create a bundle using the subset of `limits` relevant to source graphs.
    #[must_use]
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            sources: BTreeMap::new(),
            total_bytes: 0,
        }
    }

    /// Return the number of logical sources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Return whether the bundle has no sources.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Return the aggregate uncompressed byte count.
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Return one exact logical source without host fallback.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.sources.get(path).map(Vec::as_slice)
    }

    /// Insert or replace a logical source transactionally.
    ///
    /// # Errors
    ///
    /// Failed insertion leaves the bundle unchanged.
    pub fn insert(
        &mut self,
        path: impl Into<Box<str>>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Option<Vec<u8>>, BundleError> {
        let path = path.into();
        validate_logical_path(&path)?;
        let bytes = bytes.into();
        if bytes.len() > self.limits.max_root_source_bytes {
            return Err(bundle_error(
                path,
                BundleErrorKind::SourceTooLarge,
                "source exceeds max_root_source_bytes",
            ));
        }
        let previous_len = self.sources.get(&path).map_or(0, Vec::len);
        if previous_len == 0
            && !self.sources.contains_key(&path)
            && self.sources.len() >= self.limits.max_sources
        {
            return Err(bundle_error(
                path,
                BundleErrorKind::TooManySources,
                "source bundle exceeds max_sources",
            ));
        }
        let next_total = self
            .total_bytes
            .checked_sub(previous_len)
            .and_then(|total| total.checked_add(bytes.len()))
            .filter(|total| *total <= self.limits.max_total_source_bytes)
            .ok_or_else(|| {
                bundle_error(
                    path.clone(),
                    BundleErrorKind::BundleTooLarge,
                    "source bundle exceeds max_total_source_bytes",
                )
            })?;
        let previous = self.sources.insert(path, bytes);
        self.total_bytes = next_total;
        Ok(previous)
    }
}

impl SourceResolver for SourceBundle {
    fn resolve(
        &mut self,
        request: IncludeRequest<'_>,
    ) -> Result<Option<ResolvedSource>, ResolveError> {
        let target = std::str::from_utf8(request.raw_target).map_err(|_| ResolveError {
            kind: ResolveErrorKind::InvalidTarget,
            message: "include target is not UTF-8 logical path text".into(),
        })?;
        let path =
            normalize_include_target(request.including.as_str(), target).ok_or_else(|| {
                ResolveError {
                    kind: ResolveErrorKind::InvalidTarget,
                    message: "include target escapes its logical source directory".into(),
                }
            })?;
        Ok(self.sources.get(path.as_str()).map(|bytes| ResolvedSource {
            name: SourceName(path.into()),
            bytes: bytes.clone(),
        }))
    }
}

fn validate_logical_path(path: &str) -> Result<(), BundleError> {
    let invalid = path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', '\0'])
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."));
    (!invalid).then_some(()).ok_or_else(|| {
        bundle_error(
            path.into(),
            BundleErrorKind::InvalidPath,
            "logical source paths must be normalized relative POSIX paths",
        )
    })
}

fn normalize_include_target(including: &str, target: &str) -> Option<String> {
    if target.starts_with('/') || target.contains(['\\', '\0']) {
        return None;
    }
    let mut components = including
        .rsplit_once('/')
        .map_or_else(Vec::new, |(parent, _)| {
            parent.split('/').map(str::to_owned).collect()
        });
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            value => components.push(value.to_owned()),
        }
    }
    (!components.is_empty()).then(|| components.join("/"))
}

fn bundle_error(path: Box<str>, kind: BundleErrorKind, message: &'static str) -> BundleError {
    BundleError {
        path,
        kind,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{IncludeRequest, Limits, SourceName, SourceResolver};

    use super::{BundleErrorKind, SourceBundle};

    #[test]
    fn virtual_sources_are_transactional_and_do_not_accept_host_paths() {
        let mut bundle = SourceBundle::default();
        bundle.insert("man1/demo.1", b"first".to_vec()).unwrap();
        let error = bundle.insert("../escape.1", b"bad".to_vec()).unwrap_err();
        assert_eq!(error.kind(), BundleErrorKind::InvalidPath);
        assert_eq!(bundle.get("man1/demo.1"), Some(b"first".as_slice()));

        let previous = bundle.insert("man1/demo.1", b"second".to_vec()).unwrap();
        assert_eq!(previous, Some(b"first".to_vec()));
        assert_eq!(bundle.total_bytes(), 6);
    }

    #[test]
    fn resolver_has_no_implicit_current_directory_fallback() {
        let mut bundle = SourceBundle::default();
        bundle.insert("man1/child.1", b"child".to_vec()).unwrap();
        let including = SourceName::new("man1/root.1").unwrap();
        let found = bundle
            .resolve(IncludeRequest {
                including: &including,
                raw_target: b"child.1",
                remaining_depth: 1,
                remaining_bytes: 64,
            })
            .unwrap()
            .expect("bundle child");
        assert_eq!(found.name.as_str(), "man1/child.1");
        assert!(
            bundle
                .resolve(IncludeRequest {
                    including: &including,
                    raw_target: b"missing.1",
                    remaining_depth: 1,
                    remaining_bytes: 64,
                })
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn source_graph_limits_are_applied_before_mutation() {
        let limits = Limits {
            max_sources: 1,
            ..Limits::default()
        };
        let mut bundle = SourceBundle::new(limits);
        bundle.insert("a.1", b"a".to_vec()).unwrap();
        let error = bundle.insert("b.1", b"b".to_vec()).unwrap_err();
        assert_eq!(error.kind(), BundleErrorKind::TooManySources);
        assert_eq!(bundle.len(), 1);
    }
}
