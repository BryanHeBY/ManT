#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(test)]
mod build_config;

mod ast;
mod diagnostics;
#[allow(unsafe_code)]
mod ffi;
mod parser;
mod special_character;

pub use ast::{
    AuthorMode, DisplayKind, Document, MacroSet, Metadata, Node, NodeFlags, NodeKind,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, TableAlignment, TableCell,
};
pub use diagnostics::{Diagnostic, DiagnosticLevel, SourceLocation};
pub use parser::{
    Compression, IncludePolicy, ParseError, ParseErrorKind, ParseOptions, ParseReport, Parser,
};
pub use special_character::{SpecialCharacter, special_character};

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

    #[cfg(windows)]
    use std::io::Write;

    use super::{
        AuthorMode, Compression, DisplayKind, Document, IncludePolicy, MacroSet, Node, NodeKind,
        NormalizedFont, NormalizedListKind, ParseError, ParseOptions, Parser, TableAlignment,
    };

    #[cfg(windows)]
    use super::DiagnosticLevel;

    fn source_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mant-{label}-{}.1", process::id()))
    }

    fn measured_depth(node: &Node) -> usize {
        1 + node.children.iter().map(measured_depth).max().unwrap_or(0)
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
    fn parser_recognizes_the_modern_man_reference_macro() {
        let report = Parser::default()
            .parse_bytes(
                "modern-reference.1",
                b".TH MODERN-REFERENCE 1\n.SH NAME\nmodern-reference \\- fixture\n\
.SH SEE ALSO\n.MR git-add 1 ,\n",
            )
            .expect("parse modern man reference");

        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("unknown macro")),
            "MR must be a native parser node: {:?}",
            report.diagnostics
        );
        let reference = find_macro(&report.document.root, "MR").expect("MR node");
        assert_eq!(reference.kind, NodeKind::Element);
        assert_eq!(
            reference
                .children
                .iter()
                .filter_map(|child| child.text.as_deref())
                .collect::<Vec<_>>(),
            ["git-add", "1", ","]
        );
    }

    #[test]
    fn parser_accepts_pandoc_verbatim_font_aliases() {
        let report = Parser::default()
            .parse_bytes(
                "pandoc-fonts.1",
                b".TH PANDOC-FONTS 1\n.SH NAME\npandoc-fonts \\- fixture\n\
.SH DESCRIPTION\n\\f[C]code\\f[R] \\f[V]verbatim\\f[R] \\f[VB]bold\\f[R] \\f[VI]italic\\f[R]\n",
            )
            .expect("parse Pandoc font aliases");

        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("invalid escape sequence")),
            "supported font aliases must not emit invalid-escape diagnostics: {:?}",
            report.diagnostics
        );
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
    fn windows_parser_accepts_the_date_formats_used_by_libmandoc() {
        for (date, normalized, normalized_with_style) in [
            ("2026-07-20", "2026-07-20", false),
            ("Jul 20, 2026", "July 20, 2026", true),
            ("July 20, 2026", "July 20, 2026", false),
            ("$Mdocdate: Jul 20 2026 $", "July 20, 2026", false),
        ] {
            let source =
                format!(".TH WINDOWS-DATE 1 \"{date}\"\n.SH NAME\nwindows-date \\- portable\n");
            let report = Parser::default()
                .parse_bytes("windows-date.1", source.as_bytes())
                .expect("parse a supported manual date");

            if normalized_with_style {
                assert_eq!(report.diagnostics.len(), 1);
                assert_eq!(report.diagnostics[0].level, DiagnosticLevel::Style);
                assert_eq!(
                    report.diagnostics[0].message,
                    "normalizing date format to: TH July 20, 2026"
                );
            } else {
                assert!(
                    report.diagnostics.is_empty(),
                    "unexpected diagnostics for {date}: {:?}",
                    report.diagnostics
                );
            }
            assert_eq!(report.document.metadata.date.as_deref(), Some(normalized));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_rejects_c_file_inclusion_but_accepts_memory_parsing() {
        let report = Parser::default()
            .parse_bytes("memory.1", b".TH MEMORY 1\n.SH NAME\nmemory \\- portable\n")
            .expect("parse caller-owned bytes");
        assert_eq!(report.document.metadata.title.as_deref(), Some("MEMORY"));

        let error = Parser::new(ParseOptions {
            includes: IncludePolicy::SourceTree,
            compression: Compression::Plain,
        })
        .parse_bytes("memory.1", b".so target.1\n")
        .expect_err("reject native C file inclusion");
        assert_eq!(error.kind, super::ParseErrorKind::Unsupported);
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

    #[cfg(unix)]
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
    fn parser_preserves_mdoc_delimiter_spacing_roles() {
        let path = source_path("delimiter-role-mandoc-session");
        fs::write(
            &path,
            ".Dd August 4, 2026\n.Dt DELIMITERS 1\n.Os\n.Sh EXAMPLES\n\
             .Dl name ( ) command\n\
             .Dl local [ variable | - ] ...\n\
             .Dl return [ exitstatus ]\n",
        )
        .expect("write delimiter-role source");

        let document = parse_file(&path, false).expect("parse delimiter-role source");
        fs::remove_file(path).expect("remove delimiter-role source");

        let opening_parenthesis = find_node(&document.root, &|node| {
            node.line == 5 && node.text.as_deref() == Some("(")
        })
        .expect("opening parenthesis");
        let closing_parenthesis = find_node(&document.root, &|node| {
            node.line == 5 && node.text.as_deref() == Some(")")
        })
        .expect("closing parenthesis");
        let opening_bracket = find_node(&document.root, &|node| {
            node.line == 7 && node.text.as_deref() == Some("[")
        })
        .expect("opening bracket");
        let trailing_bracket = find_node(&document.root, &|node| {
            node.line == 7 && node.text.as_deref() == Some("]")
        })
        .expect("trailing bracket");

        assert!(opening_parenthesis.flags.delimiter_open);
        assert!(closing_parenthesis.flags.delimiter_close);
        assert!(opening_bracket.flags.delimiter_open);
        assert!(trailing_bracket.flags.delimiter_close);
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

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn explicit_include_root_rejects_linked_target_files() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "libmandoc-rs-linked-include-target-{}",
            process::id()
        ));
        let includes = base.join("includes");
        fs::create_dir_all(&includes).expect("create explicit include root");
        let outside = base.join("outside.1");
        fs::write(
            &outside,
            ".TH OUTSIDE 1\n.SH NAME\noutside \\- must not be included\n",
        )
        .expect("write outside target");
        symlink(&outside, includes.join("target.1")).expect("link target outside root");
        let alias = base.join("alias.1");
        fs::write(&alias, ".so target.1\n").expect("write alias source");

        let result = Parser::new(ParseOptions {
            includes: IncludePolicy::Root(includes),
            compression: Compression::Auto,
        })
        .parse_file(&alias);
        fs::remove_dir_all(base).expect("remove temporary manual tree");

        match result {
            Ok(report) => assert_ne!(report.document.metadata.title.as_deref(), Some("OUTSIDE")),
            Err(error) => assert_eq!(error.kind, super::ParseErrorKind::Parse),
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicit_include_root_rejects_linked_intermediate_directories() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "libmandoc-rs-linked-include-directory-{}",
            process::id()
        ));
        let includes = base.join("includes");
        let outside = base.join("outside");
        fs::create_dir_all(&includes).expect("create explicit include root");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::write(
            outside.join("target.1"),
            ".TH OUTSIDE-DIR 1\n.SH NAME\noutside-dir \\- must not be included\n",
        )
        .expect("write outside target");
        fs::write(outside.join("alias.1"), ".so target.1\n").expect("write alias source");
        symlink(&outside, includes.join("linked")).expect("link directory outside root");
        let alias = includes.join("linked/alias.1");

        let result = Parser::new(ParseOptions {
            includes: IncludePolicy::Root(includes),
            compression: Compression::Auto,
        })
        .parse_file(&alias);
        fs::remove_dir_all(base).expect("remove temporary manual tree");

        match result {
            Ok(report) => assert_ne!(
                report.document.metadata.title.as_deref(),
                Some("OUTSIDE-DIR")
            ),
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

    #[test]
    fn deeply_nested_input_is_bounded_instead_of_overflowing_the_stack() {
        // Far more nesting than the copy cap; the parse must return a finite
        // tree rather than recursing without limit while copying it out.
        let depth = 5_000;
        let mut source = String::from(".TH DEEP 1\n.SH BODY\n");
        for _ in 0..depth {
            source.push_str(".RS\n");
        }
        source.push_str("deep\n");

        let document = Parser::default()
            .parse_bytes("deep.1", source.as_bytes())
            .expect("deeply nested source parses")
            .document;

        // The owned tree stays well under the input nesting, proving the copy
        // stopped descending at the cap.
        assert!(
            measured_depth(&document.root) <= 300,
            "tree depth must be bounded by the copy cap"
        );
    }

    #[test]
    fn deeply_nested_equation_is_bounded_instead_of_overflowing_the_stack() {
        // Braces nest eqn boxes, a recursive walk the node-copy cap never
        // enters: copy_equation descends box->first without limit, so a
        // pathologically nested equation overflows the stack while flattening
        // it. Each `sqrt` level emits text, so an unbounded render would grow
        // the string with the input depth; a bounded one plateaus at the cap.
        let depth = 5_000;
        let mut equation = String::new();
        for _ in 0..depth {
            equation.push_str("sqrt { ");
        }
        equation.push('x');
        for _ in 0..depth {
            equation.push_str(" }");
        }
        let source = format!(".TH DEEP 1\n.SH BODY\n.EQ\n{equation}\n.EN\n");

        let document = Parser::default()
            .parse_bytes("deep-eqn.1", source.as_bytes())
            .expect("deeply nested equation parses")
            .document;

        let node = find_kind(&document.root, NodeKind::Equation).expect("equation node");
        let rendered = node.equation.as_deref().expect("equation text");
        // The render stopped at the cap: the flattened text is far shorter than
        // the ~30k chars all 5000 `sqrt` levels would emit, proving it did not
        // recurse through every box (and so could not overflow the stack).
        assert!(
            rendered.len() < 2_000,
            "equation text must be bounded by the copy cap, got {} bytes",
            rendered.len()
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
             .Bl -tag -compact -offset indent -width 12n\n.It item\nfirst\n.El\n\
             .Bd -literal -offset indent\ncode line\n.Ed\n",
        )
        .expect("write normalized mdoc source");

        let document = parse_file(&path, false).expect("parse normalized mdoc source");
        fs::remove_file(path).expect("remove normalized mdoc source");

        let list = find_macro(&document.root, "Bl").expect("normalized list node");
        assert_eq!(list.list_kind, Some(NormalizedListKind::Definition));
        assert!(list.compact);
        assert_eq!(list.offset.as_deref(), Some("indent"));
        assert_eq!(list.width.as_deref(), Some("12n"));
        let display = find_macro(&document.root, "Bd").expect("normalized display node");
        assert_eq!(display.display_kind, Some(DisplayKind::Literal));
        assert_eq!(display.offset.as_deref(), Some("indent"));
    }

    #[test]
    fn parser_copies_normalized_font_and_author_modes() {
        let report = Parser::default()
            .parse_bytes(
                "normalized-modes.1",
                b".Dd July 19, 2026\n.Dt NORMALIZED-MODES 1\n.Os\n.Sh AUTHORS\n\
.An -split\n.An Alice Example\n.An -nosplit\n.An Bob Example\n\
.Sh DESCRIPTION\n.Bf -literal\nliteral text\n.Ef\n",
            )
            .expect("parse normalized mdoc modes");

        let split = find_node(&report.document.root, &|node| {
            node.macro_name.as_deref() == Some("An") && node.author_mode == Some(AuthorMode::Split)
        });
        let no_split = find_node(&report.document.root, &|node| {
            node.macro_name.as_deref() == Some("An")
                && node.author_mode == Some(AuthorMode::NoSplit)
        });
        let font = find_macro(&report.document.root, "Bf").expect("Bf node");

        assert!(split.is_some());
        assert!(no_split.is_some());
        assert_eq!(font.font, Some(NormalizedFont::Literal));
    }

    #[test]
    fn parser_resolves_stateful_mdoc_enclosures_onto_each_use() {
        let report = Parser::default()
            .parse_bytes(
                "normalized-enclosure.1",
                b".Dd August 17, 2026\n.Dt ENCLOSURE 1\n.Os\n.Sh DESCRIPTION\n\
.Es << >>\n.En value\n",
            )
            .expect("parse stateful mdoc enclosure");

        let enclosure = find_macro(&report.document.root, "En")
            .and_then(|node| node.enclosure.as_ref())
            .expect("resolved En delimiters");
        assert_eq!(enclosure.opening, "<<");
        assert_eq!(enclosure.closing.as_deref(), Some(">>"));
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
