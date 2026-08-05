//! Public parser configuration, input handling, and typed failure boundary.

use std::{
    ffi::CString,
    fmt,
    fs::File,
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use crate::{Diagnostic, Document, RawDocument, diagnostics, ffi};

static PARSER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Policy controlling whether `.so` requests may resolve files.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum IncludePolicy {
    /// Reject `.so` expansion. This is the safe default for arbitrary input.
    #[default]
    Deny,
    /// Resolve `.so` files using the parsed source's manual tree.
    SourceTree,
    /// Resolve `.so` files from one caller-approved directory.
    Root(PathBuf),
}

/// How the parser receives a manual source's top-level compression.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Compression {
    /// Let file parsing use libmandoc's native gzip handling and recognize
    /// zstd frames before sending a staged buffer to libmandoc.
    #[default]
    Auto,
    /// Treat the source bytes as uncompressed roff input.
    Plain,
    /// Decode the source as a zstd frame before parsing it.
    Zstd,
}

/// Configuration for one [`Parser`] instance.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParseOptions {
    pub includes: IncludePolicy,
    pub compression: Compression,
}

/// Completed owned document and any non-fatal parser findings.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseReport {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
}

/// Categorizes a source-level failure without exposing C implementation details.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    InvalidPath,
    Read,
    Decompression,
    Parse,
}

/// File-level failure reported without leaking C or runtime diagnostics.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub path: PathBuf,
    pub kind: ParseErrorKind,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ParseError {}

/// Reusable parser with an explicit input policy.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Parser {
    options: ParseOptions,
}

impl Parser {
    /// Create a parser with the supplied include and compression policies.
    #[must_use]
    pub const fn new(options: ParseOptions) -> Self {
        Self { options }
    }

    /// Return this parser's immutable configuration.
    #[must_use]
    pub const fn options(&self) -> &ParseOptions {
        &self.options
    }

    /// Parse one source path into an owned document.
    ///
    /// Auto-detected file input supports libmandoc's native gzip handling and
    /// zstd files.  `.so` expansion is governed by [`IncludePolicy`].
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the path cannot be represented for C, the
    /// source cannot be read or decoded, or libmandoc rejects the source.
    pub fn parse_file(&self, path: impl AsRef<Path>) -> Result<ParseReport, ParseError> {
        let path = path.as_ref();
        match self.options.compression {
            Compression::Auto if path.extension().is_some_and(|extension| extension == "zst") => {
                self.parse_zstd_file(path)
            }
            Compression::Auto => self.parse_native_file(path),
            Compression::Plain => {
                let source = std::fs::read(path).map_err(|error| read_error(path, &error))?;
                self.parse_plain_bytes(path, &source)
            }
            Compression::Zstd => self.parse_zstd_file(path),
        }
    }

    /// Parse caller-owned source bytes under a logical source path.
    ///
    /// Byte input is useful when a caller owns its transport or decompression
    /// layer.  In auto mode zstd magic is recognized; gzip byte input should
    /// use [`Parser::parse_file`] so libmandoc can open it natively.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the logical path is invalid, the requested
    /// zstd decoding fails, or libmandoc rejects the supplied roff bytes.
    pub fn parse_bytes(
        &self,
        source_path: impl AsRef<Path>,
        source: &[u8],
    ) -> Result<ParseReport, ParseError> {
        let path = source_path.as_ref();
        match self.options.compression {
            Compression::Auto if has_zstd_magic(source) => self.parse_zstd_bytes(path, source),
            Compression::Auto | Compression::Plain => self.parse_plain_bytes(path, source),
            Compression::Zstd => self.parse_zstd_bytes(path, source),
        }
    }

    fn parse_zstd_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        let source = File::open(path)
            .and_then(zstd::stream::decode_all)
            .map_err(|error| decompression_error(path, &error))?;
        self.parse_plain_bytes(path, &source)
    }

    fn parse_zstd_bytes(&self, path: &Path, source: &[u8]) -> Result<ParseReport, ParseError> {
        let source =
            zstd::stream::decode_all(source).map_err(|error| decompression_error(path, &error))?;
        self.parse_plain_bytes(path, &source)
    }

    fn parse_native_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        self.finish(path, |c_path, include_root, allow_includes| {
            ffi::parse_file(c_path, include_root.map(CString::as_c_str), allow_includes)
        })
    }

    fn parse_plain_bytes(&self, path: &Path, source: &[u8]) -> Result<ParseReport, ParseError> {
        self.finish(path, |c_path, include_root, allow_includes| {
            ffi::parse_buffer(
                c_path,
                source,
                include_root.map(CString::as_c_str),
                allow_includes,
            )
        })
    }

    fn finish(
        &self,
        path: &Path,
        parse: impl FnOnce(&CString, Option<&CString>, bool) -> Result<RawDocument, String>,
    ) -> Result<ParseReport, ParseError> {
        let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ParseError {
            path: path.to_path_buf(),
            kind: ParseErrorKind::InvalidPath,
            message: "manual source path contains a NUL byte".into(),
        })?;
        let lock = PARSER_LOCK.get_or_init(|| Mutex::new(()));
        let _guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (include_root, allow_includes) = self.include_root()?;
        let raw = parse(&c_path, include_root.as_ref(), allow_includes).map_err(|message| {
            ParseError {
                path: path.to_path_buf(),
                kind: ParseErrorKind::Parse,
                message,
            }
        })?;
        Ok(ParseReport {
            document: raw.document,
            diagnostics: diagnostics::parse_diagnostics(&raw.diagnostics),
        })
    }

    fn include_root(&self) -> Result<(Option<CString>, bool), ParseError> {
        match &self.options.includes {
            IncludePolicy::Deny => Ok((None, false)),
            IncludePolicy::SourceTree => Ok((None, true)),
            IncludePolicy::Root(root) => CString::new(root.as_os_str().as_bytes())
                .map(Some)
                .map(|root| (root, true))
                .map_err(|_| ParseError {
                    path: root.clone(),
                    kind: ParseErrorKind::InvalidPath,
                    message: "manual include root contains a NUL byte".into(),
                }),
        }
    }
}

fn has_zstd_magic(source: &[u8]) -> bool {
    source.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
}

fn read_error(path: &Path, error: &io::Error) -> ParseError {
    ParseError {
        path: path.to_path_buf(),
        kind: ParseErrorKind::Read,
        message: error.to_string(),
    }
}

fn decompression_error(path: &Path, error: &io::Error) -> ParseError {
    ParseError {
        path: path.to_path_buf(),
        kind: ParseErrorKind::Decompression,
        message: format!("could not decompress zstd manual source: {error}"),
    }
}
