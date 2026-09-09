//! Validated macro roles, metadata and visible native facts.

use super::*;

#[test]
fn incomplete_root_font_scopes_report_diagnostics_and_reset() {
    let parser = Parser::default();
    for macro_name in ["I", "B", "R", "SM", "SB"] {
        for prefix in ["", ".TH PROBE 1\n", ".I\n", ".SH NAME\n", ".TP\n"] {
            for suffix in ["", "\\c\n"] {
                let source = format!("{prefix}.{macro_name}\n{suffix}");
                let report = parser
                    .parse_bytes("incomplete.1", source.as_bytes())
                    .expect("incomplete scope is recoverable");
                assert!(!report.diagnostics.is_empty(), "{source:?}");
                let report = parser
                    .parse_bytes("next.1", b".TH NEXT 1\n.SH NAME\nnext \\- retained\n")
                    .expect("next session succeeds");
                assert_eq!(report.document.metadata.title.as_deref(), Some("NEXT"));
            }
        }
    }
}

#[test]
fn upstream_version_is_pinned() {
    assert_eq!(crate::LIBMANDOC_VERSION, "1.14.6");
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
fn parser_retains_mdoc_include_arguments() {
    let report = Parser::default()
        .parse_bytes(
            "include.3",
            b".Dd August 19, 2026\n.Dt INCLUDE 3\n.Os\n.Sh SYNOPSIS\n.In fido.h\n",
        )
        .expect("parse mdoc include");

    let include = find_macro(&report.document.root, "In").expect("In node");
    assert_eq!(include.kind, NodeKind::Element);
    assert_eq!(
        include
            .children
            .iter()
            .filter_map(|child| child.text.as_deref())
            .collect::<Vec<_>>(),
        ["fido.h"]
    );
}

#[test]
fn parser_can_pin_bare_mdoc_operating_system_metadata() {
    let parser = Parser::default()
        .with_mdoc_operating_system("PinnedOS 1.0")
        .expect("valid operating-system override");
    assert_eq!(
        parser.mdoc_operating_system().map(std::ffi::CStr::to_bytes),
        Some(b"PinnedOS 1.0".as_slice())
    );

    let bare = parser
        .parse_bytes(
            "bare-os.1",
            b".Dd August 24, 2026\n.Dt BARE-OS 1\n.Os\n.Sh NAME\n.Nm bare-os\n",
        )
        .expect("parse a caller-pinned bare Os macro");
    assert_eq!(bare.document.metadata.os.as_deref(), Some("PinnedOS 1.0"));

    let authored = parser
        .parse_bytes(
            "authored-os.1",
            b".Dd August 24, 2026\n.Dt AUTHORED-OS 1\n.Os AuthoredOS\n.Sh NAME\n.Nm authored-os\n",
        )
        .expect("parse an authored Os value");
    assert_eq!(authored.document.metadata.os.as_deref(), Some("AuthoredOS"));
}

#[test]
fn public_text_normalizes_native_layout_sentinels() {
    let report = Parser::default()
        .parse_bytes(
            "visible-text.1",
            b".Dd August 24, 2026\n.Dt VISIBLE-TEXT 1\n.Os ManT\n.Sh NAME\n.Nm visible-text\n.Nd well-known read-only thing\n",
        )
        .expect("parse hyphenated visible text");
    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);

    assert!(visible.join(" ").contains("well-known read-only thing"));
    assert!(
        find_node(&report.document.root, &|node| {
            node.text.as_deref().is_some_and(|text| {
                text.chars()
                    .any(|character| ['\u{1d}', '\u{1e}', '\u{1f}'].contains(&character))
            })
        })
        .is_none(),
        "public AST text must not expose libmandoc layout sentinels"
    );
}

#[test]
fn parser_expands_the_libbsd_library_name() {
    let report = Parser::default()
        .parse_bytes(
            "libbsd.3bsd",
            b".Dd August 19, 2026\n.Dt LIBBSD 3bsd\n.Os\n.Sh LIBRARY\n.Lb libbsd\n",
        )
        .expect("parse libbsd library declaration");
    let library = find_macro(&report.document.root, "Lb").expect("Lb node");
    let visible = library
        .children
        .iter()
        .filter(|child| !child.flags.no_print)
        .filter_map(|child| child.text.as_deref())
        .collect::<Vec<_>>();

    assert_eq!(
        visible,
        ["Utility functions from BSD systems (libbsd, \\-lbsd)"]
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("unknown library"))
    );
}

