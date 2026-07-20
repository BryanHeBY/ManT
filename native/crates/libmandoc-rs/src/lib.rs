//! Safe ownership boundary around the pinned libmandoc parser.
//!
//! The C shim completes and copies a parse before returning. Rust therefore
//! never observes libmandoc's private `roff_node` layout, and the global C
//! parser state is serialized inside this crate.

#[cfg(test)]
mod build_config;

mod ast;
mod diagnostics;
#[allow(unsafe_code)]
mod ffi;
mod parser;

pub use ast::{
    DisplayKind, Document, MacroSet, Metadata, Node, NodeFlags, NodeKind, NormalizedListKind,
    TableAlignment, TableCell,
};
pub use diagnostics::{Diagnostic, DiagnosticLevel, SourceLocation};
pub use parser::{
    Compression, IncludePolicy, ParseError, ParseErrorKind, ParseOptions, ParseReport, Parser,
};

/// Pinned upstream version compiled by this crate's build script.
pub const LIBMANDOC_VERSION: &str = "1.14.6";

/// Private output of the FFI boundary before diagnostics become public values.
struct RawDocument {
    document: Document,
    diagnostics: String,
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::{
        Compression, DisplayKind, Document, IncludePolicy, MacroSet, Node, NodeKind,
        NormalizedListKind, ParseError, ParseOptions, Parser, TableAlignment,
    };

