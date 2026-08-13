//! Product-level failures while loading and parsing one native manual.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use libmandoc_rs::ParseError;

/// Stable category for a native manual failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualErrorKind {
    /// Source bytes could not be read.
    Read,
    /// Compressed source bytes could not be decoded.
    Decompression,
    /// A configured resource bound was exceeded.
    Limit,
    /// A path or redirect crossed the approved manual tree.
    UnsafePath,
    /// A native alias redirect was invalid or could not be resolved.
    Redirect,
    /// libmandoc rejected or could not represent the source.
    Parse,
}

/// Failure produced by `ManT`'s source policy or by the underlying roff parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManualError {
    /// Filesystem read failure.
    Read {
        /// Original source path.
        path: PathBuf,
        /// Stable human-readable detail.
        message: String,
    },
    /// Top-level decompression failure.
    Decompression {
        /// Original source path.
        path: PathBuf,
        /// Stable human-readable detail.
        message: String,
    },
    /// Input resource limit violation.
    Limit {
        /// Original source path.
        path: PathBuf,
        /// Stable human-readable detail.
        message: String,
    },
    /// Manual-tree containment policy violation.
    UnsafePath {
        /// Rejected source or target path.
        path: PathBuf,
        /// Stable human-readable detail.
        message: String,
    },
    /// Invalid or unresolved `.so` alias page.
    Redirect {
        /// Alias source path.
        path: PathBuf,
        /// Stable human-readable detail.
        message: String,
    },
    /// Failure reported by the low-level libmandoc binding.
    Parse(ParseError),
}

impl ManualError {
    pub(super) fn read(path: &Path, message: impl Into<String>) -> Self {
        Self::Read {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    pub(super) fn decompression(path: &Path, message: impl Into<String>) -> Self {
        Self::Decompression {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    pub(super) fn limit(path: &Path, message: impl Into<String>) -> Self {
        Self::Limit {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    pub(super) fn unsafe_path(path: &Path, message: impl Into<String>) -> Self {
        Self::UnsafePath {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    pub(super) fn redirect(path: &Path, message: impl Into<String>) -> Self {
        Self::Redirect {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    #[must_use]
    /// Return the stable failure category.
    pub const fn kind(&self) -> ManualErrorKind {
        match self {
            Self::Read { .. } => ManualErrorKind::Read,
            Self::Decompression { .. } => ManualErrorKind::Decompression,
            Self::Limit { .. } => ManualErrorKind::Limit,
            Self::UnsafePath { .. } => ManualErrorKind::UnsafePath,
            Self::Redirect { .. } => ManualErrorKind::Redirect,
            Self::Parse(_) => ManualErrorKind::Parse,
        }
    }

    #[must_use]
    /// Return the original caller-facing source path.
    pub fn path(&self) -> &Path {
        match self {
            Self::Read { path, .. }
            | Self::Decompression { path, .. }
            | Self::Limit { path, .. }
            | Self::UnsafePath { path, .. }
            | Self::Redirect { path, .. } => path,
            Self::Parse(error) => &error.path,
        }
    }

    #[must_use]
    /// Return the stable human-readable detail.
    pub fn message(&self) -> &str {
        match self {
            Self::Read { message, .. }
            | Self::Decompression { message, .. }
            | Self::Limit { message, .. }
            | Self::UnsafePath { message, .. }
            | Self::Redirect { message, .. } => message,
            Self::Parse(error) => &error.message,
        }
    }
}

impl From<ParseError> for ManualError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for ManualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path().display(), self.message())
    }
}

impl std::error::Error for ManualError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Read { .. }
            | Self::Decompression { .. }
            | Self::Limit { .. }
            | Self::UnsafePath { .. }
            | Self::Redirect { .. } => None,
        }
    }
}