#[test]
fn parser_expands_current_mdoc_standard_names() {
    let report = Parser::default()
        .parse_bytes(
            "modern-standards.7",
            b".Dd August 19, 2026\n.Dt MODERN-STANDARDS 7\n.Os\n\
.Sh STANDARDS\n.St -isoC-2023\n.St -p1003.1-2024\n",
        )
        .expect("parse current standards declarations");

    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);

    assert!(
        visible
            .iter()
            .any(|text| text.contains("ISO/IEC 9899:2024")),
        "C23 declaration must expand: {visible:?}"
    );
    assert!(
        visible
            .iter()
            .any(|text| text.contains("IEEE Std 1003.1-2024")),
        "POSIX.1-2024 declaration must expand: {visible:?}"
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
fn parser_accepts_the_date_formats_used_by_libmandoc() {
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

#[test]
fn parser_normalizes_dates_consistently_across_supported_targets() {
    for (date, normalized) in [
        ("February 30, 2026", "March 2, 2026"),
        ("Jul  2, 2026", "July 2, 2026"),
        ("1-1-1", "1-1-1"),
        ("0000-01-01", "0000-01-01"),
        ("January 1, 1960", "January 1, 1960"),
    ] {
        let source =
            format!(".TH PORTABLE-DATE 1 \"{date}\"\n.SH NAME\nportable-date \\- portable\n");
        let report = Parser::default()
            .parse_bytes("portable-date.1", source.as_bytes())
            .expect("parse a portable manual date");

        assert_eq!(
            report.document.metadata.date.as_deref(),
            Some(normalized),
            "date input {date}"
        );
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("bad date argument")),
            "date input {date}: {:?}",
            report.diagnostics
        );
    }
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
fn parser_preserves_mdoc_synopsis_presentation_roles() {
    let path = source_path("synopsis-role-mandoc-session");
    fs::write(
        &path,
        ".Dd August 19, 2026\n.Dt SYNOPSIS-ROLE 3\n.Os\n\
         .Sh SYNOPSIS\n.Fn synopsis_call \"int value\"\n\
         .Fo explicit_call\n.Fa \"int value\"\n.Fc\n\
         .Sh DESCRIPTION\n.Fn prose_call \"int value\"\n",
    )
    .expect("write synopsis-role source");

    let document = parse_file(&path, false).expect("parse synopsis-role source");
    fs::remove_file(path).expect("remove synopsis-role source");

    let synopsis_function = find_node(&document.root, &|node| {
        node.macro_name.as_deref() == Some("Fn") && node.line == 5
    })
    .expect("synopsis Fn");
    let explicit_function = find_node(&document.root, &|node| {
        node.macro_name.as_deref() == Some("Fo") && node.kind == NodeKind::Body
    })
    .expect("synopsis Fo body");
    let prose_function = find_node(&document.root, &|node| {
        node.macro_name.as_deref() == Some("Fn") && node.line == 10
    })
    .expect("prose Fn");

    assert!(synopsis_function.flags.synopsis_pretty);
    assert!(explicit_function.flags.synopsis_pretty);
    assert!(!prose_function.flags.synopsis_pretty);
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
            .any(|diagnostic| diagnostic.level == crate::DiagnosticLevel::Unsupported)
    );
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
    assert_eq!(list.definition_list_style, Some(DefinitionListStyle::Tag));
    assert!(list.compact);
    assert_eq!(list.offset.as_deref(), Some("indent"));
    assert_eq!(list.width.as_deref(), Some("12n"));
    let display = find_macro(&document.root, "Bd").expect("normalized display node");
    assert_eq!(display.display_kind, Some(DisplayKind::Literal));
    assert_eq!(display.offset.as_deref(), Some("indent"));
}

#[test]
fn parser_preserves_each_mdoc_definition_list_style() {
    for (source_style, expected) in [
        ("tag", DefinitionListStyle::Tag),
        ("diag", DefinitionListStyle::Diagnostic),
        ("hang", DefinitionListStyle::Hang),
        ("inset", DefinitionListStyle::Inset),
        ("ohang", DefinitionListStyle::Overhang),
    ] {
        let source = format!(
            ".Dd September 4, 2026\n.Dt LIST-STYLE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{source_style}\n.It term\nDescription.\n.El\n"
        );
        let report = Parser::default()
            .parse_bytes("list-style.1", source.as_bytes())
            .expect("parse definition list style");
        let list = find_macro(&report.document.root, "Bl").expect("definition list node");
        assert_eq!(list.list_kind, Some(NormalizedListKind::Definition));
        assert_eq!(
            list.definition_list_style,
            Some(expected),
            "-{source_style}"
        );
    }
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
        node.macro_name.as_deref() == Some("An") && node.author_mode == Some(AuthorMode::NoSplit)
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
