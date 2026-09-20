//! Loader-level failures while preparing and parsing one native manual.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use libmandoc_rs::{ExecutionError, ExecutionErrorKind};
use mant_codec::{RoffError, RoffProjectionError};

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
    /// A successful native execution could not be projected into stable IR.
    Projection,
}

/// Failure produced by `ManT`'s source policy or complete roff-to-IR boundary.
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
    /// Failure reported by the atomic native execution boundary.
    Native(ExecutionError),
    /// Failure while projecting a successful native report into stable IR.
    Projection(RoffProjectionError),
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
            Self::Native(_) => ManualErrorKind::Parse,
            Self::Projection(_) => ManualErrorKind::Projection,
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
            Self::Native(error) => &error.path,
            Self::Projection(error) => error.path(),
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
            Self::Native(error) => &error.message,
            Self::Projection(error) => error.message(),
        }
    }
}

impl From<ExecutionError> for ManualError {
    fn from(error: ExecutionError) -> Self {
        if error.kind == ExecutionErrorKind::Budget {
            return Self::Limit {
                path: error.path,
                message: error.message,
            };
        }
        Self::Native(error)
    }
}

impl From<RoffError> for ManualError {
    fn from(error: RoffError) -> Self {
        match error {
            RoffError::Native(error) => Self::from(error),
            RoffError::Projection(error) => Self::Projection(error),
        }
    }
}

impl fmt::Display for ManualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(error) => fmt::Display::fmt(error, formatter),
            Self::Projection(error) => fmt::Display::fmt(error, formatter),
            Self::Read { .. }
            | Self::Decompression { .. }
            | Self::Limit { .. }
            | Self::UnsafePath { .. }
            | Self::Redirect { .. } => {
                write!(formatter, "{}: {}", self.path().display(), self.message())
            }
        }
    }
}

impl std::error::Error for ManualError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Native(error) => Some(error),
            Self::Projection(error) => Some(error),
            Self::Read { .. }
            | Self::Decompression { .. }
            | Self::Limit { .. }
            | Self::UnsafePath { .. }
            | Self::Redirect { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_codec::RoffProjectionStage;

    #[test]
    fn maps_native_budget_to_loader_limit() {
        let error = ManualError::from(RoffError::Native(ExecutionError {
            path: PathBuf::from("probe.1"),
            kind: ExecutionErrorKind::Budget,
            message: "budget exhausted".to_owned(),
        }));

        assert_eq!(error.kind(), ManualErrorKind::Limit);
        assert_eq!(error.path(), Path::new("probe.1"));
        assert!(std::error::Error::source(&error).is_none());
    }

    #[test]
    fn preserves_projection_as_a_distinct_loader_failure() {
        let error = ManualError::from(RoffError::Projection(RoffProjectionError::new(
            "probe.1",
            RoffProjectionStage::Materialization,
            "definition owner was not materialized",
        )));

        assert_eq!(error.kind(), ManualErrorKind::Projection);
        assert_eq!(error.path(), Path::new("probe.1"));
        assert_eq!(error.message(), "definition owner was not materialized");
        assert!(error.to_string().contains("roff materialization failed"));
        assert!(std::error::Error::source(&error).is_some());
    }
}
