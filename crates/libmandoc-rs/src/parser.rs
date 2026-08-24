//! Public parser configuration, input handling, and typed failure boundary.

use std::{
    ffi::{CStr, CString},
    fmt,
    fs::File,
    io,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(windows)]
use std::io::Read;

use crate::{
    Diagnostic, DiagnosticCode, DiagnosticLevel, Document, RawDocument, SourceBundle, compression,
    diagnostics, ffi,
};

/// Selects the macro language before parsing begins.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputFormat {
    /// Detect mdoc from `.Dd`, man from `.TH`, and otherwise use man.
    #[default]
    Auto,
    /// Parse the source as man regardless of its first macro.
    Man,
    /// Parse the source as mdoc regardless of its first macro.
    Mdoc,
}

/// Policy controlling whether `.so` requests may resolve files.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum IncludePolicy {
    /// Reject `.so` expansion. This is the safe default for arbitrary input.
    #[default]
    Deny,
    /// Resolve `.so` files using Unix libmandoc-compatible source-tree and
    /// process-working-directory lookup.
    ///
    /// This compatibility policy is for trusted manual trees, not strict
    /// containment, and is unavailable on Windows.
    SourceTree,
    /// Resolve `.so` files below one caller-approved directory without
    /// traversing symbolic links beneath that root or falling back elsewhere.
    ///
    /// The approved root itself may be a symbolic link. On Windows, source
    /// files are read by the Rust boundary and passed to memory-only
    /// libmandoc; Unix retains its descriptor-relative native reader.
    Root(PathBuf),
}

/// How the parser receives a manual source's top-level compression.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Compression {
    /// Detect supported compression at the relevant input boundary.
    ///
    /// File input uses a `.zst` suffix for zstd. Windows additionally uses a
    /// `.gz` suffix for gzip; other Unix file input goes through libmandoc's
    /// native reader. Byte input recognizes zstd magic but not gzip.
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
///
/// Independent calls may run concurrently. The bundled C parser keeps its
/// mutable session state in static thread-local storage; this type does not
/// support recursive re-entry on one OS thread. Owned node and equation copies
/// stop after 256 levels, omit deeper descendants from pathological input, and
/// report either truncation through [`ParseReport::diagnostics`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Parser {
    options: ParseOptions,
    input_format: InputFormat,
    mdoc_operating_system: Option<CString>,
}

impl Parser {
    /// Create a parser with the supplied include and compression policies.
    #[must_use]
    pub const fn new(options: ParseOptions) -> Self {
        Self {
            options,
            input_format: InputFormat::Auto,
            mdoc_operating_system: None,
        }
    }

    /// Return this parser's immutable configuration.
    #[must_use]
    pub const fn options(&self) -> &ParseOptions {
        &self.options
    }

    /// Select the input macro language without changing the existing options shape.
    #[must_use]
    pub const fn with_input_format(mut self, input_format: InputFormat) -> Self {
        self.input_format = input_format;
        self
    }

    /// Return this parser's macro-language selection.
    #[must_use]
    pub const fn input_format(&self) -> InputFormat {
        self.input_format
    }

    /// Override the operating-system name used by an argument-less mdoc
    /// `.Os` macro.
    ///
    /// Without an override, libmandoc retains its native behavior: Unix uses
    /// `uname(3)` and Windows uses its target configuration. An explicit `.Os
    /// name` in the document still takes precedence over this value.
    ///
    /// # Errors
    ///
    /// Returns [`std::ffi::NulError`] when the supplied name contains a NUL
    /// byte and therefore cannot cross the native boundary.
    pub fn with_mdoc_operating_system(
        mut self,
        operating_system: impl AsRef<str>,
    ) -> Result<Self, std::ffi::NulError> {
        self.mdoc_operating_system = Some(CString::new(operating_system.as_ref())?);
        Ok(self)
    }

    /// Return the caller-selected operating-system override for bare `.Os`.
    #[must_use]
    pub fn mdoc_operating_system(&self) -> Option<&CStr> {
        self.mdoc_operating_system.as_deref()
    }

    /// Parse one source path into an owned document.
    ///
    /// Auto-detected file input selects Rust zstd decoding for `.zst`; Windows
    /// also selects Rust gzip decoding for `.gz`, while other Unix paths use
    /// libmandoc's native reader. `.so` expansion is governed by
    /// [`IncludePolicy`].
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
    /// layer. In auto mode zstd magic is recognized. Callers must decompress
    /// gzip byte input themselves, or pass a gzip file to
    /// [`Parser::parse_file`].
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

    /// Parse one root from a bounded, read-only virtual source tree.
    ///
    /// Bundle entries are uncompressed source bytes. `.so` requests resolve
    /// first as exact bundle paths, then beside the including source, and
    /// never fall back to the host filesystem. This boundary behaves the same
    /// way on Unix and Windows.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the root is absent, its path cannot cross
    /// the C boundary, or libmandoc rejects the root or an included source.
    pub fn parse_bundle(
        &self,
        root: impl AsRef<Path>,
        bundle: &SourceBundle,
    ) -> Result<ParseReport, ParseError> {
        let root = root.as_ref();
        let root_label = root.to_str().ok_or_else(|| ParseError {
            path: root.to_path_buf(),
            kind: ParseErrorKind::InvalidPath,
            message: "source bundle roots must be UTF-8 logical paths".into(),
        })?;
        if bundle.get(root_label).is_none() {
            return Err(ParseError {
                path: root.to_path_buf(),
                kind: ParseErrorKind::Read,
                message: "source bundle does not contain the requested root".into(),
            });
        }
        self.finish(root, |c_path, _| {
            ffi::parse_bundle(
                c_path,
                bundle,
                self.input_format,
                self.mdoc_operating_system(),
            )
        })
    }

