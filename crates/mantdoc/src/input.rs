//! Explicit filesystem and compression adapters for the byte-oriented core.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

#[cfg(any(feature = "gzip", feature = "zstd"))]
use std::io::Cursor;

use crate::{
    ContainedRootResolver, FatalError, FatalErrorKind, ParseReport, Parser, Source, SourceBundle,
    SourceName,
};

/// Transport encoding supplied to a high-level parser input adapter.
///
/// [`Parser::parse`] deliberately accepts only raw caller-owned bytes.  Use
/// [`Parser::parse_bytes`] or [`Parser::parse_file`] when the caller wants the
/// crate to own decompression.  All accepted transport paths impose the same
/// uncompressed root-source limit before scanner allocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Compression {
    /// Detect gzip and zstd frame magic; treat all other bytes as plain roff.
    #[default]
    Auto,
    /// Treat input as uncompressed roff bytes.
    Plain,
    /// Require a gzip member; enabled by the optional `gzip` feature.
    Gzip,
    /// Require a zstd frame; enabled by the optional `zstd` feature.
    Zstd,
}

impl Parser {
    /// Parse caller-owned bytes after an explicit compression transport step.
    ///
    /// A plain input is passed straight to the byte-oriented parser.  An
    /// auto-detected or explicitly selected compressed input is decoded into a
    /// bounded owned buffer first.  `Auto` recognizes gzip (`1f 8b`) and zstd
    /// (`28 b5 2f fd`) frame magic only; filename suffixes are intentionally
    /// irrelevant for in-memory bytes.
    ///
    /// # Errors
    ///
    /// Returns [`FatalError`] for unavailable feature-gated transport support,
    /// unreadable/decode-invalid data, or either the compressed or
    /// uncompressed root-source limit.  Syntax recovery stays in the report.
    pub fn parse_bytes(
        &self,
        name: &SourceName,
        bytes: &[u8],
        compression: Compression,
    ) -> Result<ParseReport, FatalError> {
        let limit = self.config().limits.max_root_source_bytes;
        if bytes.len() > limit {
            return Err(source_limit(name, bytes.len(), limit));
        }
        let bytes = match selected_compression(bytes, compression) {
            Compression::Plain => return self.parse(Source::new(name, bytes)),
            Compression::Gzip => decode_gzip(name, bytes, limit)?,
            Compression::Zstd => decode_zstd(name, bytes, limit)?,
            Compression::Auto => unreachable!("auto transport is resolved before decoding"),
        };
        self.parse(Source::new(name, &bytes))
    }

