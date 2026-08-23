//! Bounded, read-only virtual source trees for cross-platform `.so` expansion.

use std::{collections::BTreeMap, fmt};

/// Maximum number of sources retained by one [`SourceBundle`].
pub const MAX_SOURCE_BUNDLE_FILES: usize = 4_096;

/// Maximum size of one uncompressed source in a [`SourceBundle`].
pub const MAX_SOURCE_BUNDLE_FILE_BYTES: usize = 16 * 1024 * 1024;

/// Maximum aggregate uncompressed size of one [`SourceBundle`].
pub const MAX_SOURCE_BUNDLE_BYTES: usize = 64 * 1024 * 1024;

/// Categorizes a rejected virtual source without exposing implementation details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBundleErrorKind {
    /// The logical source path is empty, absolute, or contains unsafe components.
    InvalidPath,
    /// One source exceeds [`MAX_SOURCE_BUNDLE_FILE_BYTES`].
    SourceTooLarge,
    /// The bundle would exceed [`MAX_SOURCE_BUNDLE_FILES`].
    TooManySources,
    /// The bundle would exceed [`MAX_SOURCE_BUNDLE_BYTES`].
    BundleTooLarge,
}

/// Failure to add one source to a [`SourceBundle`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBundleError {
    path: String,
    kind: SourceBundleErrorKind,
    message: String,
}

impl SourceBundleError {
    /// Return the rejected logical path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the stable failure category.
    #[must_use]
    pub const fn kind(&self) -> SourceBundleErrorKind {
        self.kind
    }
}

impl fmt::Display for SourceBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for SourceBundleError {}

/// Bounded, immutable-at-parse-time collection of uncompressed roff sources.
///
/// Paths use `/` separators and are exact relative logical identities. Empty
/// components, `.`, `..`, absolute paths, backslashes, and NUL bytes are
/// rejected. The native parser can only resolve `.so` requests to entries in
/// this collection; it never falls back to the host filesystem.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceBundle {
    sources: BTreeMap<String, Vec<u8>>,
    total_bytes: usize,
}

impl SourceBundle {
    /// Create an empty source bundle.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
            total_bytes: 0,
        }
    }

    /// Return the number of logical sources in this bundle.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Return whether this bundle contains no sources.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Return the aggregate uncompressed byte size.
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Return one exact logical source.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.sources.get(path).map(Vec::as_slice)
    }

    /// Insert or replace one uncompressed source.
    ///
    /// The previous bytes are returned when `path` already existed. Failed
    /// insertions leave the bundle unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`SourceBundleError`] when the logical path is unsafe or a
    /// documented source-count or byte limit would be exceeded.
    pub fn insert(
        &mut self,
        path: impl Into<String>,
        source: impl Into<Vec<u8>>,
    ) -> Result<Option<Vec<u8>>, SourceBundleError> {
        let path = path.into();
        validate_path(&path)?;
        let source = source.into();
        if source.len() > MAX_SOURCE_BUNDLE_FILE_BYTES {
            return Err(error(
                path,
                SourceBundleErrorKind::SourceTooLarge,
                format!(
                    "source has {} bytes; the maximum is {MAX_SOURCE_BUNDLE_FILE_BYTES}",
                    source.len()
                ),
            ));
        }

        let previous_len = self.sources.get(&path).map_or(0, Vec::len);
        if previous_len == 0
            && !self.sources.contains_key(&path)
            && self.sources.len() == MAX_SOURCE_BUNDLE_FILES
        {
            return Err(error(
                path,
                SourceBundleErrorKind::TooManySources,
                format!("bundle already contains {MAX_SOURCE_BUNDLE_FILES} sources"),
            ));
        }
        let next_total = self
            .total_bytes
            .checked_sub(previous_len)
            .and_then(|total| total.checked_add(source.len()))
            .filter(|total| *total <= MAX_SOURCE_BUNDLE_BYTES)
            .ok_or_else(|| {
                error(
                    path.clone(),
                    SourceBundleErrorKind::BundleTooLarge,
                    format!("bundle would exceed {MAX_SOURCE_BUNDLE_BYTES} bytes"),
                )
            })?;

        let previous = self.sources.insert(path, source);
        self.total_bytes = next_total;
        Ok(previous)
    }

    pub(crate) fn sources(&self) -> impl ExactSizeIterator<Item = (&str, &[u8])> {
        self.sources
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_slice()))
    }
}

