//! Optional bounded wrappers around libmandoc's reference renderers.

use std::{
    ffi::CString,
    fmt,
    fs::File,
    io,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use std::io::Read;

#[cfg(windows)]
use flate2::read::MultiGzDecoder;

use crate::{
    Compression, Diagnostic, ParseError, ParseErrorKind, Parser, RawRender, SourceBundle,
    diagnostics, ffi, parser::IncludeSettings,
};

/// Default maximum bytes retained for one render call.
pub const DEFAULT_RENDER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// Default terminal width used by ASCII and UTF-8 output.
pub const DEFAULT_RENDER_WIDTH: usize = 78;

/// Hard maximum accepted output budget for one render call.
pub const MAX_RENDER_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// Smallest supported terminal width.
pub const MIN_RENDER_WIDTH: usize = 20;

/// Largest supported terminal width.
pub const MAX_RENDER_WIDTH: usize = 1_000;

/// Reference output format produced by [`Renderer`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderFormat {
    /// Portable 7-bit terminal text using backspace overstrikes for styling.
    Ascii,
    /// Deterministic UTF-8 terminal text with Unicode cell widths.
    Utf8,
    /// UTF-8 HTML, either a full document or a caller-selected fragment.
    Html,
}

/// Successful bounded reference rendering and non-fatal parser findings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderReport {
    /// Complete rendered output. Limit failures never return a partial value.
    pub output: String,
    /// Non-fatal findings emitted while parsing the source.
    pub diagnostics: Vec<Diagnostic>,
}

/// Stable category for a failed render call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderErrorKind {
    /// A source path cannot cross the native boundary.
    InvalidPath,
    /// Source bytes could not be read.
    Read,
    /// Compressed source bytes could not be decoded.
    Decompression,
    /// The parser configuration is unavailable on this platform.
    Unsupported,
    /// A width or output limit is outside the documented range.
    InvalidOptions,
    /// The complete output exceeds the configured byte budget.
    OutputLimit,
    /// libmandoc could not parse or render the source.
    Render,
    /// The native renderer returned invalid UTF-8.
    Encoding,
}

/// Failure from an optional reference renderer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderError {
    /// Logical or physical source associated with the failure.
    pub path: PathBuf,
    /// Stable category suitable for programmatic handling.
    pub kind: RenderErrorKind,
    /// Human-readable detail.
    pub message: String,
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for RenderError {}

/// Bounded, thread-safe access to libmandoc's ASCII, UTF-8, and HTML renderers.
///
/// Rendering reparses the source and formats the native tree in one call;
/// it does not reconstruct private libmandoc state from the owned Rust AST.
/// Output is captured by a per-thread native sink and never uses process
/// standard output. Recursive re-entry on one OS thread remains unsupported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Renderer {
    parser: Parser,
    format: RenderFormat,
    width: usize,
    max_output_bytes: usize,
    html_fragment: bool,
}

impl Renderer {
    /// Create a renderer with the default parser, width 78, and an 8 MiB cap.
    #[must_use]
    pub fn new(format: RenderFormat) -> Self {
        Self {
            parser: Parser::default(),
            format,
            width: DEFAULT_RENDER_WIDTH,
            max_output_bytes: DEFAULT_RENDER_OUTPUT_BYTES,
            html_fragment: false,
        }
    }

    /// Use an explicit parser policy and input-format selection.
    #[must_use]
    pub fn with_parser(mut self, parser: Parser) -> Self {
        self.parser = parser;
        self
    }

    /// Set the terminal width used by ASCII and UTF-8 output.
    #[must_use]
    pub const fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    /// Set the maximum complete output size retained by one call.
    #[must_use]
    pub const fn with_max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    /// Select an HTML fragment instead of a complete HTML document.
    #[must_use]
    pub const fn with_html_fragment(mut self, html_fragment: bool) -> Self {
        self.html_fragment = html_fragment;
        self
    }

    /// Return the parser configuration used by this renderer.
    #[must_use]
    pub const fn parser(&self) -> &Parser {
        &self.parser
    }

    /// Return the selected output format.
    #[must_use]
    pub const fn format(&self) -> RenderFormat {
        self.format
    }