    /// Read one caller-authorized file and parse it with an explicit transport.
    ///
    /// The `name` is the document's logical identity; `path` is only the
    /// caller-authorized filesystem location.  This adapter does not enable
    /// `.so` filesystem lookup: callers that need includes must use
    /// [`Self::parse_with_resolver`] and an explicit resolver.
    ///
    /// # Errors
    ///
    /// Returns [`FatalError`] with [`FatalErrorKind::Read`] when the file is
    /// unavailable, and otherwise follows [`Self::parse_bytes`].
    pub fn parse_file(
        &self,
        name: &SourceName,
        path: impl AsRef<Path>,
        compression: Compression,
    ) -> Result<ParseReport, FatalError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| read_error(name, path, &error))?;
        let bytes = read_limited(file, name, self.config().limits.max_root_source_bytes)?;
        self.parse_bytes(name, &bytes, compression)
    }

    /// Parse one file and allow `.so` includes only below an explicit root.
    ///
    /// The logical name is derived from the canonical root-relative path of
    /// `path`; callers cannot accidentally bind include resolution to their
    /// process working directory. Included files are plain roff bytes and
    /// individually bounded by `max_root_source_bytes`; only the root uses
    /// the requested compression transport.
    ///
    /// # Errors
    ///
    /// Returns [`FatalErrorKind::Read`] when the root, initial file, or
    /// containment policy cannot be established. A later `.so` miss or
    /// resolver rejection remains a source-addressable recovery diagnostic.
    pub fn parse_file_in_root(
        &self,
        root: impl AsRef<Path>,
        path: impl AsRef<Path>,
        compression: Compression,
    ) -> Result<ParseReport, FatalError> {
        let mut resolver = ContainedRootResolver::new(root, &self.config().limits)
            .map_err(contained_root_error)?;
        let (name, bytes) = resolver.read_root(path).map_err(contained_root_error)?;
        let limit = self.config().limits.max_root_source_bytes;
        let bytes = match selected_compression(&bytes, compression) {
            Compression::Plain => bytes,
            Compression::Gzip => decode_gzip(&name, &bytes, limit)?,
            Compression::Zstd => decode_zstd(&name, &bytes, limit)?,
            Compression::Auto => unreachable!("auto transport is resolved before decoding"),
        };
        self.parse_with_resolver(Source::new(&name, &bytes), &mut resolver)
    }

    /// Parse one uncompressed root from a bounded virtual source bundle.
    ///
    /// The root is copied before parsing because the bundle simultaneously
    /// serves as the mutable explicit `.so` resolver.  Includes resolve only
    /// against normalized bundle paths; no host filesystem path is consulted.
    ///
    /// # Errors
    ///
    /// Returns [`FatalErrorKind::Read`] when `root` is absent and otherwise
    /// follows [`Self::parse_with_resolver`].
    pub fn parse_bundle(
        &self,
        bundle: &mut SourceBundle,
        root: &str,
    ) -> Result<ParseReport, FatalError> {
        let name = SourceName::new(root).map_err(|error| FatalError {
            kind: FatalErrorKind::Read,
            message: format!("invalid source bundle root {root:?}: {error}").into(),
        })?;
        let bytes = bundle
            .get(root)
            .map(ToOwned::to_owned)
            .ok_or_else(|| FatalError {
                kind: FatalErrorKind::Read,
                message: format!("source bundle does not contain root {root:?}").into(),
            })?;
        self.parse_with_resolver(Source::new(&name, &bytes), bundle)
    }
}

fn selected_compression(bytes: &[u8], requested: Compression) -> Compression {
    match requested {
        Compression::Auto if bytes.starts_with(&[0x1f, 0x8b]) => Compression::Gzip,
        Compression::Auto if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) => Compression::Zstd,
        Compression::Auto => Compression::Plain,
        explicit => explicit,
    }
}

fn read_limited(
    mut reader: impl Read,
    name: &SourceName,
    maximum: usize,
) -> Result<Vec<u8>, FatalError> {
    let maximum_plus_one = maximum.checked_add(1).ok_or_else(|| FatalError {
        kind: FatalErrorKind::Invariant,
        message: "root source limit cannot be represented for bounded I/O".into(),
    })?;
    let mut bytes = Vec::with_capacity(maximum_plus_one.min(64 * 1024));
    reader
        .by_ref()
        .take(u64::try_from(maximum_plus_one).expect("usize fits u64 on supported targets"))
        .read_to_end(&mut bytes)
        .map_err(|error| FatalError {
            kind: FatalErrorKind::Read,
            message: format!("{}: {error}", name.as_str()).into(),
        })?;
    if bytes.len() > maximum {
        return Err(source_limit(name, bytes.len(), maximum));
    }
    Ok(bytes)
}

fn source_limit(name: &SourceName, actual: usize, maximum: usize) -> FatalError {
    FatalError {
        kind: FatalErrorKind::SourceLimit,
        message: format!(
            "{}: source has {actual} bytes; configured limit is {maximum}",
            name.as_str()
        )
        .into(),
    }
}

fn read_error(name: &SourceName, path: &Path, error: &io::Error) -> FatalError {
    FatalError {
        kind: FatalErrorKind::Read,
        message: format!("{} ({}) : {error}", name.as_str(), path.display()).into(),
    }
}

fn contained_root_error(error: crate::ResolveError) -> FatalError {
    FatalError {
        kind: FatalErrorKind::Read,
        message: error.message,
    }
}

#[cfg(feature = "gzip")]
fn decode_gzip(name: &SourceName, bytes: &[u8], maximum: usize) -> Result<Vec<u8>, FatalError> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    decode_limited(decoder, name, maximum, "gzip")
}

