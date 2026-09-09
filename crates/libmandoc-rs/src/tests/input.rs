//! Byte and file transports, compression and encoding contracts.

use super::*;
#[cfg(windows)]
use std::io::Write;

#[test]
fn parser_decompresses_zstd_sources_before_calling_libmandoc() {
    let path = source_path("zstd-mandoc-session").with_extension("1.zst");
    let source = b".TH ZSTD-MANT 1 \"2026-07-20\"\n.SH NAME\nzstd-mant \\- compressed manual\n";
    let compressed = zstd::stream::encode_all(source.as_slice(), 1).expect("compress source");
    fs::write(&path, compressed).expect("write compressed manual source");

    let report = Parser::default()
        .parse_file(&path)
        .expect("parse zstd manual");
    fs::remove_file(path).expect("remove compressed manual source");

    assert!(report.diagnostics.is_empty());
    let document = report.document;
    assert_eq!(document.macro_set, MacroSet::Man);
    assert_eq!(document.metadata.title.as_deref(), Some("ZSTD-MANT"));
    assert_eq!(document.metadata.section.as_deref(), Some("1"));
    assert!(document.metadata.has_body);
}

#[cfg(windows)]
#[test]
fn windows_parser_decompresses_gzip_before_calling_libmandoc() {
    use flate2::{Compression as GzipCompression, write::GzEncoder};

    let path = source_path("gzip-mandoc-session").with_extension("1.gz");
    let mut encoder = GzEncoder::new(Vec::new(), GzipCompression::fast());
    encoder
        .write_all(b".TH GZIP-MANT 1\n.SH NAME\ngzip-mant \\- compressed manual\n")
        .expect("encode gzip source");
    fs::write(&path, encoder.finish().expect("finish gzip source")).expect("write gzip source");

    let report = Parser::default()
        .parse_file(&path)
        .expect("parse gzip manual");
    fs::remove_file(path).expect("remove gzip source");

    assert_eq!(report.document.metadata.title.as_deref(), Some("GZIP-MANT"));
}

#[cfg(windows)]
#[test]
fn windows_parser_uses_native_gzip_fallback_for_top_level_files() {
    use flate2::{Compression as GzipCompression, write::GzEncoder};

    let requested = source_path("gzip-fallback-session");
    let mut compressed_path = requested.as_os_str().to_os_string();
    compressed_path.push(".gz");
    let compressed_path = std::path::PathBuf::from(compressed_path);
    let mut encoder = GzEncoder::new(Vec::new(), GzipCompression::fast());
    encoder
        .write_all(b".TH GZIP-FALLBACK 1\n.SH NAME\ngzip-fallback \\- compressed manual\n")
        .expect("encode fallback source");
    fs::write(
        &compressed_path,
        encoder.finish().expect("finish gzip source"),
    )
    .expect("write fallback source");

    let report = Parser::default()
        .parse_file(&requested)
        .expect("parse the implicit .gz fallback");
    fs::remove_file(compressed_path).expect("remove gzip fallback source");

    assert_eq!(
        report.document.metadata.title.as_deref(),
        Some("GZIP-FALLBACK")
    );
}

#[cfg(windows)]
#[test]
fn windows_rejects_ambient_source_tree_but_accepts_memory_parsing() {
    let report = Parser::default()
        .parse_bytes("memory.1", b".TH MEMORY 1\n.SH NAME\nmemory \\- portable\n")
        .expect("parse caller-owned bytes");
    assert_eq!(report.document.metadata.title.as_deref(), Some("MEMORY"));

    let error = Parser::new(ParseOptions {
        includes: IncludePolicy::SourceTree,
        compression: Compression::Plain,
    })
    .parse_bytes("memory.1", b".so target.1\n")
    .expect_err("reject ambient source-tree inclusion");
    assert_eq!(error.kind, crate::ParseErrorKind::Unsupported);
    assert_eq!(error.path, std::path::Path::new("memory.1"));
}

#[test]
fn invalid_zstd_sources_fail_before_reaching_libmandoc() {
    let path = source_path("invalid-zstd-mandoc-session").with_extension("1.zst");
    fs::write(&path, b"not a zstd frame").expect("write invalid compressed source");

    let error = parse_file(&path, false).expect_err("invalid zstd source must fail");
    fs::remove_file(path).expect("remove invalid compressed source");

    assert!(
        error
            .message
            .starts_with("could not decompress zstd manual source:")
    );
    assert_eq!(error.kind, crate::ParseErrorKind::Decompression);
    assert!(!error.message.contains("unsupported control character"));
}