    /// Return the configured terminal width.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Return the maximum complete output size retained by one call.
    #[must_use]
    pub const fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }

    /// Return whether HTML output is configured as a fragment.
    #[must_use]
    pub const fn html_fragment(&self) -> bool {
        self.html_fragment
    }

    /// Render one source path.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for invalid options or paths, transport and
    /// decompression failures, parser/renderer failures, or output overflow.
    pub fn render_file(&self, path: impl AsRef<Path>) -> Result<RenderReport, RenderError> {
        let path = path.as_ref();
        self.validate(path)?;
        match self.parser.options().compression {
            Compression::Auto if path.extension().is_some_and(|extension| extension == "zst") => {
                self.render_zstd_file(path)
            }
            Compression::Auto => self.render_auto_file(path),
            Compression::Plain => {
                let source = std::fs::read(path).map_err(|error| read_error(path, &error))?;
                self.render_plain_bytes(path, &source)
            }
            Compression::Zstd => self.render_zstd_file(path),
        }
    }

    /// Render caller-owned source bytes under a logical path.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] under the same conditions as
    /// [`Renderer::render_file`].
    pub fn render_bytes(
        &self,
        source_path: impl AsRef<Path>,
        source: &[u8],
    ) -> Result<RenderReport, RenderError> {
        let path = source_path.as_ref();
        self.validate(path)?;
        match self.parser.options().compression {
            Compression::Auto if has_zstd_magic(source) => self.render_zstd_bytes(path, source),
            Compression::Auto | Compression::Plain => self.render_plain_bytes(path, source),
            Compression::Zstd => self.render_zstd_bytes(path, source),
        }
    }

    /// Render one root from a bounded virtual source tree.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] if the root is absent or invalid, options are
    /// invalid, parsing/rendering fails, or output exceeds the byte budget.
    pub fn render_bundle(
        &self,
        root: impl AsRef<Path>,
        bundle: &SourceBundle,
    ) -> Result<RenderReport, RenderError> {
        let root = root.as_ref();
        self.validate(root)?;
        let root_label = root.to_str().ok_or_else(|| RenderError {
            path: root.to_path_buf(),
            kind: RenderErrorKind::InvalidPath,
            message: "source bundle roots must be UTF-8 logical paths".into(),
        })?;
        if bundle.get(root_label).is_none() {
            return Err(RenderError {
                path: root.to_path_buf(),
                kind: RenderErrorKind::Read,
                message: "source bundle does not contain the requested root".into(),
            });
        }
        let c_path = native_path(root)?;
        Self::finish(
            root,
            ffi::render_bundle(
                &c_path,
                bundle,
                self.parser.input_format(),
                format_code(self.format),
                self.width,
                self.html_fragment,
                self.max_output_bytes,
            ),
        )
    }

    #[cfg(unix)]
    fn render_auto_file(&self, path: &Path) -> Result<RenderReport, RenderError> {
        let c_path = native_path(path)?;
        let includes = self.include_settings()?;
        Self::finish(
            path,
            ffi::render_file(
                &c_path,
                includes.root.as_deref(),
                includes.allow_includes,
                self.parser.input_format(),
                format_code(self.format),
                self.width,
                self.html_fragment,
                self.max_output_bytes,
            ),
        )
    }

    #[cfg(windows)]
    fn render_auto_file(&self, path: &Path) -> Result<RenderReport, RenderError> {
        let source = File::open(path).map_err(|error| read_error(path, &error))?;
        if path.extension().is_some_and(|extension| extension == "gz") {
            let mut decoded = Vec::new();
            MultiGzDecoder::new(source)
                .read_to_end(&mut decoded)
                .map_err(|error| RenderError {
                    path: path.to_path_buf(),
                    kind: RenderErrorKind::Decompression,
                    message: format!("could not decompress gzip manual source: {error}"),
                })?;
            self.render_plain_bytes(path, &decoded)
        } else {
            let mut source = source;
            let mut bytes = Vec::new();
            source
                .read_to_end(&mut bytes)
                .map_err(|error| read_error(path, &error))?;
            self.render_plain_bytes(path, &bytes)
        }
    }

    fn render_zstd_file(&self, path: &Path) -> Result<RenderReport, RenderError> {
        let source = File::open(path)
            .and_then(zstd::stream::decode_all)
            .map_err(|error| decompression_error(path, &error))?;
        self.render_plain_bytes(path, &source)
    }

    fn render_zstd_bytes(&self, path: &Path, source: &[u8]) -> Result<RenderReport, RenderError> {
        let source =
            zstd::stream::decode_all(source).map_err(|error| decompression_error(path, &error))?;
        self.render_plain_bytes(path, &source)
    }

    #[cfg(unix)]
    fn render_plain_bytes(&self, path: &Path, source: &[u8]) -> Result<RenderReport, RenderError> {
        let c_path = native_path(path)?;
        let includes = self.include_settings()?;
        Self::finish(
            path,
            ffi::render_buffer(
                &c_path,
                source,
                includes.root.as_deref(),
                includes.allow_includes,
                self.parser.input_format(),
                format_code(self.format),
                self.width,
                self.html_fragment,
                self.max_output_bytes,
            ),
        )
    }

    #[cfg(windows)]
    fn render_plain_bytes(&self, path: &Path, source: &[u8]) -> Result<RenderReport, RenderError> {
        let c_path = native_path(path)?;
        let includes = self.include_settings()?;
        Self::finish(
            path,
            ffi::render_buffer(
                &c_path,
                source,
                includes.root.as_deref(),
                includes.allow_includes,
                self.parser.input_format(),
                format_code(self.format),
                self.width,
                self.html_fragment,
                self.max_output_bytes,
            ),
        )
    }

    fn finish(
        path: &Path,
        rendered: Result<RawRender, ffi::NativeRenderError>,
    ) -> Result<RenderReport, RenderError> {
        let raw = rendered.map_err(|error| RenderError {
            path: path.to_path_buf(),
            kind: if error.status == 1 {
                RenderErrorKind::OutputLimit
            } else {
                RenderErrorKind::Render
            },
            message: error.message,
        })?;
        let output = String::from_utf8(raw.output).map_err(|error| RenderError {
            path: path.to_path_buf(),
            kind: RenderErrorKind::Encoding,
            message: format!("native renderer returned invalid UTF-8: {error}"),
        })?;
        Ok(RenderReport {
            output,
            diagnostics: diagnostics::parse_diagnostics(&raw.diagnostics),
        })
    }

    fn validate(&self, path: &Path) -> Result<(), RenderError> {
        if self.format != RenderFormat::Html
            && !(MIN_RENDER_WIDTH..=MAX_RENDER_WIDTH).contains(&self.width)
        {
            return Err(RenderError {
                path: path.to_path_buf(),
                kind: RenderErrorKind::InvalidOptions,
                message: format!(
                    "render width must be between {MIN_RENDER_WIDTH} and {MAX_RENDER_WIDTH}"
                ),
            });
        }
        if !(1..=MAX_RENDER_OUTPUT_BYTES).contains(&self.max_output_bytes) {
            return Err(RenderError {
                path: path.to_path_buf(),
                kind: RenderErrorKind::InvalidOptions,
                message: format!(
                    "render output limit must be between 1 and {MAX_RENDER_OUTPUT_BYTES} bytes"
                ),
            });
        }
        Ok(())
    }

    fn include_settings(&self) -> Result<IncludeSettings, RenderError> {
        self.parser.include_settings().map_err(map_parse_error)
    }
}