fn validate_path(path: &str) -> Result<(), SourceBundleError> {
    let invalid = path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', '\0'])
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."));
    if invalid {
        Err(error(
            path.to_owned(),
            SourceBundleErrorKind::InvalidPath,
            "logical paths must be normalized relative paths using '/' separators".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn error(path: String, kind: SourceBundleErrorKind, message: String) -> SourceBundleError {
    SourceBundleError {
        path,
        kind,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SOURCE_BUNDLE_BYTES, MAX_SOURCE_BUNDLE_FILE_BYTES, MAX_SOURCE_BUNDLE_FILES,
        SourceBundle, SourceBundleErrorKind,
    };

    #[test]
    fn bundle_paths_are_exact_relative_posix_identities() {
        let mut bundle = SourceBundle::new();
        bundle
            .insert("man3/foo.3", b"foo".to_vec())
            .expect("insert normalized source");
        assert_eq!(bundle.get("man3/foo.3"), Some(b"foo".as_slice()));

        for path in [
            "",
            "/foo.1",
            "foo\\bar.1",
            "foo//bar.1",
            "./foo.1",
            "../foo.1",
        ] {
            let error = bundle
                .insert(path, Vec::new())
                .expect_err("reject unsafe logical path");
            assert_eq!(error.kind(), SourceBundleErrorKind::InvalidPath, "{path:?}");
        }
    }

    #[test]
    fn replacing_a_source_updates_the_aggregate_size_transactionally() {
        let mut bundle = SourceBundle::new();
        assert_eq!(bundle.insert("foo.1", b"one".to_vec()), Ok(None));
        assert_eq!(bundle.total_bytes(), 3);
        assert_eq!(
            bundle.insert("foo.1", b"replacement".to_vec()),
            Ok(Some(b"one".to_vec()))
        );
        assert_eq!(bundle.total_bytes(), b"replacement".len());
    }

    #[test]
    fn documented_bundle_limits_are_enforced_without_partial_mutation() {
        let mut bundle = SourceBundle::new();
        let error = bundle
            .insert("oversized.1", vec![0; MAX_SOURCE_BUNDLE_FILE_BYTES + 1])
            .expect_err("reject oversized source");
        assert_eq!(error.kind(), SourceBundleErrorKind::SourceTooLarge);
        assert!(bundle.is_empty());

        for index in 0..MAX_SOURCE_BUNDLE_FILES {
            bundle
                .insert(format!("source-{index}.1"), Vec::new())
                .expect("fill source count exactly");
        }
        let error = bundle
            .insert("one-too-many.1", Vec::new())
            .expect_err("reject source beyond count cap");
        assert_eq!(error.kind(), SourceBundleErrorKind::TooManySources);
        assert_eq!(bundle.len(), MAX_SOURCE_BUNDLE_FILES);

        let mut aggregate = SourceBundle::new();
        for index in 0..MAX_SOURCE_BUNDLE_BYTES / MAX_SOURCE_BUNDLE_FILE_BYTES {
            aggregate
                .insert(
                    format!("large-{index}.1"),
                    vec![0; MAX_SOURCE_BUNDLE_FILE_BYTES],
                )
                .expect("fill aggregate byte limit exactly");
        }
        let error = aggregate
            .insert("aggregate-overflow.1", vec![0])
            .expect_err("reject aggregate byte overflow");
        assert_eq!(error.kind(), SourceBundleErrorKind::BundleTooLarge);
        assert_eq!(aggregate.total_bytes(), MAX_SOURCE_BUNDLE_BYTES);
        assert_eq!(aggregate.len(), 4);
    }
}