#[test]
fn oversized_zstd_sources_fail_without_returning_partial_input() {
    let source = vec![b'x'; crate::MAX_DECOMPRESSED_SOURCE_BYTES + 1];
    let compressed =
        zstd::stream::encode_all(source.as_slice(), 0).expect("compress oversized source fixture");
    let error = Parser::default()
        .parse_bytes("oversized.1.zst", &compressed)
        .expect_err("reject a decoded source above the fixed limit");

    assert_eq!(error.kind, crate::ParseErrorKind::Decompression);
    assert!(
        error.message.contains(&format!(
            "{}-byte limit",
            crate::MAX_DECOMPRESSED_SOURCE_BYTES
        )),
        "unexpected decompression error: {error}"
    );
}

#[test]
fn explicit_input_format_overrides_detection_without_changing_parse_options() {
    let options = ParseOptions::default();
    let man = Parser::new(options.clone()).with_input_format(InputFormat::Man);
    let mdoc = Parser::new(options.clone()).with_input_format(InputFormat::Mdoc);

    assert_eq!(man.options(), &options);
    assert_eq!(mdoc.options(), &options);
    assert_eq!(man.input_format(), InputFormat::Man);
    assert_eq!(mdoc.input_format(), InputFormat::Mdoc);
    assert_eq!(
        man.parse_bytes("forced-man.1", b"plain input\n")
            .expect("force man parser")
            .document
            .macro_set,
        MacroSet::Man
    );
    assert_eq!(
        mdoc.parse_bytes("forced-mdoc.1", b"plain input\n")
            .expect("force mdoc parser")
            .document
            .macro_set,
        MacroSet::Mdoc
    );
}

#[test]
fn parser_accepts_owned_bytes_and_detects_zstd_frames() {
    let source = b".TH BYTES 1\n.SH NAME\nbytes \\- parser input\n";
    let plain = Parser::default()
        .parse_bytes("memory.1", source)
        .expect("parse plain byte input");
    assert_eq!(plain.document.metadata.title.as_deref(), Some("BYTES"));

    let compressed = zstd::stream::encode_all(source.as_slice(), 1).expect("compress source");
    let zstd = Parser::default()
        .parse_bytes("memory.1", &compressed)
        .expect("detect and parse zstd byte input");
    assert_eq!(zstd.document.metadata.title.as_deref(), Some("BYTES"));
}

#[test]
fn coding_declarations_never_disable_available_byte_decoding() {
    for declaration in ["latin-1", "ISO-8859-9"] {
        let mut source =
            format!(".\\\" -*- coding: {declaration} -*-\n.TH CD 1\n.SH BODY\nText: ").into_bytes();
        source.extend_from_slice(b"e\xf0itmen ba\xfelat\xfdr.\n");
        let report = Parser::default()
            .parse_bytes("coding.1", &source)
            .expect("unsupported coding declaration retains a best-effort parse");
        let mut visible = Vec::new();
        collect_visible_text(&report.document.root, &mut visible);
        let visible = visible.join(" ");
        assert!(
            visible.contains("e\\[u00F0]itmen ba\\[u00FE]lat\\[u00FD]r."),
            "{declaration}: {visible}"
        );
        assert!(!visible.contains('?'), "{declaration}: {visible}");
    }
}

#[test]
fn parser_decodes_truncated_utf8_tails_without_reading_past_memory_input() {
    for byte in [0xc2, 0xe2, 0xf0] {
        let mut source = b".TH TRUNCATED 1\n.SH BODY\n".to_vec();
        source.push(byte);
        let source = source.into_boxed_slice();
        let report = Parser::default()
            .parse_bytes("truncated.1", &source)
            .expect("truncated UTF-8 tail must retain a best-effort parse");
        let mut visible = Vec::new();
        collect_visible_text(&report.document.root, &mut visible);
        assert!(
            visible.join(" ").contains(&format!("\\[u{byte:04X}]")),
            "byte {byte:#x} was not preserved as Latin-1: {visible:?}"
        );
    }
}