    fn parse_zstd_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        let source = File::open(path)
            .and_then(compression::decode_zstd)
            .map_err(|error| decompression_error(path, &error))?;
        self.parse_plain_bytes(path, &source)
    }

    #[cfg(unix)]
    fn parse_auto_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        self.parse_native_file(path)
    }

    #[cfg(windows)]
    fn parse_auto_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        let (source, gzip) =
            compression::open_auto_file(path).map_err(|error| read_error(path, &error))?;
        if gzip {
            let decoded = compression::decode_gzip(source)
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
            compression::decode_zstd(source).map_err(|error| decompression_error(path, &error))?;
        self.parse_plain_bytes(path, &source)
    }

    #[cfg(unix)]
    fn parse_native_file(&self, path: &Path) -> Result<ParseReport, ParseError> {
        self.finish(path, |c_path, includes| {
            ffi::parse_file(
                c_path,
                includes.root.as_deref(),
                includes.allow_includes,
                self.input_format,
                self.mdoc_operating_system(),
            )
        })
    }

    #[cfg(unix)]
    fn parse_plain_bytes(&self, path: &Path, source: &[u8]) -> Result<ParseReport, ParseError> {
        self.finish(path, |c_path, includes| {
            ffi::parse_buffer(
                c_path,
                source,
                includes.root.as_deref(),
                includes.allow_includes,
                self.input_format,
                self.mdoc_operating_system(),
            )
        })
    }

    #[cfg(windows)]
    fn parse_plain_bytes(&self, path: &Path, source: &[u8]) -> Result<ParseReport, ParseError> {
        self.finish(path, |c_path, includes| {
            ffi::parse_buffer(
                c_path,
                source,
                includes.root.as_deref(),
                includes.allow_includes,
                self.input_format,
                self.mdoc_operating_system(),
            )
        })
    }

    fn finish(
        &self,
        path: &Path,
        parse: impl FnOnce(&CString, &IncludeSettings) -> Result<RawDocument, String>,
    ) -> Result<ParseReport, ParseError> {
        let c_path = path_label(path).map_err(|_| ParseError {
            path: path.to_path_buf(),
            kind: ParseErrorKind::InvalidPath,
            message: "manual source path contains a NUL byte".into(),
        })?;
        let include_settings = self.include_settings(path)?;
        let raw = parse(&c_path, &include_settings).map_err(|message| ParseError {
            path: path.to_path_buf(),
            kind: ParseErrorKind::Parse,
            message,
        })?;
        let mut findings = diagnostics::parse_diagnostics(&raw.diagnostics);
        if raw.node_truncated {
            findings.push(Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some(DiagnosticCode::SyntaxTreeDepthLimit),
                message: "owned syntax tree exceeded the 256-level copy limit; deeper descendants were omitted"
                    .into(),
                location: None,
            });
        }
        if raw.equation_truncated {
            findings.push(Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some(DiagnosticCode::EquationTreeDepthLimit),
                message: "equation tree exceeded the 256-level copy limit; deeper equation content was omitted"
                    .into(),
                location: None,
            });
        }
        Ok(ParseReport {
            document: raw.document,
            diagnostics: findings,
        })
    }

    pub(crate) fn include_settings(
        &self,
        source_path: &Path,
    ) -> Result<IncludeSettings, ParseError> {
        #[cfg(unix)]
        let _ = source_path;

        match &self.options.includes {
            IncludePolicy::Deny => Ok(IncludeSettings {
                root: None,
                allow_includes: false,
            }),
            #[cfg(unix)]
            IncludePolicy::SourceTree => Ok(IncludeSettings {
                root: None,
                allow_includes: true,
            }),
            #[cfg(windows)]
            IncludePolicy::SourceTree => Err(unsupported_includes(source_path.to_path_buf())),
            IncludePolicy::Root(root) if root.as_os_str().is_empty() => Err(ParseError {
                path: root.clone(),
                kind: ParseErrorKind::InvalidPath,
                message: "manual include root is empty".into(),
            }),
            #[cfg(unix)]
            IncludePolicy::Root(root) => CString::new(root.as_os_str().as_bytes())
                .map(|root| IncludeSettings {
                    root: Some(root),
                    allow_includes: true,
                })
                .map_err(|_| ParseError {
                    path: root.clone(),
                    kind: ParseErrorKind::InvalidPath,
                    message: "manual include root contains a NUL byte".into(),
                }),
            #[cfg(windows)]
            IncludePolicy::Root(root) => Ok(IncludeSettings {
                root: Some(root.clone()),
                allow_includes: true,
            }),
        }
    }
}

pub(crate) struct IncludeSettings {
    #[cfg(unix)]
    pub(crate) root: Option<CString>,
    #[cfg(windows)]
    pub(crate) root: Option<PathBuf>,
    pub(crate) allow_includes: bool,
}

#[cfg(unix)]
pub(crate) fn path_label(path: &Path) -> Result<CString, std::ffi::NulError> {
    CString::new(path.as_os_str().as_bytes())
}

#[cfg(windows)]
pub(crate) fn path_label(path: &Path) -> Result<CString, std::ffi::NulError> {
    CString::new(path.to_string_lossy().as_bytes())
}

#[cfg(windows)]
fn unsupported_includes(path: PathBuf) -> ParseError {
    ParseError {
        path,
        kind: ParseErrorKind::Unsupported,
        message: "libmandoc-compatible source-tree inclusion is unavailable on Windows; use IncludePolicy::Root or SourceBundle"
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