fn native_path(path: &Path) -> Result<CString, RenderError> {
    crate::parser::path_label(path).map_err(|_| RenderError {
        path: path.to_path_buf(),
        kind: RenderErrorKind::InvalidPath,
        message: "manual source path contains a NUL byte".into(),
    })
}

fn map_parse_error(error: ParseError) -> RenderError {
    RenderError {
        path: error.path,
        kind: match error.kind {
            ParseErrorKind::InvalidPath => RenderErrorKind::InvalidPath,
            ParseErrorKind::Read => RenderErrorKind::Read,
            ParseErrorKind::Decompression => RenderErrorKind::Decompression,
            ParseErrorKind::Unsupported => RenderErrorKind::Unsupported,
            ParseErrorKind::Parse => RenderErrorKind::Render,
        },
        message: error.message,
    }
}

const fn format_code(format: RenderFormat) -> i32 {
    match format {
        RenderFormat::Ascii => 1,
        RenderFormat::Html => 2,
        RenderFormat::Utf8 => 3,
    }
}

fn has_zstd_magic(source: &[u8]) -> bool {
    source.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
}

fn read_error(path: &Path, error: &io::Error) -> RenderError {
    RenderError {
        path: path.to_path_buf(),
        kind: RenderErrorKind::Read,
        message: error.to_string(),
    }
}

fn decompression_error(path: &Path, error: &io::Error) -> RenderError {
    RenderError {
        path: path.to_path_buf(),
        kind: RenderErrorKind::Decompression,
        message: format!("could not decompress zstd manual source: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use crate::{RenderFormat, Renderer, ffi};

    #[test]
    fn utf8_rendering_does_not_change_process_ctype_locale() {
        let before = ffi::ctype_locale();
        let report = Renderer::new(RenderFormat::Utf8)
            .render_bytes(
                "locale.1",
                ".TH LOCALE 1\n.SH NAME\ncafé \\(em 日本 😀\n".as_bytes(),
            )
            .expect("render deterministic UTF-8");
        let after = ffi::ctype_locale();

        assert_eq!(after, before);
        assert!(report.output.contains("café — 日本 😀"));
    }
}