#[cfg(not(feature = "gzip"))]
fn decode_gzip(_name: &SourceName, _bytes: &[u8], _maximum: usize) -> Result<Vec<u8>, FatalError> {
    Err(unsupported_transport("gzip", "gzip"))
}

#[cfg(feature = "zstd")]
fn decode_zstd(name: &SourceName, bytes: &[u8], maximum: usize) -> Result<Vec<u8>, FatalError> {
    let decoder =
        zstd::stream::read::Decoder::new(Cursor::new(bytes)).map_err(|error| FatalError {
            kind: FatalErrorKind::Decompression,
            message: format!("{}: invalid zstd stream: {error}", name.as_str()).into(),
        })?;
    decode_limited(decoder, name, maximum, "zstd")
}

#[cfg(not(feature = "zstd"))]
fn decode_zstd(_name: &SourceName, _bytes: &[u8], _maximum: usize) -> Result<Vec<u8>, FatalError> {
    Err(unsupported_transport("zstd", "zstd"))
}

#[cfg(any(feature = "gzip", feature = "zstd"))]
fn decode_limited(
    reader: impl Read,
    name: &SourceName,
    maximum: usize,
    transport: &str,
) -> Result<Vec<u8>, FatalError> {
    read_limited(reader, name, maximum).map_err(|error| match error.kind {
        FatalErrorKind::Read => FatalError {
            kind: FatalErrorKind::Decompression,
            message: format!(
                "{}: invalid {transport} stream: {}",
                name.as_str(),
                error.message
            )
            .into(),
        },
        _ => error,
    })
}

