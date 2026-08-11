//! Product-level failures while loading and parsing one native manual.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use libmandoc_rs::ParseError;

/// Stable category for a native manual failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualErrorKind {
    Read,
    Decompression,
    Limit,
    UnsafePath,
    Redirect,
    Parse,
}

/// Failure produced by `ManT`'s source policy or by the underlying roff parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManualError {
    Read { path: PathBuf, message: String },
    Decompression { path: PathBuf, message: String },
    Limit { path: PathBuf, message: String },
    UnsafePath { path: PathBuf, message: String },
    Redirect { path: PathBuf, message: String },
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
