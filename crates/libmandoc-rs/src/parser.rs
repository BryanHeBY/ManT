//! Public parser configuration, input handling, and typed failure boundary.

use std::{
    ffi::CString,
    fmt,
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(windows)]
use std::io::Read;

#[cfg(windows)]
use flate2::read::MultiGzDecoder;

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
    /// Resolve `.so` files below one caller-approved directory without
    /// traversing symbolic links beneath that root.
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
    /// Policy for resolving roff `.so` include requests.
    pub includes: IncludePolicy,
    /// Compression expected at the outermost source boundary.
    pub compression: Compression,
}

/// Completed owned document and any non-fatal parser findings.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseReport {
    /// Fully owned syntax tree and metadata.
    pub document: Document,
    /// Non-fatal findings emitted while validating the source.
    pub diagnostics: Vec<Diagnostic>,
}

/// Categorizes a source-level failure without exposing C implementation details.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// A path cannot be represented safely at the native API boundary.
    InvalidPath,
    /// Source bytes could not be read.
    Read,
    /// Compressed source bytes could not be decoded.
    Decompression,
    /// The selected parsing policy is unavailable on this platform.
    Unsupported,
    /// libmandoc rejected the source or failed to produce a document.
    Parse,
}

/// File-level failure reported without leaking C or runtime diagnostics.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    /// Source path associated with the failure.
    pub path: PathBuf,
    /// Stable category suitable for programmatic handling.
    pub kind: ParseErrorKind,
    /// Human-readable detail without unstable native diagnostic structure.
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
            Compression::Auto => self.parse_auto_file(path),
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

    #[cfg(unix)]
    fn parse_auto_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        self.parse_native_file(path)
    }

    #[cfg(windows)]
    fn parse_auto_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        let source = File::open(path).map_err(|error| read_error(path, &error))?;
        if path.extension().is_some_and(|extension| extension == "gz") {
            let mut decoded = Vec::new();
            MultiGzDecoder::new(source)
                .read_to_end(&mut decoded)
                .map_err(|error| gzip_decompression_error(path, &error))?;
            self.parse_plain_bytes(path, &decoded)
        } else {
            let mut source = source;
            let mut bytes = Vec::new();
            source
                .read_to_end(&mut bytes)
                .map_err(|error| read_error(path, &error))?;
            self.parse_plain_bytes(path, &bytes)
        }
    }

    fn parse_zstd_bytes(&self, path: &Path, source: &[u8]) -> Result<ParseReport, ParseError> {
        let source =
            zstd::stream::decode_all(source).map_err(|error| decompression_error(path, &error))?;
        self.parse_plain_bytes(path, &source)
    }

    #[cfg(unix)]
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
        let c_path = path_label(path).map_err(|_| ParseError {
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
            #[cfg(unix)]
            IncludePolicy::SourceTree => Ok((None, true)),
            #[cfg(windows)]
            IncludePolicy::SourceTree => Err(unsupported_includes(PathBuf::new())),
            IncludePolicy::Root(root) if root.as_os_str().is_empty() => Err(ParseError {
                path: root.clone(),
                kind: ParseErrorKind::InvalidPath,
                message: "manual include root is empty".into(),
            }),
            #[cfg(unix)]
            IncludePolicy::Root(root) => CString::new(root.as_os_str().as_bytes())
                .map(Some)
                .map(|root| (root, true))
                .map_err(|_| ParseError {
                    path: root.clone(),
                    kind: ParseErrorKind::InvalidPath,
                    message: "manual include root contains a NUL byte".into(),
                }),
            #[cfg(windows)]
            IncludePolicy::Root(root) => Err(unsupported_includes(root.clone())),
        }
    }
}

#[cfg(unix)]
fn path_label(path: &Path) -> Result<CString, std::ffi::NulError> {
    CString::new(path.as_os_str().as_bytes())
}

#[cfg(windows)]
fn path_label(path: &Path) -> Result<CString, std::ffi::NulError> {
    CString::new(path.to_string_lossy().as_bytes())
}

#[cfg(windows)]
fn unsupported_includes(path: PathBuf) -> ParseError {
    ParseError {
        path,
        kind: ParseErrorKind::Unsupported,
        message:
            "libmandoc file inclusion is unavailable on Windows; resolve .so sources before parsing"
                .into(),
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

#[cfg(windows)]
fn gzip_decompression_error(path: &Path, error: &io::Error) -> ParseError {
    ParseError {
        path: path.to_path_buf(),
        kind: ParseErrorKind::Decompression,
        message: format!("could not decompress gzip manual source: {error}"),
    }
}