    fn source_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mant-{label}-{}.1", process::id()))
    }

    fn parse_file(path: &std::path::Path, allow_includes: bool) -> Result<Document, ParseError> {
        Parser::new(ParseOptions {
            includes: if allow_includes {
                IncludePolicy::SourceTree
            } else {
                IncludePolicy::Deny
            },
            compression: Compression::Auto,
        })
        .parse_file(path)
        .map(|report| report.document)
    }

    fn find_macro<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
        (node.macro_name.as_deref() == Some(name))
            .then_some(node)
            .or_else(|| {
                node.children
                    .iter()
                    .find_map(|child| find_macro(child, name))
            })
    }

    fn find_kind(node: &Node, kind: NodeKind) -> Option<&Node> {
        (node.kind == kind).then_some(node).or_else(|| {
            node.children
                .iter()
                .find_map(|child| find_kind(child, kind))
        })
    }

    fn find_node<'a>(node: &'a Node, predicate: &impl Fn(&Node) -> bool) -> Option<&'a Node> {
        predicate(node).then_some(node).or_else(|| {
            node.children
                .iter()
                .find_map(|child| find_node(child, predicate))
        })
    }

    #[test]
    fn upstream_version_is_pinned() {
        assert_eq!(super::LIBMANDOC_VERSION, "1.14.6");
    }

    #[test]
    fn parser_session_returns_an_owned_man_tree() {
        let path = source_path("mandoc-session");
        fs::write(
            &path,
            ".TH MANT 1 \"2026-07-19\"\n.SH NAME\nmant \\- manual viewer\n",
        )
        .expect("write temporary manual source");

        let document = parse_file(&path, false).expect("parse temporary manual");
        fs::remove_file(path).expect("remove temporary manual source");

        assert_eq!(document.macro_set, MacroSet::Man);
        assert_eq!(document.metadata.title.as_deref(), Some("MANT"));
        assert_eq!(document.metadata.section.as_deref(), Some("1"));
        assert!(document.metadata.has_body);
        assert_eq!(document.root.kind, NodeKind::Root);
        assert!(!document.root.children.is_empty());
    }

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
        assert_eq!(error.kind, super::ParseErrorKind::Decompression);
        assert!(!error.message.contains("unsupported control character"));
    }

    #[test]
    fn zstd_sources_keep_their_original_include_root() {
        let root = std::env::temp_dir().join(format!(
            "mant-zstd-include-mandoc-session-{}",
            process::id()
        ));
        let man1 = root.join("man1");
        fs::create_dir_all(&man1).expect("create temporary manual tree");
        let target = man1.join("target.1");
        fs::write(
            &target,
            ".TH ZSTD-INCLUDE 1\n.SH NAME\nzstd-include \\- included manual\n",
        )
        .expect("write included manual");
        let alias = man1.join("alias.1.zst");
        let compressed =
            zstd::stream::encode_all(b".so man1/target.1\n".as_slice(), 1).expect("compress alias");
        fs::write(&alias, compressed).expect("write compressed alias");

        let document = parse_file(&alias, true).expect("resolve include from zstd source");
        fs::remove_dir_all(root).expect("remove temporary manual tree");

        assert_eq!(document.macro_set, MacroSet::Man);
        assert_eq!(document.metadata.title.as_deref(), Some("ZSTD-INCLUDE"));
        assert!(document.metadata.has_body);
    }

    #[test]
    fn parser_preserves_same_line_layout_and_next_line_content_roles() {
        let path = source_path("line-role-mandoc-session");
        fs::write(
            &path,
            ".TH LINE-ROLE 1\n.SH EXAMPLES\n.TP \\w'man\\ 'u\n.BI man \\ ls\nBody.\n",
        )
        .expect("write tagged paragraph source");

        let document = parse_file(&path, false).expect("parse tagged paragraph source");
        fs::remove_file(path).expect("remove tagged paragraph source");

        let tagged_paragraph = find_macro(&document.root, "TP").expect("TP block");
        let head = tagged_paragraph
            .children
            .iter()
            .find(|child| child.kind == NodeKind::Head)
            .expect("TP head");
        assert_eq!(head.children[0].text.as_deref(), Some("96u"));
        assert!(!head.children[0].flags.line_start);
        assert_eq!(head.children[1].macro_name.as_deref(), Some("BI"));
        assert!(head.children[1].flags.line_start);
    }

    #[test]
    fn parser_session_reports_file_errors_as_values() {
        let path = source_path("missing-mandoc-session");
        let error = parse_file(&path, false).expect_err("missing source must fail");

        assert_eq!(error.path, path);
        assert!(!error.message.is_empty());
    }

    #[test]
    fn concurrent_callers_are_serialized_around_libmandoc_globals() {
        let path = source_path("concurrent-mandoc-session");
        fs::write(&path, ".TH THREADS 1\n.SH NAME\nthreads \\- test\n")
            .expect("write temporary manual source");

        let workers: Vec<_> = (0..4)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || parse_file(&path, false))
            })
            .collect();
        for worker in workers {
            let document = worker
                .join()
                .expect("parser worker must not panic")
                .expect("concurrent parse must succeed");
            assert_eq!(document.metadata.title.as_deref(), Some("THREADS"));
        }

        fs::remove_file(path).expect("remove temporary manual source");
    }

    #[test]
    fn source_relative_includes_do_not_change_process_cwd() {
        let root =
            std::env::temp_dir().join(format!("libmandoc-rs-relative-include-{}", process::id()));
        fs::create_dir_all(&root).expect("create temporary manual tree");
        let target = root.join("minimal-mdoc.1");
        fs::write(
            &target,
            ".Dd July 19, 2026\n.Dt INCLUDE-FIXTURE 1\n.Os\n.Sh NAME\ninclude-fixture\n",
        )
        .expect("write included source");
        let alias = root.join("alias-mdoc.1");
        fs::write(&alias, ".so minimal-mdoc.1\n").expect("write alias source");
        let cwd = std::env::current_dir().expect("current directory before parse");

        let document = parse_file(&alias, true).expect("resolve source-relative include");
        fs::remove_dir_all(root).expect("remove temporary manual tree");

        assert_eq!(document.macro_set, MacroSet::Mdoc);
        assert_eq!(document.metadata.title.as_deref(), Some("INCLUDE-FIXTURE"));
        assert_eq!(
            std::env::current_dir().expect("current directory after parse"),
            cwd
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
    fn parser_only_expands_includes_when_policy_allows_a_root() {
        let base = std::env::temp_dir().join(format!(
            "libmandoc-rs-explicit-include-root-{}",
            process::id()
        ));
        let includes = base.join("includes");
        fs::create_dir_all(&includes).expect("create explicit include root");
        fs::write(
            includes.join("target.1"),
            ".TH EXPLICIT-ROOT 1\n.SH NAME\nexplicit-root \\- include fixture\n",
        )
        .expect("write included source");
        let alias = base.join("alias.1");
        fs::write(&alias, ".so target.1\n").expect("write alias source");

        let denied = Parser::default()
            .parse_file(&alias)
            .expect("parse alias without include expansion");
        let expanded = Parser::new(ParseOptions {
            includes: IncludePolicy::Root(includes),
            compression: Compression::Auto,
        })
        .parse_file(&alias)
        .expect("resolve alias against explicit root");
        fs::remove_dir_all(base).expect("remove temporary manual tree");

        assert_ne!(
            denied.document.metadata.title.as_deref(),
            Some("EXPLICIT-ROOT")
        );
        assert_eq!(
            expanded.document.metadata.title.as_deref(),
            Some("EXPLICIT-ROOT")
        );
    }

    #[test]
    fn explicit_include_root_does_not_fall_back_to_process_cwd() {
        let identifier = format!("libmandoc-rs-ambient-{}", process::id());
        let cwd_target = std::env::current_dir()
            .expect("read test cwd")
            .join(format!("{identifier}.1"));
        fs::write(
            &cwd_target,
            ".TH AMBIENT 1\n.SH NAME\nambient \\- must not be included\n",
        )
        .expect("write ambient source");

        let base = std::env::temp_dir().join(format!("{identifier}-root"));
        fs::create_dir_all(&base).expect("create empty include root");
        let alias = base.join("alias.1");
        fs::write(&alias, format!(".so {identifier}.1\n")).expect("write alias source");

        let result = Parser::new(ParseOptions {
            includes: IncludePolicy::Root(base.clone()),
            compression: Compression::Auto,
        })
        .parse_file(&alias);
        fs::remove_file(cwd_target).expect("remove ambient source");
        fs::remove_dir_all(base).expect("remove temporary manual tree");

        match result {
            Ok(report) => assert_ne!(report.document.metadata.title.as_deref(), Some("AMBIENT")),
            Err(error) => assert_eq!(error.kind, super::ParseErrorKind::Parse),
        }
    }

    #[test]
    fn parser_returns_structured_nonfatal_diagnostics() {
        let report = Parser::default()
            .parse_bytes(
                "diagnostics.1",
                b".Dd July 19, 2026\n.Dt BAD 1\n.Os\n.Sh NAME\n.Nm bad\n.ab\n",
            )
            .expect("return best-effort document");

        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.level == super::DiagnosticLevel::Unsupported)
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_feature_round_trips_the_public_parse_report() {
        let report = Parser::default()
            .parse_bytes("serde.1", b".TH SERDE 1\n.SH NAME\nserde \\- fixture\n")
            .expect("parse source for serialization");
        let encoded = serde_json::to_string(&report).expect("serialize parse report");
        let decoded: super::ParseReport =
            serde_json::from_str(&encoded).expect("deserialize parse report");

        assert_eq!(decoded, report);
    }

    #[test]
    fn parser_copies_normalized_list_and_display_attributes() {
        let path = source_path("normalized-mandoc-session");
        fs::write(
            &path,
            ".Dd July 19, 2026\n.Dt NORMALIZED 1\n.Os\n.Sh ITEMS\n\
             .Bl -enum -compact -offset indent\n.It\nfirst\n.El\n\
             .Bd -literal -offset indent\ncode line\n.Ed\n",
        )
        .expect("write normalized mdoc source");

        let document = parse_file(&path, false).expect("parse normalized mdoc source");
        fs::remove_file(path).expect("remove normalized mdoc source");

        let list = find_macro(&document.root, "Bl").expect("normalized list node");
        assert_eq!(list.list_kind, Some(NormalizedListKind::Ordered));
        assert!(list.compact);
        assert_eq!(list.offset.as_deref(), Some("indent"));
        let display = find_macro(&document.root, "Bd").expect("normalized display node");
        assert_eq!(display.display_kind, Some(DisplayKind::Literal));
        assert_eq!(display.offset.as_deref(), Some("indent"));
    }

    #[test]
    fn parser_copies_table_cells_and_equation_text() {
        let path = source_path("structured-payload-mandoc-session");
        fs::write(
            &path,
            ".TH PAYLOAD 1\n.SH TABLE\n.TS\ntab(|);\nl r.\nleft|right\n.TE\n\
             .SH EQUATION\n.EQ\nx sup 2\n.EN\n",
        )
        .expect("write table and equation source");

        let document = parse_file(&path, false).expect("parse table and equation source");
        fs::remove_file(path).expect("remove table and equation source");

        let table = find_kind(&document.root, NodeKind::Table).expect("table row node");
        assert_eq!(table.table_cells.len(), 2);
        assert_eq!(table.table_cells[0].text.as_deref(), Some("left"));
        assert_eq!(table.table_cells[1].alignment, TableAlignment::Right);
        let equation = find_kind(&document.root, NodeKind::Equation).expect("equation node");
        assert!(
            equation
                .equation
                .as_deref()
                .is_some_and(|value| value.contains('x'))
        );
    }

    #[test]
    fn parser_copies_validated_same_document_navigation() {
        let path = source_path("navigation-mandoc-session");
        fs::write(
            &path,
            ".Dd July 19, 2026\n.Dt NAVIGATION 1\n.Os\n.Sh FIRST\n\
             See\n\
             .Sx TARGET\n\
             for details.\n\
             .Tg explicit-target\n\
             .Fl x\n\
             .Sh TARGET\nTarget text.\n",
        )
        .expect("write navigation mdoc source");

        let document = parse_file(&path, false).expect("parse navigation mdoc source");
        fs::remove_file(path).expect("remove navigation mdoc source");

        assert!(find_macro(&document.root, "Sx").is_some());
        let explicit_target = find_node(&document.root, &|node| {
            node.flags.deep_link_target && node.tag.as_deref() == Some("explicit-target")
        });
        let explicit_target = explicit_target.expect("Tg must annotate its resolved destination");
        assert!(explicit_target.flags.permalink);
    }
}