#[cfg(any(not(feature = "gzip"), not(feature = "zstd")))]
fn unsupported_transport(transport: &str, feature: &str) -> FatalError {
    FatalError {
        kind: FatalErrorKind::Unsupported,
        message: format!("{transport} input requires mantdoc's `{feature}` feature").into(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::Compression;
    use crate::{DiagnosticCode, FatalErrorKind, Parser, SourceBundle, SourceName};

    #[cfg(feature = "gzip")]
    use crate::{Limits, ParserConfig};

    fn name() -> SourceName {
        SourceName::new("manual.1").unwrap()
    }

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after UNIX epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("mantdoc-{label}-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&path).expect("unique temporary directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn plain_and_auto_bytes_share_the_raw_parser_contract() {
        let parser = Parser::default();
        let name = name();
        let bytes = b".TH INPUT 1\n";
        let plain = parser
            .parse_bytes(&name, bytes, Compression::Plain)
            .unwrap();
        let auto = parser.parse_bytes(&name, bytes, Compression::Auto).unwrap();
        assert_eq!(plain, auto);
    }

    #[test]
    fn missing_file_is_a_typed_read_failure() {
        let parser = Parser::default();
        let error = parser
            .parse_file(
                &name(),
                "/definitely/not/a/mantdoc-input",
                Compression::Plain,
            )
            .unwrap_err();
        assert_eq!(error.kind, FatalErrorKind::Read);
    }

    #[test]
    fn bundle_root_uses_only_the_virtual_resolver() {
        let mut bundle = SourceBundle::default();
        bundle
            .insert("man1/root.1", b".so child.1\n")
            .expect("root source");
        bundle
            .insert("man1/child.1", b".TH BUNDLE 1\n")
            .expect("included source");
        let report = Parser::default()
            .parse_bundle(&mut bundle, "man1/root.1")
            .expect("virtual include must resolve");
        assert_eq!(report.statistics.source_files, 2);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn absent_bundle_root_is_a_typed_read_failure() {
        let error = Parser::default()
            .parse_bundle(&mut SourceBundle::default(), "missing.1")
            .unwrap_err();
        assert_eq!(error.kind, FatalErrorKind::Read);
    }

    #[test]
    fn contained_root_file_adapter_resolves_only_root_relative_includes() {
        let directory = TemporaryDirectory::new("contained-root");
        let manual = directory.path().join("man1");
        fs::create_dir_all(&manual).unwrap();
        let root = manual.join("root.1");
        fs::write(&root, b".so child.1\n").unwrap();
        fs::write(manual.join("child.1"), b".TH CONTAINED 1\n").unwrap();

        let report = Parser::default()
            .parse_file_in_root(directory.path(), &root, Compression::Plain)
            .expect("contained child resolves");
        assert_eq!(report.statistics.source_files, 2);
        assert!(report.diagnostics.is_empty());
        assert_eq!(
            report
                .document
                .source_name(report.document.root_source())
                .map(SourceName::as_str),
            Some("man1/root.1")
        );
    }

    #[cfg(unix)]
    #[test]
    fn contained_root_symlink_escape_is_a_recoverable_resolver_rejection() {
        use std::os::unix::fs::symlink;

        let directory = TemporaryDirectory::new("contained-symlink");
        let manual = directory.path().join("man1");
        fs::create_dir_all(&manual).unwrap();
        let outside = directory.path().join("outside.1");
        fs::write(&outside, b".TH OUTSIDE 1\n").unwrap();
        let root = manual.join("root.1");
        fs::write(&root, b".so escape.1\n").unwrap();
        symlink(&outside, manual.join("escape.1")).unwrap();

        let report = Parser::default()
            .parse_file_in_root(directory.path().join("man1"), &root, Compression::Plain)
            .expect("resolver failure remains a report diagnostic");
        assert_eq!(report.statistics.source_files, 1);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::ROFF_INCLUDE_RESOLVER
        }));
    }

    #[cfg(not(feature = "gzip"))]
    #[test]
    fn auto_gzip_requires_the_opt_in_feature() {
        let error = Parser::default()
            .parse_bytes(&name(), &[0x1f, 0x8b], Compression::Auto)
            .unwrap_err();
        assert_eq!(error.kind, FatalErrorKind::Unsupported);
    }

    #[cfg(not(feature = "zstd"))]
    #[test]
    fn auto_zstd_requires_the_opt_in_feature() {
        let error = Parser::default()
            .parse_bytes(&name(), &[0x28, 0xb5, 0x2f, 0xfd], Compression::Auto)
            .unwrap_err();
        assert_eq!(error.kind, FatalErrorKind::Unsupported);
    }

    #[cfg(feature = "gzip")]
    #[test]
    fn gzip_and_auto_decode_to_the_same_bounded_report() {
        use std::io::Write;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b".TH GZIP 1\n").unwrap();
        let compressed = encoder.finish().unwrap();
        let parser = Parser::default();
        let name = name();
        let explicit = parser
            .parse_bytes(&name, &compressed, Compression::Gzip)
            .unwrap();
        let automatic = parser
            .parse_bytes(&name, &compressed, Compression::Auto)
            .unwrap();
        assert_eq!(explicit, automatic);
    }

    #[cfg(feature = "zstd")]
    #[test]
    fn zstd_and_auto_decode_to_the_same_bounded_report() {
        let compressed = zstd::stream::encode_all(&b".TH ZSTD 1\n"[..], 0).unwrap();
        let parser = Parser::default();
        let name = name();
        let explicit = parser
            .parse_bytes(&name, &compressed, Compression::Zstd)
            .unwrap();
        let automatic = parser
            .parse_bytes(&name, &compressed, Compression::Auto)
            .unwrap();
        assert_eq!(explicit, automatic);
    }

    #[cfg(feature = "gzip")]
    #[test]
    fn decompression_cannot_bypass_the_root_input_limit() {
        use std::io::Write;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b".TH TOO-LONG 1\n").unwrap();
        let compressed = encoder.finish().unwrap();
        let parser = Parser::new(ParserConfig {
            limits: Limits {
                max_root_source_bytes: 8,
                max_total_source_bytes: 8,
                ..Limits::default()
            },
            ..ParserConfig::default()
        });
        let error = parser
            .parse_bytes(&name(), &compressed, Compression::Gzip)
            .unwrap_err();
        assert_eq!(error.kind, FatalErrorKind::SourceLimit);
    }
}
