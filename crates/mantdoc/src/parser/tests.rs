use crate::{
    DiagnosticCode, FatalErrorKind, Limits, MacroSet, NodeKind, Parser, ParserConfig, Severity,
    Source, SourceBundle, SourceName, Syntax,
};

fn maximum_document_depth(document: &crate::Document) -> usize {
    let root = document.node(document.root()).unwrap();
    let mut maximum = 0;
    let mut pending = vec![(root, 1_usize)];
    while let Some((node, depth)) = pending.pop() {
        maximum = maximum.max(depth);
        pending.extend(node.children().map(|child| (child, depth + 1)));
    }
    maximum
}

#[test]
fn physical_os_request_detection_distinguishes_absent_and_bare_forms() {
    assert!(super::source_has_mdoc_operating_system_request(b".Os\n"));
    assert!(super::source_has_mdoc_operating_system_request(
        b".Os OpenBSD\n"
    ));
    assert!(!super::source_has_mdoc_operating_system_request(
        b".Dt TEST 1\n"
    ));
}

#[test]
fn tbl_projection_keeps_utf8_and_malformed_byte_origins_distinct() {
    assert_eq!(
        super::legacy_table_input_text(b"\\[u0080]\xc2\x80"),
        "\\[u0080]\\[u0080]"
    );
    assert_eq!(super::legacy_table_input_text(b"\xc2x"), "?x");
    assert_eq!(
        super::legacy_table_input_text(b"\xc2\xc3\x80"),
        "?\\[u00C0]"
    );
}

#[test]
fn m2_scanner_accepts_arbitrary_bytes_without_utf8_replacement() {
    let name = SourceName::new("arbitrary.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".TH TEST 1\n\xff"))
        .unwrap();
    assert_eq!(report.document.macro_set(), MacroSet::Man);
    assert_eq!(
        report
            .document
            .source_name(report.document.root_source())
            .map(crate::SourceName::as_str),
        Some("arbitrary.1")
    );
    assert_eq!(report.statistics.source_bytes, 12);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        crate::DiagnosticCode::INPUT_INVALID_BYTE
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "skipping bad character: 0xff"
    );
    let children = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].text(), Some("ÿ"));
}

#[test]
fn lowercase_man_title_keeps_the_legacy_visible_diagnostic() {
    let name = SourceName::new("lowercase-title.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".TH bar-man 1\n"))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::MAN_TITLE_NOT_UPPERCASE
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "lower case character in document title: TH bar-man"
    );
}

#[test]
fn lowercase_mdoc_title_keeps_the_legacy_visible_diagnostic() {
    let name = SourceName::new("lowercase-mdoc-title.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt Cm-PUNCT 1\n.Os\n.Sh NAME\n.Nm cm-punct\n.Nd title validation\n",
            ))
            .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::MDOC_TITLE_NOT_UPPERCASE
    );
    assert_eq!(report.diagnostics[0].severity, Severity::Style);
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "lower case character in document title: Dt Cm-PUNCT"
    );
}

#[test]
fn mdoc_date_validation_distinguishes_missing_legacy_and_unparseable_dates() {
    let cases = [
        (
            b".Dd\n.Dt DATE 1\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n".as_slice(),
            DiagnosticCode::MDOC_DATE_MISSING,
            Severity::Warning,
            "missing date, using \"\": Dd",
        ),
        (
            b".Dd \"not a date\"\n.Dt DATE 1\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n".as_slice(),
            DiagnosticCode::MDOC_DATE_UNPARSEABLE,
            Severity::Warning,
            "cannot parse date, using it verbatim: Dd not a date",
        ),
        (
            b".Dd 2014-08-07\n.Dt DATE 1\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n".as_slice(),
            DiagnosticCode::MDOC_DATE_LEGACY,
            Severity::Style,
            "legacy man(7) date format: Dd 2014-08-07",
        ),
    ];
    let name = SourceName::new("mdoc-date-validation.1").unwrap();
    for (source, code, severity, message) in cases {
        let report = Parser::default().parse(Source::new(&name, source)).unwrap();
        assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
        assert_eq!(report.diagnostics[0].code.as_str(), code);
        assert_eq!(report.diagnostics[0].severity, severity);
        assert_eq!(report.diagnostics[0].message.as_ref(), message);
    }
}

#[test]
fn mdoc_date_prologue_order_recovery_preserves_the_last_authored_date() {
    let name = SourceName::new("mdoc-date-prologue-order.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dt DATE 1\n.Dd August 5, 2014\n.Os\n.Sh NAME\n.Nm date\n.Nd validation\n.Sh DESCRIPTION\ntext\n.Dd August 6, 2014\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::MDOC_PROLOGUE_ORDER,
            DiagnosticCode::MDOC_DUPLICATE_PROLOGUE,
        ]
    );
    assert_eq!(
        report.document.metadata().date.as_deref(),
        Some("August 6, 2014")
    );
}

#[test]
fn filled_text_tab_keeps_the_legacy_visible_diagnostic() {
    let name = SourceName::new("filled-tab.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH TABS 1\n.SH DESCRIPTION\nleft\tright\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT
    );
    assert_eq!(report.diagnostics[0].message.as_ref(), "tab in filled text");
}

#[test]
fn copy_mode_string_tabs_survive_expansion_and_warn_in_filled_text() {
    let name = SourceName::new("string-tab.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH TABS 1\n.SH DESCRIPTION\n.ds value\ttext\n>>\\*[value]<<\n",
        ))
        .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(visible.contains(&">>\ttext<<"));
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT
    );
    assert_eq!(
        report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap()),
        Some(crate::SourcePosition { line: 4, column: 3 })
    );
}

#[test]
fn undefined_string_warning_starts_at_its_interpolation() {
    let name = SourceName::new("missing-string.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH STRING 1\n.SH DESCRIPTION\n>>>\\*[missing]<<<\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_UNDEFINED_REFERENCE
    );
    assert_eq!(
        report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap()),
        Some(crate::SourcePosition { line: 3, column: 4 })
    );
}

#[test]
fn missing_strings_on_one_line_report_in_reverse_source_order() {
    let name = SourceName::new("missing-strings.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH STRING 1\n.SH DESCRIPTION\n\\*[first] and \\*[second]\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 3);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "undefined string, using \"\": second",
            "undefined string, using \"\": first",
            "whitespace at end of input line",
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        positions,
        [
            Some(crate::SourcePosition {
                line: 3,
                column: 15,
            }),
            Some(crate::SourcePosition { line: 3, column: 1 }),
            Some(crate::SourcePosition { line: 3, column: 5 }),
        ]
    );
}

#[test]
fn nested_man_examples_keep_non_stack_fill_style_diagnostics() {
    let name = SourceName::new("nested-examples.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH EXAMPLE 1\n.SH DESCRIPTION\n.EX\nouter\n.EX\ninner\n.EE\nouter\n.EE\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 2);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MAN_REDUNDANT_FILL_MODE)
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == Severity::Style)
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "fill mode already disabled, skipping: EX",
            "fill mode already enabled, skipping: EE",
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.primary.as_ref())
        .filter_map(|span| report.document.source_position(span))
        .map(|position| (position.line, position.column))
        .collect::<Vec<_>>();
    assert_eq!(positions, [(5, 2), (9, 2)]);
}

#[test]
fn redundant_man_fill_request_keeps_the_legacy_style_diagnostic() {
    let name = SourceName::new("redundant-fill.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".TH FILL 1\n.SH DESCRIPTION\n.fi\n"))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::MAN_REDUNDANT_FILL_MODE
    );
    assert_eq!(report.diagnostics[0].severity, Severity::Style);
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "fill mode already enabled, skipping: fi"
    );
    let position = report.diagnostics[0]
        .primary
        .as_ref()
        .and_then(|span| report.document.source_position(span))
        .unwrap();
    assert_eq!((position.line, position.column), (3, 2));
}

#[test]
fn implicit_mdoc_enclosures_require_a_blank_before_trailing_delimiters() {
    let name = SourceName::new("implicit-delimiter.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt AQ 1\n.Os\n.Sh DESCRIPTION\n.Aq user@host:\n",
        ))
        .unwrap();
    let diagnostics = report
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING
        })
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code.as_str(),
        DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING
    );
    assert_eq!(diagnostics[0].severity, Severity::Style);
    assert_eq!(
        diagnostics[0].message.as_ref(),
        "no blank before trailing delimiter: Aq user@host:"
    );
    let position = diagnostics[0]
        .primary
        .as_ref()
        .and_then(|span| report.document.source_position(span))
        .unwrap();
    assert_eq!((position.line, position.column), (5, 14));

    let prose = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt PQ 1\n.Os\n.Sh DESCRIPTION\n.Pq Like in this case.\n.Pq \\&.\n",
            ))
            .unwrap();
    assert!(prose.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
}

#[test]
fn selected_syntax_is_deterministic_before_scanning_is_implemented() {
    let name = SourceName::new("syntax.1").unwrap();
    let parser = Parser::new(ParserConfig {
        syntax: Syntax::Mdoc,
        ..ParserConfig::default()
    });
    let report = parser
        .parse(Source::new(&name, b".TH ignored 1\n"))
        .unwrap();
    assert_eq!(report.document.macro_set(), MacroSet::Mdoc);
}

#[test]
fn explicit_roff_syntax_does_not_select_or_structure_macro_packages() {
    let name = SourceName::new("raw-roff.in").unwrap();
    let parser = Parser::new(ParserConfig {
        syntax: Syntax::Roff,
        ..ParserConfig::default()
    });
    let report = parser
        .parse(Source::new(&name, b".TH RAW 1\n.SH BODY\ntext\n"))
        .unwrap();
    assert_eq!(report.document.macro_set(), MacroSet::None);
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes[0].kind(), NodeKind::Element);
    assert_eq!(nodes[1].kind(), NodeKind::Element);
    assert_eq!(nodes[1].macro_name(), Some("SH"));
}

#[test]
fn point_size_and_page_length_requests_are_non_public_formatter_requests() {
    let name = SourceName::new("point-size.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ps 36\n.pl 8000\n.if dps active\nvisible\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty());
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|node| node.kind() == NodeKind::Text));
    assert_eq!(nodes[0].text(), Some("active"));
    assert_eq!(nodes[1].text(), Some("visible"));
}

#[test]
fn font_family_requests_are_non_public_formatter_requests() {
    let name = SourceName::new("font-family.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".ftr V CR\n.ftr VI CI\nvisible\n"))
        .unwrap();
    assert!(report.diagnostics.is_empty());
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["visible"]);
}

#[test]
fn conditional_font_family_setup_consumes_both_scope_closers() {
    let name = SourceName::new("conditional-font-family.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b"'\\\" t\r\n.\\\" comment\r\n.ie \"\\f[CB]x\\f[]\"x\" \\{\\\r\n. ftr V B\r\n.\\}\r\n.el \\{\\\r\n. ftr V CR\r\n.\\}\r\n.TH CONDITIONAL 1\r\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.text() != Some("\r"))
    );
}

#[test]
fn no_adjust_request_is_a_non_public_formatter_request() {
    let name = SourceName::new("no-adjust.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b"before\n.na ignored arguments\nafter\n",
        ))
        .unwrap();
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["before", "after"]);
}

#[test]
fn man_rs_updates_the_an_margin_register_before_text_expansion() {
    let name = SourceName::new("an-margin.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH AN-MARGIN 1\n.SH DESCRIPTION\n.RS 0.0\n\\n[an-margin]\n.RS 3.5\n\\n[an-margin]\n.RE\n\\n[an-margin]\n.RE\n\\n[an-margin]\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let values = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .filter(|text| text.chars().all(|character| character.is_ascii_digit()))
        .collect::<Vec<_>>();
    assert_eq!(values, ["168", "252", "168", "168"]);
}

#[test]
fn m3_mdoc_os_uses_the_session_fallback_only_when_the_source_is_bare() {
    let name = SourceName::new("operating-system.1").unwrap();
    let parser = Parser::new(ParserConfig {
        syntax: Syntax::Mdoc,
        operating_system: Some("PinnedOS 1.0".into()),
        ..ParserConfig::default()
    });

    let bare = parser
        .parse(Source::new(
            &name,
            b".Dd August 24, 2026\n.Dt BARE-OS 1\n.Os\n",
        ))
        .unwrap();
    assert_eq!(bare.document.metadata().os.as_deref(), Some("PinnedOS 1.0"));

    let authored = parser
        .parse(Source::new(
            &name,
            b".Dd August 24, 2026\n.Dt AUTHORED-OS 1\n.Os AuthoredOS\n",
        ))
        .unwrap();
    assert_eq!(
        authored.document.metadata().os.as_deref(),
        Some("AuthoredOS")
    );

    let man = parser
        .parse(Source::new(
            &name,
            b".TH FALLBACK-OS 1\n.SH NAME\nfallback-os\n",
        ))
        .unwrap();
    assert_eq!(man.document.metadata().os.as_deref(), Some("PinnedOS 1.0"));
}

#[test]
fn m3_string_definition_quotes_are_not_generic_argument_quotes() {
    let name = SourceName::new("string-quote.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds foo \"first part\n.as foo \" second part\n\\*[foo]\n.ds bar \"string value\"\n\\*[bar]\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["first part second part", "string value\""]);
}

#[test]
fn m3_string_definition_copy_mode_is_preserved_in_generated_and_scoped_execution() {
    let name = SourceName::new("generated-string-quote.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de rootdef\n.ds root \"root \"quote\n..\n.de scopedef\n.ds scoped \"scoped \"quote\n..\n.rootdef\n.if 1 .ds inline \"inline \"quote\n.if 1 \\{\\\n.ds block \"block \"quote\n.scopedef\n.if 1 .ds nested \"nested \"quote\n.\\}\n\\*[root]\n\\*[inline]\n\\*[block]\n\\*[scoped]\n\\*[nested]\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        [
            "root \"quote",
            "inline \"quote",
            "block \"quote",
            "scoped \"quote",
            "nested \"quote"
        ]
    );
}

#[test]
fn m3_nested_string_names_are_resolved_before_the_outer_lookup() {
    let name = SourceName::new("nested-string.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ds foo bar\n.ds bar output\nThis is \\*[\\*[foo]].\n",
        ))
        .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["This is output."]);
}

#[test]
fn m3_ignore_blocks_consume_source_without_retaining_their_contents() {
    let name = SourceName::new("ignore.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b"before\n.ig custom\nignored one\n.ig\nignored two\n..\n.custom\nafter\n.ig\nignored through eof\n",
            ))
            .unwrap();

    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [DiagnosticCode::ROFF_UNCLOSED_IGNORE]
    );
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["before", "after"]);
}

#[test]
fn m3_macro_generated_ignore_requests_consume_following_physical_input() {
    let name = SourceName::new("macro-ignore.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de top\n.ig top-end\n..\n.top\ntop-hidden\n.top-end\ntop-visible\n.de scoped\n.ig scope-end\n..\n.if 1 \\{\\\n.scoped\n.\\}\nscope-hidden\n.scope-end\nscope-visible\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["top-visible", "scope-visible"]);
}

#[test]
fn m3_macro_generated_definition_consumes_following_copy_mode_input() {
    let name = SourceName::new("macro-definition.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de outer\n.de inner\n..\n.outer\ninner body\n..\n.inner\n",
        ))
        .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["inner body"]);
}

#[test]
fn m3_macro_replay_discards_input_comments_after_control_arguments() {
    let name = SourceName::new("macro-inline-comment.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            br#".de annotated
.IR troff s, \" formatter annotation
..
.annotated
"#,
        ))
        .unwrap();

    let visible = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        ["troff", "s,"],
        "input comment leaked through macro replay"
    );
}

#[test]
fn m3_macro_replay_honors_active_escape_for_input_comments() {
    let name = SourceName::new("macro-custom-inline-comment.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            br#".ec @
.de annotated
.IR troff s, @" formatter annotation
..
.annotated
"#,
        ))
        .unwrap();

    let visible = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        ["@", "troff", "s,"],
        "custom input comment leaked through macro replay"
    );
}

#[test]
fn m3_macro_generated_indirect_definitions_resolve_names_before_copy_mode() {
    let name = SourceName::new("macro-indirect-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds target inner\n.ds marker done\n.de outer\n.dei target marker\n.ami target marker\n..\n.outer\nfirst\n.done\nsecond\n.done\n.inner\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["first", "second"]);
}

#[test]
fn m3_macro_conditional_scopes_select_their_immediate_else_branch() {
    let name = SourceName::new("macro-conditional-scope.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de decide\n.ie \\$1 \\{\\\nhit \\$1\n.br\\}\n.el \\{\\\nmiss\n.br\\}\n..\n.decide 1\n.decide 0\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["hit 1", "miss"]);
}

#[test]
fn m3_scope_macros_execute_their_own_conditional_brace_frames() {
    let name = SourceName::new("scope-macro-conditional.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de decide\n.ie \\$1 \\{\\\nhit \\$1\n.br\\}\n.el \\{\\\nmiss\n.br\\}\n..\n.if 1 \\{\\\n.decide 1\n.decide 0\n.\\}\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["hit 1", "miss"]);
}

#[test]
fn m3_scope_macros_can_install_indirect_definitions_from_following_input() {
    let name = SourceName::new("scope-macro-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds target inner\n.ds marker done\n.de outer\n.dei target marker\n.ami target marker\n..\n.if 1 \\{\\\n.outer\n.\\}\nfirst\n.done\nsecond\n.done\n.inner\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["first", "second"]);
}

#[test]
fn m3_collected_scope_ignores_direct_lines_through_its_local_marker() {
    let name = SourceName::new("scope-ignore.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\nbefore\n.ig stop\nhidden\n.stop\nafter\n.\\}\noutside\n",
        ))
        .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["before", "after", "outside"]);
}

#[test]
fn root_input_limit_is_fatal_before_ast_allocation() {
    let name = SourceName::new("too-large.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_root_source_bytes = 3;
    let error = Parser::new(config)
        .parse(Source::new(&name, b"four"))
        .unwrap_err();
    assert_eq!(error.kind, FatalErrorKind::SourceLimit);
}

#[test]
fn source_line_limit_bounds_document_line_index_allocation() {
    let name = SourceName::new("many-lines.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_source_lines = 2;
    let error = Parser::new(config)
        .parse(Source::new(&name, b"one\ntwo\nthree"))
        .unwrap_err();
    assert_eq!(error.kind, FatalErrorKind::SourceLineLimit);
}

#[test]
fn scanner_emits_control_arguments_and_honors_dynamic_characters() {
    let name = SourceName::new("dynamic.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".cc !\n!ec @\nvisible @(em @\\ tail\n!TH \"two words\" 1\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes[0].macro_name(), Some("cc"));
    assert_eq!(nodes[1].macro_name(), Some("ec"));
    assert_eq!(nodes[2].text(), Some("visible — @ tail"));
    assert_eq!(nodes[3].macro_name(), Some("TH"));
    assert_eq!(
        nodes[3]
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>(),
        ["two words", "1"]
    );
}

#[test]
fn character_control_requests_are_private_and_discard_excess_bytes_in_man_input() {
    let name = SourceName::new("character-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CHARACTER-CONTROL 1\n.SH DESCRIPTION\n.cc :\n:cc ;bogus\ntext\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
            "skipping excess arguments: cc ... bogus",
        )]
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (4, 6));
    assert!(
        report
            .document
            .preorder()
            .all(|node| !matches!(node.macro_name(), Some("cc" | "c2" | "ec")))
    );
}

#[test]
fn roff_font_requests_validate_and_project_the_legacy_ft_shape() {
    let name = SourceName::new("font-request.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd January 1, 2020\n.Dt FONT-REQUEST 1\n.Os\n.Sh NAME\n.Nm font-request\n.Nd font validation\n.Sh DESCRIPTION\n.ft B\n.ft foo\n.ft I bogus\n.ft P\n.ft\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                "skipping excess arguments: ft ... bogus",
            ),
            (
                DiagnosticCode::ROFF_UNKNOWN_FONT,
                "unknown font, skipping request: ft foo",
            ),
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .map(|position| (position.line, position.column))
            })
            .collect::<Vec<_>>(),
        [Some((10, 7)), Some((9, 2))]
    );
    let fonts = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("ft"))
        .map(|node| {
            node.children()
                .map(crate::NodeRef::text)
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()
        .unwrap();
    assert_eq!(fonts, [vec!["B"], vec!["I"], vec!["P"], vec!["P"]]);
}

#[test]
fn char_requests_are_private_but_expand_declared_character_values() {
    let name = SourceName::new("character-definitions.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CHARACTER-DEFINITIONS 1\n.SH DESCRIPTION\n.char \\[myc] myval\n.char x y\n.char \\[boldX] \\fBX\n\\[boldX] \\[myc]\nfinal text\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "invalid escape sequence: \\[myc]",
            "invalid escape sequence: \\[boldX]",
            "invalid escape sequence: \\[myc]",
            "invalid escape sequence: \\[boldX]",
        ]
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"\\fBX\\fP myval"));
    assert!(text.contains(&"final teyt"));
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("char"))
    );
}

#[test]
fn char_requests_report_invalid_left_operands_at_their_precise_source_spans() {
    let name = SourceName::new("character-invalid.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CHARACTER-INVALID 1\n.SH DESCRIPTION\n.char\n.char \\fR myval\n.char \\[myc]x myval\n.char xy myval\nmyc: <\\[myc]> x\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char",
            ),
            (
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char \\fR myval",
            ),
            (
                DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                "invalid escape sequence: \\[myc]",
            ),
            (
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char \\[myc]x myval",
            ),
            (
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char xy myval",
            ),
            (
                DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                "invalid escape sequence: \\[myc]",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(3, 6), (4, 7), (5, 7), (5, 7), (6, 7), (7, 7)]);
}

#[test]
fn scanner_limits_return_a_bounded_prefix_and_typed_findings() {
    let name = SourceName::new("bounded.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_nodes = 2;
    config.limits.max_diagnostics = 4;
    let report = Parser::new(config)
        .parse(Source::new(&name, b"one\ntwo\nthree\n"))
        .unwrap();
    assert_eq!(report.document.node_count(), 2);
    assert!(report.statistics.truncated);
    assert_eq!(report.diagnostics[0].code.as_str(), "limits.nodes");
}

#[test]
fn default_tree_limit_matches_the_legacy_finite_prefix_boundary() {
    let name = SourceName::new("deep-man.1").unwrap();
    let mut source = String::from(".TH DEEP 1\n.SH BODY\n");
    for _ in 0..300 {
        source.push_str(".RS\n");
    }
    source.push_str("retained prefix\n");
    for _ in 0..300 {
        source.push_str(".RE\n");
    }

    let report = Parser::default()
        .parse(Source::new(&name, source.as_bytes()))
        .unwrap();
    assert!(report.statistics.truncated);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::LEGACY_SYNTAX_TREE_DEPTH_LIMIT
    }));
    assert_eq!(
        report.document.node_count(),
        report.document.preorder().count()
    );
    assert_eq!(maximum_document_depth(&report.document), 256);
    assert_eq!(
        report.statistics.emitted_nodes,
        report.document.node_count()
    );
}

#[test]
fn caller_selected_tree_limit_uses_the_native_limit_code() {
    let name = SourceName::new("narrow-tree.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_tree_depth = 4;
    let mut source = String::from(".TH NARROW 1\n.SH BODY\n");
    for _ in 0..10 {
        source.push_str(".RS\n");
    }
    source.push_str("text\n");
    let report = Parser::new(config)
        .parse(Source::new(&name, source.as_bytes()))
        .unwrap();

    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::LIMIT_TREE_DEPTH)
    );
    assert_eq!(maximum_document_depth(&report.document), 4);
}

#[test]
fn m3_semantic_staging_respects_the_node_budget_before_adding_man_parts() {
    let name = SourceName::new("bounded-man.1").unwrap();
    let mut config = ParserConfig {
        syntax: Syntax::Man,
        ..ParserConfig::default()
    };
    config.limits.max_nodes = 4;
    let report = Parser::new(config)
        .parse(Source::new(&name, b".SH BOUNDED\n"))
        .unwrap();

    // The scanner emitted root, SH, and its argument.  Forming the
    // staging Block/Head/Body shape needs two more nodes, so the original
    // event remains reachable rather than exceeding max_nodes.
    assert_eq!(report.document.node_count(), 3);
    let section = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(section.kind(), NodeKind::Element);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.nodes")
    );
}

#[test]
fn m5_semantic_staging_respects_the_node_budget_before_adding_mdoc_parts() {
    let name = SourceName::new("bounded-mdoc.1").unwrap();
    let mut config = ParserConfig {
        syntax: Syntax::Mdoc,
        ..ParserConfig::default()
    };
    config.limits.max_nodes = 4;
    let report = Parser::new(config)
        .parse(Source::new(&name, b".Sh BOUNDED\n"))
        .unwrap();

    // The scanner emitted root, Sh, and its argument. Forming the
    // staging Block/Head/Body shape needs two more nodes, so the original
    // event remains reachable rather than exceeding max_nodes.
    assert_eq!(report.document.node_count(), 3);
    let section = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(section.kind(), NodeKind::Element);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.nodes")
    );
}

#[test]
fn aggregate_escape_work_limit_stops_before_unbounded_scanner_output() {
    let name = SourceName::new("escapes.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_expansion_steps = 1;
    let report = Parser::new(config)
        .parse(Source::new(&name, b"\\&\\&\nnext\n"))
        .unwrap();
    assert_eq!(report.document.node_count(), 1);
    assert!(report.statistics.truncated);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        "limits.expansion-steps"
    );
}

#[test]
fn scanner_is_total_and_source_bounded_for_every_two_byte_prefix() {
    let name = SourceName::new("all-byte-prefixes.roff").unwrap();
    let parser = Parser::default();
    for first in u8::MIN..=u8::MAX {
        for second in u8::MIN..=u8::MAX {
            let bytes = [first, second];
            let report = parser.parse(Source::new(&name, &bytes)).unwrap();
            assert_eq!(
                report.document.node_count(),
                report.statistics.emitted_nodes
            );
            for node in report.document.preorder() {
                if let Some(span) = node.location() {
                    assert!(span.start <= span.end);
                    assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                    assert!(report.document.source_position(span).is_some());
                }
            }
            for finding in &report.diagnostics {
                for span in finding
                    .primary
                    .iter()
                    .chain(finding.related.iter().map(|related| &related.span))
                {
                    assert!(span.start <= span.end);
                    assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                    assert!(report.document.source_position(span).is_some());
                }
            }
        }
    }
}

#[test]
fn dynamic_character_requests_keep_public_spans_inside_source_bytes() {
    let name = SourceName::new("fuzz-dynamic-control.roff").unwrap();
    let bytes = b".cc !\x8c";
    let report = Parser::default().parse(Source::new(&name, bytes)).unwrap();
    for node in report.document.preorder() {
        if let Some(span) = node.location() {
            assert!(span.start <= span.end);
            assert!(usize::try_from(span.end).unwrap() <= bytes.len());
            assert!(report.document.source_position(span).is_some());
        }
    }
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_EXCESS_ARGUMENTS
    );
    for finding in &report.diagnostics {
        if let Some(span) = &finding.primary {
            assert!(span.start <= span.end);
            assert!(usize::try_from(span.end).unwrap() <= bytes.len());
            assert!(report.document.source_position(span).is_some());
        }
    }
}

#[test]
fn man_title_validation_keeps_malformed_input_diagnostic_spans_in_bounds() {
    let name = SourceName::new("fuzz-man-title.roff").unwrap();
    let bytes = b".TH A\xc7n";
    let report = Parser::default().parse(Source::new(&name, bytes)).unwrap();
    let finding = report
        .diagnostics
        .iter()
        .find(|finding| finding.code.as_str() == DiagnosticCode::MAN_TITLE_NOT_UPPERCASE)
        .expect("malformed title still contains an ASCII lower-case character");
    let span = finding.primary.as_ref().expect("title finding has a span");
    assert_eq!((span.start, span.end), (6, 7));
    assert!(usize::try_from(span.end).unwrap() <= bytes.len());
    assert!(report.document.source_position(span).is_some());
}

#[test]
fn pinned_head_c_escape_shape_is_visible_before_roff_scope_execution() {
    let name = SourceName::new("c_man.in").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".B\none\\c\nword\n"))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes[0].macro_name(), Some("B"));
    assert_eq!(nodes[1].text(), Some("one"));
    assert!(nodes[1].flags().line_continuation);
    assert_eq!(nodes[2].text(), Some("word"));
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
}

#[test]
fn package_ast_retains_a_final_no_space_escape() {
    let name = SourceName::new("package-c.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH PACKAGE-C 1\n.SH DESCRIPTION\none\\c\nword\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .find(|node| node.text() == Some("one\\c"))
        .expect("the package AST retains the authored escape");
    assert!(text.flags().line_continuation);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_environment_requests_expand_text_and_control_arguments() {
    let name = SourceName::new("environment.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds title mantdoc\n.nr count 7\n.TH \\*[title] \\n[count]\ntext \\*[title] \\n[count]\n.as title -rs\n\\*[title]\n.rm title count\n\\*[title] \\n[count]\n",
            ))
            .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(report.document.metadata().title.as_deref(), Some("mantdoc"));
    assert_eq!(report.document.metadata().section.as_deref(), Some("7"));
    assert_eq!(nodes[0].text(), Some("text mantdoc 7"));
    assert_eq!(nodes[1].text(), Some("mantdoc-rs"));
    assert_eq!(nodes[2].text(), Some(" 0"));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|finding| finding.code.as_str() == "roff.undefined-reference")
            .count(),
        1
    );
}

#[test]
fn empty_user_strings_are_silent_in_control_position() {
    let name = SourceName::new("empty-string-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH EMPTY-STRING 1\n.SH DESCRIPTION\n.ds empty \"\n.empty\nvisible\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("empty"))
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("visible"))
    );
}

#[test]
fn mdoc_control_arguments_expand_unescaped_string_references() {
    let name = SourceName::new("mdoc-string-argument.1").unwrap();
    let report = Parser::new(ParserConfig {
            operating_system: Some("mantdoc canonical differential".into()),
            ..ParserConfig::default()
        })
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt STRING-ARG 1\n.Os\n.Sh DESCRIPTION\n.ds o \\(Fo\n.Eo \\*o\nbody\n.Ec \\*o\n.Pp\n.Eo \\\\*o\nbody\n.Ec \\\\*o\n",
            ))
            .unwrap();
    let texts = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(texts.iter().filter(|text| **text == "\\(Fo").count() >= 4);
    assert!(!texts.contains(&"\\*o"));
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
}

#[test]
fn m3_string_definitions_retain_the_full_unquoted_value() {
    let name = SourceName::new("string-value.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ds phrase native rust parser\n\\*[phrase]\n.as phrase with bounds\n\\*[phrase]\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        ["native rust parser", "native rust parserwith bounds"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn recursive_string_expansion_drops_only_its_own_input_line() {
    let name = SourceName::new("recursive-string.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH RECURSIVE-STRING 1\n.SH DESCRIPTION\n.ds recur \\\\*[recur]\nbefore recursion\n(and do not \\*[recur] print this)\nafter recursion\n",
            ))
            .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        (
            report.diagnostics[0].code.as_str(),
            report.diagnostics[0].severity,
            report.diagnostics[0].message.as_ref(),
        ),
        (
            DiagnosticCode::LIMIT_EXPANSION_STEPS,
            Severity::Error,
            "input stack limit exceeded, infinite loop?",
        )
    );
    assert_eq!(
        report.diagnostics[0]
            .primary
            .as_ref()
            .and_then(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column)),
        Some((5, 13))
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"before recursion"));
    assert!(text.contains(&"after recursion"));
    assert!(!text.iter().any(|value| value.contains("print this")));
}

#[test]
fn string_definition_names_normalize_literal_escapes_and_reject_other_escapes() {
    let name = SourceName::new("string-escaped-name.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ds std\\\\esc stdval\n\\*[std\\\\esc]\n.ds esc\\eesc ignored\n\\*[esc]\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_ESCAPED_NAME,
                "escaped character not allowed in a name: esc\\e",
            ),
            (
                DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                "undefined string, using \"\": esc",
            ),
        ]
    );
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"stdval"));
    assert!(!text.contains(&"ignored"));
}

#[test]
fn mdoc_bracketed_string_name_preserves_its_literal_escape_until_lookup() {
    let name = SourceName::new("mdoc-string-escaped-name.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt ESCAPED-NAME 1\n.Os\n.Sh NAME\n.Nm escaped-name\n.Nd test\n.Sh DESCRIPTION\n.ds std\\\\esc stdval\n.Sq \\*[std\\\\esc] .\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("stdval"))
    );
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.text() == Some(""))
    );
}

#[test]
fn m3_copy_mode_macro_body_expands_arguments_at_invocation() {
    let name = SourceName::new("macro.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds salutation welcome\n.de greet\nHello, \\$1!\n\\*[salutation]\n..\n.ds salutation later\n.greet mantdoc\n",
            ))
            .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].text(), Some("Hello, mantdoc!"));
    assert_eq!(nodes[1].text(), Some("welcome"));
    assert!(nodes.iter().all(|node| !node.flags().generated));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_generated_controls_relex_expanded_macro_arguments() {
    let name = SourceName::new("macro-expanded-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH MACRO 1\n.SH DESCRIPTION\n.de show\n.BI \\$@\n..\n.show one two three\n",
        ))
        .unwrap();
    let bold_italic = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("BI"))
        .unwrap();
    assert_eq!(
        bold_italic
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["one", "two", "three"]
    );
    assert_eq!(
        bold_italic
            .children()
            .map(|argument| {
                report
                    .document
                    .source_position(argument.location().expect("argument location"))
                    .expect("argument source position")
            })
            .collect::<Vec<_>>(),
        [
            crate::SourcePosition { line: 6, column: 5 },
            crate::SourcePosition {
                line: 6,
                column: 11
            },
            crate::SourcePosition {
                line: 6,
                column: 17
            },
        ]
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn man_attached_name_escape_rebases_the_first_visible_argument() {
    let name = SourceName::new("attached-man-escape.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH ATTACHED 1\n.SH DESCRIPTION\n.IB\\(lqone two\n",
        ))
        .unwrap();
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("IB"))
        .expect("recovered IB macro");
    let first = macro_node.children().next().expect("first argument");
    assert_eq!(first.text(), Some("one"));
    assert_eq!(
        report
            .document
            .source_position(first.location().expect("argument location")),
        Some(crate::SourcePosition { line: 3, column: 8 })
    );
}

#[test]
fn m3_direct_definition_in_a_macro_spans_pending_and_following_input() {
    let name = SourceName::new("nested-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de outer\nouter macro\n.de inner\ninner macro\n..\nouter definition ended\n.outer\nfollowing caller input\n..\ninner definition ended\n.inner\nfinal text\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "outer definition ended",
            "outer macro",
            "inner definition ended",
            "inner macro",
            "following caller input",
            "final text",
        ]
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn roff_input_traps_reparse_the_armed_macro_after_the_matching_text_line() {
    let name = SourceName::new("input-trap.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INPUT-TRAP 1\n.SH DESCRIPTION\n.de first\nfirst trap\n..\n.de second\nsecond trap\n..\n.it 1first\none\n.it 2 second\ntwo\nthree\nfour\n",
            ))
            .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "DESCRIPTION",
            "one",
            "first trap",
            "two",
            "three",
            "second trap",
            "four"
        ]
    );
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("it"))
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn man_builtin_macro_names_take_precedence_over_roff_definitions() {
    let name = SourceName::new("defined-man-macro.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH DEFINED-MAN 1\n.de BI\n.IB \\$1 \\$2 \\$3\n..\n.SH DESCRIPTION\n.BI bold italic bold\n",
            ))
            .unwrap();
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("BI"))
        .expect("the authored BI remains a man element");
    let children = macro_node
        .children()
        .map(|node| node.text().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(children, ["bold", "italic", "bold"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn implemented_mdoc_macro_names_take_precedence_over_roff_definitions() {
    let name = SourceName::new("defined-mdoc-macro.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DEFINED-MDOC 1\n.Os\n.de At\nBSD\n..\n.Sh DESCRIPTION\n.At\n",
            ))
            .unwrap();
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("At"))
        .expect("the authored At remains an mdoc element");
    let child = macro_node.children().next().expect("At default child");
    assert_eq!(child.text(), Some("AT&T UNIX"));
    assert!(child.flags().generated);
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
}

#[test]
fn at_expands_standard_versions_and_recovers_unknown_selectors() {
    let name = SourceName::new("at-versions.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AT-VERSIONS 1\n.Os\n.Sh DESCRIPTION\n.At v7\n.At murks \"Sy\" bold\n",
            ))
            .unwrap();
    let at_nodes = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("At"))
        .collect::<Vec<_>>();
    assert_eq!(at_nodes.len(), 2);
    let valid_children = at_nodes[0].children().collect::<Vec<_>>();
    assert_eq!(
        valid_children
            .iter()
            .copied()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["Version\\~7 AT&T UNIX", "v7"]
    );
    assert!(valid_children[0].flags().generated);
    assert!(valid_children[1].flags().no_print);
    let invalid_children = at_nodes[1].children().collect::<Vec<_>>();
    assert_eq!(
        invalid_children
            .iter()
            .copied()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["AT&T UNIX", "murks"]
    );
    assert!(invalid_children[0].flags().generated);
    assert!(report.document.preorder().any(|node| {
        node.macro_name() == Some("Sy")
            && node.children().next().and_then(crate::NodeRef::text) == Some("bold")
    }));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME,
            "mdoc.unknown-at-version",
        ]
    );
    assert_eq!(
        report.diagnostics[1].message.as_ref(),
        "unknown AT&T UNIX version: At murks"
    );
}

#[test]
fn appended_mdoc_closer_keeps_its_builtin_scope_action() {
    let name = SourceName::new("appended-mdoc-closer.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt APPENDED-CLOSER 1\n.Os\n.Sh DESCRIPTION\n.Bo in brackets\n.Bc end\n.am Bc\n.Pq appended words\n..\n.Bo in brackets\n.Bc end\n",
            ))
            .unwrap();
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
    let bracket_bodies = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        .map(|body| {
            body.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(bracket_bodies, [["in brackets"], ["in brackets"]]);
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("Pq"))
    );
}

#[test]
fn renamed_appended_mdoc_closer_keeps_scope_and_caller_provenance() {
    let name = SourceName::new("renamed-appended-mdoc-closer.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt RENAMED-APPENDED-CLOSER 1\n.Os\n.Sh NAME\n.Nm renamed-appended-closer\n.Nd package macro alias\n.Sh DESCRIPTION\n.rn Bc myBc\n.Bo first brackets\n.myBc\n.am myBc\n.Pq appended words\n..\n.Bo second brackets\n.myBc\n",
            ))
            .unwrap();
    assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::INPUT_TRAILING_WHITESPACE
    );
    let diagnostic_position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        (diagnostic_position.line, diagnostic_position.column),
        (15, 4)
    );

    let bracket_bodies = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        .map(|body| {
            body.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(bracket_bodies, [["first brackets"], ["second brackets"]]);

    let appended_text = report
        .document
        .preorder()
        .find(|node| node.text() == Some("appended words"))
        .unwrap();
    let appended_position = report
        .document
        .source_position(appended_text.location().unwrap())
        .unwrap();
    assert_eq!((appended_position.line, appended_position.column), (15, 5));
}

#[test]
fn m3_indirect_macro_definitions_expand_names_and_custom_terminators() {
    let name = SourceName::new("indirect-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds target delayed\n.ds end-marker done\n.dei target end-marker\nfirst\n.done trailing words\n.ami target end-marker\nsecond\n.done\n.delayed\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["first", "second"]);
}

#[test]
fn m3_copy_mode_reparses_delayed_register_adjustments_on_invocation() {
    let name = SourceName::new("copy-register.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 2 1\n.de decrement\n\\\\n-[count]\n..\n.decrement\ncount \\n[count]\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["1", "count 1"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_while_brace_scope_reexecutes_controls_and_closes_inline_text() {
    let name = SourceName::new("while-scope.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 3\n.while \\n[count] \\{\\\n.nr count -1\n\\n[count]\\},\nafter\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2,", "1,", "0,", "after"]);
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_break_in_a_scoped_conditional_stops_only_the_current_while() {
    let name = SourceName::new("while-break.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 3 1\n.while n \\{\\\n\\n-[count]\n.if !\\n[count] .break\nnext\n.\\}\nafter\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2", "next", "1", "next", "0", "after"]);
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_nested_while_scopes_execute_on_an_explicit_frame_stack() {
    let name = SourceName::new("nested-while.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr outer 2\n.while \\n[outer] \\{\\\n.nr inner 2\n.while \\n[inner] \\{\\\n\\n[outer]:\\n[inner]\n.nr inner -1\n.\\}\n.nr outer -1\n.\\}\nafter\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2:2", "2:1", "after"]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::ROFF_WHILE_NESTED,
            DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
        ]
    );
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_macro_body_can_close_the_active_while_scope() {
    let name = SourceName::new("while-macro-close.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2\n.de close\n.nr count -1\n.\\}\n..\n.while \\n[count] \\{\\\n\\n[count]\n.close\ninside-never\n.\\}\nafter\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2", "inside-never", "after"]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.severity))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_WHILE_INNER_SCOPE,
                Severity::Unsupported
            ),
            (
                DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                Severity::Unsupported
            ),
        ]
    );
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_copy_mode_does_not_apply_control_changes_before_macro_invocation() {
    let name = SourceName::new("copy-control.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".de delayed\n.cc !\n..\noutside\n"))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("outside"));
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "roff.unterminated-definition")
    );
}

#[test]
fn m3_macro_control_changes_activate_only_when_the_macro_runs() {
    let name = SourceName::new("copy-control-run.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de delayed\n.cc !\n!B generated\n..\n.delayed\n!TH title 1\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].macro_name(), Some("B"));
    assert!(!nodes[0].flags().generated);
    assert_eq!(
        nodes[0].children().next().unwrap().text(),
        Some("generated")
    );
    assert_eq!(nodes[1].macro_name(), Some("TH"));
    assert_eq!(
        nodes[1]
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>(),
        ["title", "1"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macro_body_control_requests_become_generated_events() {
    let name = SourceName::new("macro-controls.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de show\n.ds prefix welcome\n.B \\$1\n..\n.show mantdoc\n\\*[prefix]\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].macro_name(), Some("B"));
    assert!(!nodes[0].flags().generated);
    assert_eq!(nodes[0].children().next().unwrap().text(), Some("mantdoc"));
    assert_eq!(nodes[1].text(), Some("welcome"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macro_generated_man_controls_use_the_invocation_control_column() {
    let name = SourceName::new("generated-man-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH GENERATED 1\n.de list\n.TP 6n\ntag\n..\n.list\ntext\n",
        ))
        .unwrap();
    let term = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
        .expect("generated TP block");
    let position = report
        .document
        .source_position(term.location().expect("TP location"))
        .expect("TP source position");
    assert_eq!((position.line, position.column), (6, 2));
    let head = term.children().next().expect("TP head");
    let width = head.children().next().expect("TP width argument");
    let width_position = report
        .document
        .source_position(width.location().expect("width location"))
        .expect("width source position");
    assert_eq!((width_position.line, width_position.column), (6, 5));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macros_can_invoke_nested_macros_with_their_own_arguments() {
    let name = SourceName::new("nested-macros.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de inner\ninner: \\$1\n..\n.de outer\n.inner \\$1\n..\n.outer mantdoc\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("inner: mantdoc"));
    assert!(!nodes[0].flags().generated);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_recursive_macros_reparse_delayed_register_and_argument_escapes() {
    let name = SourceName::new("recursive-macro.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de count\n. ie \\\\$1>0 \\{\\\n.  No \\\\$1\n.  nr next \\\\$1-1\n.  count \\\\n[next]\n. \\}\n..\n.count 3\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["3", "2", "1"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn m3_macro_shift_return_and_argument_count_are_frame_local() {
    let name = SourceName::new("macro-control-flow.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de inner\ninner \\$1 \\n[.$]\n.return\ninner-never\n..\n.de outer\nouter-before \\$1 \\$2\n.shift\n.inner \\$1\nouter-after \\$1\n.return\nouter-never\n..\n.outer one two\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        ["outer-before one two", "inner two 1", "outer-after two"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn shift_recovers_outside_calls_and_invalid_macro_selectors() {
    let name = SourceName::new("shift-recovery.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH SHIFT-RECOVERY 1 \"August 26, 2026\"\n.SH NAME\nshift-recovery - shift validation\n.SH DESCRIPTION\n.shift\n.de mym\nselector: \"\\\\$x\"\n.shift bad\nafter invalid: \"\\\\$1\"\n.shift 2\nafter excessive: \"\\\\$1\"\n..\n.mym one two\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (Severity::Error, "ignoring request outside macro: shift"),
            (Severity::Error, "argument number is not numeric: \\$x"),
            (
                Severity::Error,
                "argument is not numeric, using 1: shift bad"
            ),
            (Severity::Error, "excessive shift: 2, but max is 1"),
        ]
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"after invalid: \"two\""), "{text:#?}");
    assert!(text.contains(&"after excessive: \"\""), "{text:#?}");
    assert!(!text.iter().any(|value| value.contains("$x")), "{text:#?}");
}

#[test]
fn empty_while_scope_keeps_validator_order_and_logical_blank_location() {
    let name = SourceName::new("while-empty-scope.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt WHILE-EMPTY 1\n.Os\n.Sh NAME\n.Nm while-empty\n.Nd test\n.Sh DESCRIPTION\nbefore\n.nr cnt 2 1\n.while \\n-[cnt]\n\\n[cnt]\n.Pp\nfinal text\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "conditional request controls empty scope: while",
            "blank line in fill mode, using .sp",
            "conditional request controls empty scope: while",
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(10, 2), (10, 9), (10, 2)]
    );
}

#[test]
fn roff_return_and_argument_escapes_outside_macros_are_errors() {
    let name = SourceName::new("return-outside.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".return\noutside \\$1\n.return\n"))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                "ignoring request outside macro: return",
            ),
            (
                DiagnosticCode::ROFF_MACRO_ARGUMENT_OUTSIDE,
                "using macro argument outside macro: \\$1",
            ),
            (
                DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                "ignoring request outside macro: return",
            ),
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(1, 2), (2, 9), (3, 2)]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["outside "]
    );
}

#[test]
fn m3_macro_depth_limit_returns_a_coherent_prefix() {
    let name = SourceName::new("macro-depth.roff").unwrap();
    let limits = Limits {
        max_macro_depth: 1,
        ..Limits::default()
    };
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse(Source::new(
        &name,
        b".de second\nsecond\n..\n.de first\nfirst-text\n.second\n..\n.first\n",
    ))
    .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("first-text"));
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.macro-depth")
    );
}

#[test]
fn m3_resolved_includes_preserve_order_source_maps_and_session_state() {
    let root = SourceName::new("root.roff").unwrap();
    let mut bundle = SourceBundle::default();
    bundle
        .insert(
            "part.roff",
            b"inside \\*[word]\n.ds word changed\n".to_vec(),
        )
        .unwrap();
    let report = Parser::default()
        .parse_with_resolver(
            Source::new(
                &root,
                b".ds word welcome\n.so part.roff\noutside \\*[word]\n",
            ),
            &mut bundle,
        )
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].text(), Some("inside welcome"));
    assert_eq!(nodes[1].text(), Some("outside changed"));
    assert_eq!(report.document.source_count(), 2);
    let child_span = nodes[0].location().unwrap();
    assert_eq!(
        report
            .document
            .source_name(child_span.source)
            .map(SourceName::as_str),
        Some("part.roff")
    );
    assert_eq!(report.statistics.source_files, 2);
    assert_eq!(
        report.statistics.source_bytes,
        b".ds word welcome\n.so part.roff\noutside \\*[word]\n".len()
            + b"inside \\*[word]\n.ds word changed\n".len()
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_include_cycles_and_missing_targets_are_recoverable() {
    let root = SourceName::new("root.roff").unwrap();
    let mut bundle = SourceBundle::default();
    bundle.insert("root.roff", b"ignored".to_vec()).unwrap();
    bundle
        .insert("part.roff", b".so root.roff\n".to_vec())
        .unwrap();
    let cyclic = Parser::default()
        .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
        .unwrap();
    assert_eq!(cyclic.document.source_count(), 2);
    assert!(cyclic.statistics.truncated);
    assert!(
        cyclic
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "roff.include-cycle")
    );

    let missing = Parser::default()
        .parse(Source::new(&root, b".so missing.roff\n"))
        .unwrap();
    assert_eq!(missing.document.node_count(), 1);
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "roff.include-unavailable")
    );
}

#[test]
fn m3_include_graph_limits_stop_before_source_map_mutation() {
    let root = SourceName::new("root.roff").unwrap();
    let limits = Limits {
        max_sources: 1,
        ..Limits::default()
    };
    let mut bundle = SourceBundle::new(limits.clone());
    bundle.insert("part.roff", b"child\n".to_vec()).unwrap();
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
    .unwrap();
    assert_eq!(report.document.source_count(), 1);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.sources")
    );
}

#[test]
fn m3_include_depth_and_child_source_bounds_are_diagnostic_not_fatal() {
    let root = SourceName::new("root.roff").unwrap();
    let mut bundle = SourceBundle::default();
    bundle
        .insert("first.roff", b".so second.roff\n".to_vec())
        .unwrap();
    bundle.insert("second.roff", b"second\n".to_vec()).unwrap();
    let depth_limited = Parser::new(ParserConfig {
        limits: Limits {
            max_include_depth: 1,
            ..Limits::default()
        },
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so first.roff\n"), &mut bundle)
    .unwrap();
    assert_eq!(depth_limited.document.source_count(), 2);
    assert!(
        depth_limited
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.include-depth")
    );

    let mut bytes_bundle = SourceBundle::default();
    bytes_bundle
        .insert("large.roff", b"this child is too large\n".to_vec())
        .unwrap();
    let byte_limited = Parser::new(ParserConfig {
        limits: Limits {
            max_root_source_bytes: 16,
            max_total_source_bytes: 64,
            ..Limits::default()
        },
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so large.roff\n"), &mut bytes_bundle)
    .unwrap();
    assert_eq!(byte_limited.document.source_count(), 1);
    assert!(
        byte_limited
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.source-bytes")
    );
}

#[test]
fn m3_include_diagnostics_share_the_session_budget() {
    let root = SourceName::new("root.roff").unwrap();
    let limits = Limits {
        max_diagnostics: 1,
        ..Limits::default()
    };
    let mut bundle = SourceBundle::new(limits.clone());
    bundle
        .insert("part.roff", b".so missing-a\n.so missing-b\n".to_vec())
        .unwrap();
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
    .unwrap();
    assert_eq!(report.document.source_count(), 2);
    assert_eq!(report.diagnostics.len(), 1);
    assert!(report.statistics.truncated);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        "roff.include-unavailable"
    );
}

#[test]
fn m3_while_rechecks_register_conditions_and_updates_session_state() {
    let name = SourceName::new("while.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 0\n.while \\n[count]<3 .nr count +1\ncount \\n[count]\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("count 3"));
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_while_executes_a_copy_mode_macro_body_on_each_iteration() {
    let name = SourceName::new("while-macro.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2 1\n.de decrement\n\\\\n-[count]\n..\n.while \\n[count] .decrement\ncount \\n[count]\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["1", "0", "count 0"]);
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_active_inline_conditionals_execute_environment_requests() {
    let name = SourceName::new("conditional-request.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 .ds selected yes\n.if 0 .ds selected no\n\\*[selected]\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("yes"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_while_aggregate_limit_stops_environment_updates() {
    let name = SourceName::new("while-limit.roff").unwrap();
    let limits = Limits {
        max_loop_iterations: 2,
        max_total_loop_iterations: 3,
        ..Limits::default()
    };
    let report = Parser::new(ParserConfig {
            limits,
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".nr first 0\n.while \\n[first]<2 .nr first +1\n.nr second 0\n.while \\n[second]<2 .nr second +1\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(text.is_empty());
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.total-loop-iterations")
    );
}

#[test]
fn m3_while_per_loop_limit_returns_the_generated_prefix() {
    let name = SourceName::new("while-per-loop-limit.roff").unwrap();
    let limits = Limits {
        max_loop_iterations: 2,
        max_total_loop_iterations: 3,
        ..Limits::default()
    };
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse(Source::new(&name, b".while 1 repeated\n"))
    .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["repeated", "repeated"]);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.loop-iterations")
    );
}

#[test]
fn m3_numeric_and_nroff_conditionals_choose_only_the_active_inline_branch() {
    let name = SourceName::new("conditionals.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 7\n.if 1 visible\n.if 0 hidden\n.if !0 inverted\n.if n nroff\n.if t troff\n.if \\n[count]>=7 registered\n.if \\n[count]!=7 wrong\n.ie 0 first\n.el second\n",
            ))
            .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        nodes,
        ["visible", "inverted", "nroff", "registered", "second"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn number_registers_accept_whitespace_inside_parenthesized_values() {
    let name = SourceName::new("register-parenthesized-space.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr value 18\n.nr value ( 25 - 6 )\n\\n[value]\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .find_map(|node| node.text().map(str::to_owned));
    assert_eq!(text.as_deref(), Some("19"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn number_register_division_by_zero_recovers_to_zero_and_reports_the_request() {
    let name = SourceName::new("division-by-zero.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr quotient 1/0\n.nr remainder 1%0\n\\n[quotient] \\n[remainder]\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.severity,
                    diagnostic.message.as_ref(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                Severity::Error,
                "divide by zero: 1/0",
            ),
            (
                DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                Severity::Error,
                "divide by zero: 1%0",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(1, 4), (2, 4)]);
    assert_eq!(
        report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["0 0"]
    );
    assert!(!report.statistics.truncated);
}

#[test]
fn ignore_blocks_report_excess_markers_unmatched_ends_and_eof() {
    let name = SourceName::new("ignore-blocks.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ig end excess\nignored\n.end\n..\n.ig\nignored\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.severity,
                    diagnostic.message.as_ref(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                Severity::Error,
                "skipping excess arguments: .ig ... excess",
            ),
            (
                DiagnosticCode::ROFF_UNMATCHED_END,
                Severity::Error,
                "skipping end of block that is not open: ..",
            ),
            (
                DiagnosticCode::ROFF_UNCLOSED_IGNORE,
                Severity::Error,
                "appending missing end of block: ig",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(1, 5), (4, 2), (5, 2)]);
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.text() != Some("ignored"))
    );
}

#[test]
fn input_traps_require_a_numeric_prefix_without_replacing_the_existing_trap() {
    let name = SourceName::new("input-trap-arguments.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de trap\ntrapped\n..\n.it 2 trap\n.it trap\nfirst\nsecond\n.it\nthird\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.severity,
                    diagnostic.message.as_ref(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_NON_NUMERIC_ARGUMENT,
                Severity::Error,
                "skipping request without numeric argument: it trap",
            ),
            (
                DiagnosticCode::ROFF_NON_NUMERIC_ARGUMENT,
                Severity::Error,
                "skipping request without numeric argument: it",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(5, 2), (8, 2)]);
    assert_eq!(
        report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["first", "second", "trapped", "third"]
    );
}

#[test]
fn macro_rename_requires_a_space_after_the_old_name() {
    let name = SourceName::new("rename-tab.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de old\nold body\n..\n.rn old\tnew\n.new\n.old\n.rn old new\tignored\n.old\n.new\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["old body", "old body"]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping unknown macro: .new",
            "skipping unknown macro: .old",
        ]
    );
}

#[test]
fn removed_user_macro_is_reported_without_hiding_removed_string_references() {
    let name = SourceName::new("remove-macro.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de old\nold body\n..\n.ds value text\n.rm old value\n.old\n\\*[value]\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping unknown macro: .old",
            "undefined string, using \"\": value",
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(6, 2), (7, 1)]);
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("old"))
    );
}

#[test]
fn user_macro_tabs_preserve_argument_prefixes_and_defer_validation() {
    let name = SourceName::new("macro-tabs.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH MACRO-TABS 1\n.SH DESCRIPTION\n.de show end ignored\nvalue \\\\$1;\\\\$2\n.end\n.show\t\ttwo\n.show\t\t\t three\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::ROFF_ALL_ARGUMENTS,
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(3, 5), (6, 8), (7, 8)]
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"value \ttwo;"));
    assert!(text.contains(&"value \t;three"));
}

#[test]
fn register_request_names_reject_escaped_characters_but_keep_literal_backslashes() {
    let name = SourceName::new("register-escaped-name.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr first\\\\second 1\n.nr first\\esecond 2\n.rr first\\esecond\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_ESCAPED_NAME,
                "escaped character not allowed in a name: first\\e",
            ),
            (
                DiagnosticCode::ROFF_ESCAPED_NAME,
                "escaped character not allowed in a name: first\\e",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(2, 5), (3, 5)]);
}

#[test]
fn macro_names_recover_literal_and_prohibited_escapes_consistently() {
    let name = SourceName::new("macro-escaped-name.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de second\nsecond\n..\n.de first\\\\second\nliteral\n..\n.de first\\esecond\nfirst\n..\n.first\n.second\n.first\\\\second\n.rm first\\\\second first\\esecond second\n.first\n.second\n.first\\\\second\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["first", "second", "literal", "second"]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_ESCAPED_NAME,
                "escaped character not allowed in a name: first\\e",
            ),
            (
                DiagnosticCode::ROFF_ESCAPED_NAME,
                "escaped character not allowed in a name: first\\e",
            ),
            (
                DiagnosticCode::ROFF_UNKNOWN_MACRO,
                "skipping unknown macro: .first"
            ),
            (
                DiagnosticCode::ROFF_UNKNOWN_MACRO,
                "skipping unknown macro: .first\\\\second",
            ),
        ]
    );
}

#[test]
fn unterminated_bracketed_register_reference_keeps_legacy_diagnostics() {
    let name = SourceName::new("register-unterminated.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH REGISTER 1\n.SH DESCRIPTION\nincomplete: \\n[second\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ESCAPE_INVALID,
                "invalid escape sequence: \\n[second",
            ),
            (
                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                "whitespace at end of input line",
            ),
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(3, 13), (3, 12)]
    );
    assert!(
        report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .any(|text| text == "incomplete:")
    );
}

#[test]
fn unterminated_delimited_escape_keeps_the_authored_diagnostic_spelling() {
    let name = SourceName::new("unterminated-width.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH WIDTH 1\n.SH DESCRIPTION\nunterminated: \\w'foo\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        (
            report.diagnostics[0].code.as_str(),
            report.diagnostics[0].message.as_ref(),
        ),
        (
            DiagnosticCode::ESCAPE_UNTERMINATED,
            "invalid escape sequence: \\w'foo",
        )
    );
}

#[test]
fn ignored_escape_forms_keep_only_the_legacy_invalid_diagnostics() {
    let name = SourceName::new("ignored-escapes.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            br".TH ESC-IGNORE 1
.SH NAME
esc-ignore \- ignored roff escape sequences
.SH DESCRIPTION
.nf
closing parenthesis: a\)b\[)]c
comma: a\,b\[,]c
slash: a\/b\[/]c
multiform: a\kxb\k(xyc\k[xyz]d
quoted: a\R'myreg 0'b\R'myreg \A'y'0'c
sizes: a\s0b\s(12c\s[123]d\s'123'e\s'1\w'xy'2'f
signed sizes: a\s-0b\s-(12c\s-[123]d\s-'123'e\s-'1\w'xy'2'f\s-
",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "invalid escape sequence: \\[)]",
            "invalid escape sequence: \\[,]",
            "invalid escape sequence: \\[/]",
            "invalid escape sequence: \\s-",
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(6, 26), (7, 12), (8, 12), (12, 60)]
    );
}

#[test]
fn invalid_bracket_escapes_are_reported_before_their_raw_form() {
    let name = SourceName::new("invalid-escapes.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            br".TH ESC-INVALID 1
.SH NAME
esc-invalid \- invalid roff escape sequences
.SH DESCRIPTION
.nf
plus: a\+b\[+]c
unicode: a\Ub\[U]c
",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "invalid escape sequence: \\[+]",
            "undefined escape, printing literally: \\+",
            "invalid escape sequence: \\[U]",
            "undefined escape, printing literally: \\U",
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(6, 11), (6, 8), (7, 14), (7, 11)]
    );
}

#[test]
fn m3_inline_conditional_body_keeps_its_authored_provenance_and_offset() {
    let name = SourceName::new("conditional-location.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".ie 1 body\n"))
        .unwrap();
    let node = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(node.text(), Some("body"));
    assert!(!node.flags().generated);
    let position = report
        .document
        .source_position(node.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (1, 7));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_register_defined_conditionals_track_rr_without_creating_registers() {
    let name = SourceName::new("register-condition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie rstate unexpected\n.el absent\n.nr state 1\n.ie rstate present\n.el unexpected\n.rr state\n.ie rstate unexpected\n.el removed\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["absent", "present", "removed"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn roff_register_conditionals_keep_the_legacy_name_and_tab_diagnostics() {
    let name = SourceName::new("register-condition-diagnostics.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH REGISTER 1\n.SH DESCRIPTION\n.ie rknown\tvisible\n.el hidden\n.nr known 0\n.ie rknown\\(enignored\n.el hidden\n",
            ))
            .unwrap();
    assert_eq!(report.diagnostics.len(), 2, "{:#?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_ESCAPED_NAME
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "escaped character not allowed in a name: known\\("
    );
    assert_eq!(
        report.diagnostics[1].code.as_str(),
        DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT
    );
    assert_eq!(report.diagnostics[1].message.as_ref(), "tab in filled text");
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(6, 6), (3, 11)]);
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"hidden"), "{text:#?}");
    assert!(text.contains(&"\\(enignored"), "{text:#?}");
}

#[test]
fn roff_renamed_man_macro_remains_defined_for_a_d_condition() {
    let name = SourceName::new("renamed-man-macro-condition.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH RENAMED 1\n.SH DESCRIPTION\n.rn SM renamed\n.ie drenamed visible\n.el hidden\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"visible"), "{text:#?}");
    assert!(!text.contains(&"hidden"), "{text:#?}");
    assert!(report.diagnostics.is_empty());
}

#[test]
fn undefined_string_and_conditioned_macro_recover_as_roff_state() {
    let name = SourceName::new("undefined-name-state.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH UNDEFINED-NAME-STATE 1 \"August 26, 2026\"\n.SH NAME\nundefined-name-state - roff state\n.SH DESCRIPTION\nfirst: \"\\*[missing]\"\n.ie dmissing string-defined\n.el string-undefined\n.ie dunknown macro-defined\n.el macro-undefined\n.unknown\n.ie dunknown macro-defined-after\n.el macro-undefined-after\n.rn BR newBR\n.newBR works\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (Severity::Warning, "undefined string, using \"\": missing"),
            (Severity::Error, "skipping unknown macro: .unknown"),
        ]
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"string-defined"), "{text:#?}");
    assert!(text.contains(&"macro-undefined"), "{text:#?}");
    assert!(text.contains(&"macro-defined-after"), "{text:#?}");
    let renamed_argument = report
        .document
        .preorder()
        .find(|node| node.text() == Some("works"))
        .unwrap();
    let position = report
        .document
        .source_position(renamed_argument.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (14, 5));
}

#[test]
fn man_unknown_roff_font_is_removed_at_the_request_and_reports_its_macro() {
    let name = SourceName::new("unknown-man-font.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH FONT 1\n.SH DESCRIPTION\n.ft foo\nvisible\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_UNKNOWN_FONT
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "unknown font, skipping request: ft foo"
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (3, 2));
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("ft"))
    );
}

#[test]
fn man_roff_font_request_keeps_only_its_first_selector() {
    let name = SourceName::new("man-font-selector.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH FONT 1\n.SH DESCRIPTION\n.ft I surplus\n.ft\nvisible\n",
        ))
        .unwrap();
    let font = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("ft"))
        .unwrap();
    assert_eq!(
        font.children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("I")]
    );
    assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_EXCESS_ARGUMENTS
    );
    let default_font = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("ft"))
        .nth(1)
        .unwrap();
    assert_eq!(
        default_font
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("P")]
    );
}

#[test]
fn m3_string_and_macro_defined_conditionals_accept_the_two_token_form() {
    let name = SourceName::new("defined-condition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie d phrase unexpected\n.el absent\n.ds phrase value\n.ie d phrase string\n.el unexpected\n.if !d phrase unexpected\n.de macro\nbody\n..\n.ie d macro macro\n.el unexpected\n.ie d PP builtin\n.el unexpected\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["absent", "string", "macro", "builtin"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_delimited_string_conditions_handle_match_mismatch_and_malformed_input() {
    let name = SourceName::new("string-compare.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie \"\"\" empty\n.el unexpected\n.ie xabcxabcx equal\n.el unexpected\n.ie xabcxabdx unexpected\n.el mismatch\n.ie xabc unexpected\n.el malformed\n.ie !xabcxabcx unexpected\n.el negated\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["empty", "equal", "mismatch", "malformed", "negated"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_numeric_conditions_compare_physical_units_and_boolean_operators() {
    let name = SourceName::new("numeric-condition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie 42 positive\n.el unexpected\n.ie !42 unexpected\n.el negated\n.ie -42 unexpected\n.el negative\n.ie !-42 negated-negative\n.el unexpected\n.ie 42=bad unexpected\n.el incomplete\n.ie 1&1 both\n.el unexpected\n.ie 1&0 unexpected\n.el and-false\n.ie 0:1 either\n.el unexpected\n.ie 1i>2c physical\n.el unexpected\n.ie 1i-6P unexpected\n.el zero\n.ie ( unexpected\n.el bare-open\n.ie !( unexpected\n.el negated-bare-open\n.ie (1 open\n.el unexpected\n.ie !(0 negated-open\n.el unexpected\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "positive",
            "negated",
            "negative",
            "negated-negative",
            "incomplete",
            "both",
            "and-false",
            "either",
            "physical",
            "zero",
            "bare-open",
            "negated-bare-open",
            "open",
            "negated-open",
        ]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_multiline_conditional_scopes_use_the_explicit_execution_stack() {
    let name = SourceName::new("conditional-scope.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if n \\{\\\nouter\n.if t \\{\\\nhidden\n.\\}\n.if n \\{\\\ninner\n.\\}\n.\\}\n.if t \\{\\\nskipped\n.\\}\n.ie n \\{\\\ntrue-branch\n.\\}\n.el \\{\\\nwrong-branch\n.\\}\n.ie t \\{\\\nwrong-branch\n.\\}\n.el \\{\\\nelse-branch\n.\\}\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["outer", "inner", "true-branch", "else-branch"]);
    let outer = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    let position = report
        .document
        .source_position(outer.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (2, 9));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_multiline_while_scope_preserves_its_opener_column() {
    let name = SourceName::new("while-scope.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 1\n.while \\n[count] \\{\\\nbody\n.nr count 0\n.\\}\n",
        ))
        .unwrap();
    let node = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(node.text(), Some("body"));
    let position = report
        .document
        .source_position(node.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (3, 20));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_continue_skips_to_the_nearest_explicit_loop_frame() {
    let name = SourceName::new("continue.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr remaining 3\n.while \\n[remaining] \\{\\\n.nr remaining -1\n.if \\n[remaining]=1 \\{\\\n.continue\n.\\}\nkept \\n[remaining]\n.\\}\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["kept 2", "kept 0"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_empty_ie_predicates_consume_their_next_line_before_selecting_else() {
    let name = SourceName::new("empty-ie.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie\ntext-after-empty\n.el empty-else\n.ie !\ntext-after-negated-empty\n.el negated-empty-else\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["empty-else", "negated-empty-else"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_bare_ie_leaves_an_immediate_else_as_its_paired_branch() {
    let name = SourceName::new("bare-ie-else.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".ie 0\n.el selected\n"))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["selected"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_conditional_text_preserves_literal_escape_before_a_brace() {
    let name = SourceName::new("ie-literal-brace.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ie n If \\&.el\\e{ works, nothing follows here:\n.el\\{dummy\nBOOHOO\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["If .el\\{ works, nothing follows here:"]);
}

#[test]
fn m3_conditional_scope_closes_after_a_control_request() {
    let name = SourceName::new("ie-control-closer.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ie n \\{\\\nactive branch\n.br\\}\n.el \\{\\\ninactive branch\n.br\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["active branch"]);
}

#[cfg(feature = "render")]
#[test]
fn conditional_scope_closer_suffix_keeps_terminal_inline_provenance() {
    let name = SourceName::new("conditional-scope-suffix.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\npreceding words\n.if n \\{text line block end\n\\} with additional words\nfollowing words\n",
            ))
            .unwrap();
    let suffix = report
        .document
        .preorder()
        .find(|node| {
            node.text()
                .is_some_and(|text| text.contains("additional words"))
        })
        .expect("scope-closer suffix must remain visible");
    assert!(suffix.terminal_inline_conditional());
}

#[test]
fn m3_control_scope_closer_discards_following_text() {
    let name = SourceName::new("control-scope-closer-suffix.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{\\\nfirst line\n.\\}suffix must not print\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["DESCRIPTION", "first line"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_nested_text_closers_remain_in_the_active_inner_scope() {
    let name = SourceName::new("nested-text-closers.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{outer\n.if n \\{inner\non\\} the\\} same\nafter\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        [
            "DESCRIPTION",
            "outer",
            "inner",
            "on\\& the\\& same",
            "after"
        ]
    );
}

#[test]
fn m3_attached_font_scope_closers_keep_font_arguments_and_diagnostic() {
    let name = SourceName::new("attached-font-closers.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{outer\n.if n \\{inner\n.BR\\}on\\}the same\nafter\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .first()
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref())),
        Some((
            Severity::Error,
            "escaped character not allowed in a name: BR\\&"
        ))
    );
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("BR"))
        .unwrap();
    assert_eq!(
        macro_node
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["on\\&the", "same"]
    );
}

#[test]
fn m3_unterminated_conditional_scope_reports_its_opener_and_executes_prefix() {
    let name = SourceName::new("unterminated-condition.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n \\{\nstill open\n",
        ))
        .unwrap();
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::ROFF_UNTERMINATED_SCOPE)
        .unwrap();
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.message.as_ref(),
        "appending missing end of block: if"
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("still open"))
    );
}

#[test]
fn m3_nonstandard_brace_scopes_retain_the_same_line_body_at_every_depth() {
    let name = SourceName::new("nonstandard-brace-scopes.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\nouter\n.if 1 \\{inner\n\\}\n.\\}\n.nr count 1\n.while \\n[count] \\{first\n.nr count -1\n\\}\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["outer", "inner", "first"]);
}

#[test]
fn m3_nested_scope_closers_share_a_control_line_without_leaking_frames() {
    let name = SourceName::new("nested-control-closers.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{outer\n.if 1 \\{inner\n.\\}middle\\}end\nafter\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["outer", "inner", "after"]);
}

#[test]
fn m3_nested_ie_else_scopes_keep_the_eligible_branch_in_the_same_frame() {
    let name = SourceName::new("nested-ie-else-scopes.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.ie 0 \\{\\\ninactive\n.\\}\n.el \\{\\\nactive\n.\\}\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["active"]);
}

#[test]
fn m3_collected_scopes_define_direct_and_indirect_copy_mode_macros() {
    let name = SourceName::new("scope-copy-mode-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds indirect appended\n.de direct\nfirst\n..\n.if 1 \\{\\\n.am direct\nsecond\n..\n.dei indirect\nthird\n..\n.de custom finish\ncustom marker\n.finish\n.\\}\n.direct\n.appended\n.custom\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["first", "second", "third", "custom marker"]);
}

#[test]
fn m3_conditional_macro_definitions_discard_terminator_tails_and_inactive_definitions() {
    let name = SourceName::new("conditional-definition-tails.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL-DEFINITION 1\n.SH DESCRIPTION\n.if n \\{.de first\nfirst content\n.. \\}\n.if n \\{.de second\nsecond content\n.. \\}ignored\n.if t \\{.de suppressed\nnot visible\n.. \\}ignored\ninitial text\n.first\n.second\n.suppressed\nfinal text\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        [
            "DESCRIPTION",
            "initial text",
            "first content",
            "second content",
            "final text"
        ]
    );
    assert_eq!(report.diagnostics.len(), 2, "{:#?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_ALL_ARGUMENTS
    );
    assert_eq!(
        report.diagnostics[1].code.as_str(),
        DiagnosticCode::ROFF_UNKNOWN_MACRO
    );
}

#[test]
fn m3_collected_scope_definitions_preserve_nested_ie_else_copy_mode() {
    let name = SourceName::new("scope-copy-mode-nested-ie.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\n.de emit\n.ie 0 \\{\\\nskipped\n.\\}\n.el \\{\\\nselected\n.\\}\n..\n.\\}\n.emit\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["selected"]);
}

#[test]
fn m3_inline_ie_else_inside_a_loop_scope_selects_only_the_eligible_body() {
    let name = SourceName::new("inline-ie-else-in-loop.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 1\n.while \\n[count] \\{\\\n.ie 0 skipped\n.el kept\n.nr count -1\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["kept"]);
}

#[test]
fn m3_inline_if_inside_a_loop_scope_dispatches_a_macro_body() {
    let name = SourceName::new("inline-if-macro-in-loop.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de emit\nfrom macro\n..\n.nr count 1\n.while \\n[count] \\{\\\n.if 1 .emit\n.nr count -1\n.\\}\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["from macro"]);
}

#[test]
fn m3_top_level_inline_if_dispatches_a_macro_body() {
    let name = SourceName::new("inline-if-macro.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de emit\nfrom macro: \\$1\n..\n.if n .emit argument\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["from macro: argument"]);
}

#[test]
fn m3_inline_if_inside_a_loop_scope_dispatches_translation_requests() {
    let name = SourceName::new("inline-if-translation-in-loop.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 1\n.while \\n[count] \\{\\\n.if 1 .tr xy\nx\n.nr count -1\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["y"]);
}

#[test]
fn m3_collected_scopes_reclassify_requests_after_a_dynamic_control_change() {
    let name = SourceName::new("scope-dynamic-control.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.cc !\n!ds word dynamic\n!cc .\n\\*[word]\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["dynamic"]);
}

#[test]
fn m3_inactive_collected_scopes_do_not_leak_dynamic_control_changes() {
    let name = SourceName::new("inactive-scope-dynamic-control.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 0 \\{\\\n.cc !\n!ds word hidden\n.\\}\n.ds word outside\n\\*[word]\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["outside"]);
}

#[test]
fn m3_collected_scopes_close_with_a_delayed_escape_character() {
    let name = SourceName::new("scope-dynamic-escape.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.ec @\n@}\n.ds word after\n@*[word]\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["after"]);
}

#[test]
fn m3_scope_macros_execute_their_own_while_brace_frames() {
    let name = SourceName::new("scope-macro-while.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de emit\n.nr count 1\n.while \\n[count] \\{\\\ninside\n.nr count -1\n.\\}\n..\n.if 1 \\{\\\n.emit\n.\\}\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["inside"]);
}

#[test]
fn m3_scope_macro_while_frames_share_the_session_loop_budget() {
    let name = SourceName::new("scope-macro-while-limit.roff").unwrap();
    let report = Parser::new(ParserConfig {
            limits: Limits {
                max_loop_iterations: 2,
                max_total_loop_iterations: 2,
                ..Limits::default()
            },
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".de emit\n.nr count 3\n.while \\n[count] \\{\\\ninside\n.nr count -1\n.\\}\n..\n.if 1 \\{\\\n.emit\n.\\}\n",
        ))
        .unwrap();
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.loop-iterations")
    );
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["inside", "inside"]);
}

#[test]
fn m3_parallel_sessions_do_not_share_delayed_environment_definitions() {
    let workers = ["alpha", "beta", "gamma", "delta"]
        .into_iter()
        .map(|word| {
            std::thread::spawn(move || {
                let name = SourceName::new(format!("{word}.roff")).unwrap();
                let source = format!(".ds word {word}\n\\*[word]\n");
                let report = Parser::default()
                    .parse(Source::new(&name, source.as_bytes()))
                    .unwrap();
                report
                    .document
                    .preorder()
                    .filter(|node| node.kind() == NodeKind::Text)
                    .filter_map(crate::NodeRef::text)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let observed = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        [
            vec!["alpha".to_owned()],
            vec!["beta".to_owned()],
            vec!["gamma".to_owned()],
            vec!["delta".to_owned()]
        ]
    );
}

#[test]
fn m3_tr_translates_visible_text_without_rewriting_escape_spellings() {
    let name = SourceName::new("translation.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".tr xy\nx \\(em\n.tr z\nz\n.tr \\(emw\n\\(em\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["y —", " ", "w"]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::ROFF_ODD_TRANSLATION,
            "odd number of characters in request: tr z"
        )]
    );
}

#[test]
fn m3_tr_inside_a_loop_scope_affects_later_scope_text() {
    let name = SourceName::new("scope-translation.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".while 1 \\{\\\n.tr xy\nx\n.break\n.\\}\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["y"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_tr_inside_a_scope_macro_affects_later_macro_text() {
    let name = SourceName::new("macro-translation.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de translate\n.tr xy\nx\n..\n.while 1 \\{\\\n.translate\n.break\n.\\}\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["y"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_tr_inside_a_top_level_macro_affects_later_macro_text() {
    let name = SourceName::new("top-level-macro-translation.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de translate\n.tr xy\nx\n..\n.translate\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["y"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macro_control_accepts_horizontal_space_after_the_control_character() {
    let name = SourceName::new("macro-control-space.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 0\n.de increment\n.  nr count +1\n..\n.increment\n.if \\n[count]=1 updated\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["updated"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macro_opened_while_consumes_and_replays_following_physical_scope() {
    let name = SourceName::new("macro-opened-while.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2\n.de loop\n. while \\\\n[count] \\{\\\n..\n.loop\nvalue \\n[count]\n. nr count -1\n.\\}\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["value 2"]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
            DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
        ]
    );
}

#[test]
fn retained_comments_are_not_visible_line_start_nodes() {
    let name = SourceName::new("comment-flags.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".\\\" source comment\nvisible text\n"))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let comment = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Comment)
        .unwrap();
    assert!(!comment.flags().no_print);
    assert!(!comment.flags().line_start);
    let text = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Text)
        .unwrap();
    assert!(text.flags().line_start);
}

#[test]
fn escaped_comment_control_is_skipped_with_a_style_diagnostic() {
    let name = SourceName::new("escaped-comment-control.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b"\\.\"\n"))
        .unwrap();
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.kind() != NodeKind::Text)
    );
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::INPUT_BAD_COMMENT_STYLE)
        .unwrap();
    assert_eq!(diagnostic.severity, Severity::Style);
    let position = diagnostic
        .primary
        .as_ref()
        .and_then(|span| report.document.source_position(span))
        .unwrap();
    assert_eq!((position.line, position.column), (1, 3));
}

#[test]
fn physical_line_continuation_keeps_quoted_control_arguments_together() {
    let name = SourceName::new("continued-ip.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CONTINUED 1\n.SH DESCRIPTION\n.IP \"a long \\\ncontinued \\\nterm\" 4n\nbody\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty());
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("IP"))
        .unwrap();
    assert_eq!(
        head.children().next().and_then(crate::NodeRef::text),
        Some("a long continued term")
    );
}

#[test]
fn terminal_package_macro_continuation_retains_completed_arguments() {
    let name = SourceName::new("terminal-continued.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH TERMINAL 1\n.SH DESCRIPTION\n.IB one two\\",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let element = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("IB"))
        .unwrap();
    assert_eq!(
        element
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
}

#[test]
fn mdoc_package_quote_recovery_keeps_the_argument_and_orders_its_tail_warning() {
    let name = SourceName::new("mdoc-quote-recovery.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt QUOTE 1\n.Os\n.Sh NAME\n.Nm quote\n.Nd recovery\n.Fl \"one \n",
        ))
        .unwrap();
    let element = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Fl"))
        .unwrap();
    assert_eq!(
        element
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["one "]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
        ]
    );
}

#[test]
fn man_next_line_conditions_materialize_a_vertical_boundary() {
    let name = SourceName::new("man-condition-boundaries.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CONDITIONAL 1\n.SH DESCRIPTION\n.if n First sentence.\n.if n\nSecond sentence.\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let first = nodes
        .iter()
        .position(|node| node.text() == Some("First sentence."))
        .unwrap();
    assert_eq!(nodes[first + 1].macro_name(), Some("sp"));
    assert_eq!(nodes[first + 2].text(), Some("Second sentence."));
}

#[test]
fn escaped_deferred_references_do_not_become_public_warnings() {
    let name = SourceName::new("deferred-reference.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH DEFERRED 1\n.SH DESCRIPTION\n.ds value used\n.IB prefix ##\\\\*[value]## suffix\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty());
}

#[test]
fn legacy_unicode_escape_uses_the_legacy_public_diagnostic_message() {
    let name = SourceName::new("legacy-unicode.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH LEGACY-UNICODE 1\n.SH DESCRIPTION\naccent: e\\U'0301'\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        "escape.unsupported-unicode"
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "undefined escape, printing literally: \\U"
    );
}

#[test]
fn bracketed_accent_spelling_preserves_legacy_invalid_escape_findings() {
    let name = SourceName::new("invalid-bracket-accent.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH INVALID-BRACKET-ACCENT 1\n.SH DESCRIPTION\nacute e\\[']e\ngrave e\\[`]e\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 2);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "invalid escape sequence: \\[']",
            "invalid escape sequence: \\[`]"
        ]
    );
}

#[test]
fn bracketed_whitespace_controls_keep_legacy_invalid_escape_findings() {
    let name = SourceName::new("invalid-bracket-whitespace.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INVALID-BRACKET-WHITESPACE 1\n.SH DESCRIPTION\nblank a\\[ hy]b\npercent a\\[%]b\nampersand a\\[&]b\ncolon a\\[:]b\ncaret a\\[^]b\nunderline a\\[_]b\npipe a\\[|]b\ntilde a\\[~]b\ndigit a\\[0]b\n",
            ))
            .unwrap();
    assert_eq!(report.diagnostics.len(), 9);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code.as_str() == DiagnosticCode::ESCAPE_INVALID })
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "invalid escape sequence: \\[",
            "invalid escape sequence: \\[%]",
            "invalid escape sequence: \\[&]",
            "invalid escape sequence: \\[:]",
            "invalid escape sequence: \\[^]",
            "invalid escape sequence: \\[_]",
            "invalid escape sequence: \\[|]",
            "invalid escape sequence: \\[~]",
            "invalid escape sequence: \\[0]",
        ]
    );
}

#[test]
fn invalid_bracketed_unicode_scalar_keeps_the_authored_spelling() {
    let name = SourceName::new("invalid-unicode-scalar.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH INVALID-UNICODE-SCALAR 1\n.SH DESCRIPTION\ntext \\[uD800]\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ESCAPE_UNSUPPORTED_UNICODE
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "invalid escape sequence: \\[uD800]"
    );
}

#[test]
fn malformed_unicode_escape_diagnostics_use_legacy_order_and_position() {
    let name = SourceName::new("invalid-unicode-shape.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH INVALID-UNICODE-SHAPE 1\n.SH DESCRIPTION\ntext \\[u2B].\\[u02B]\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "invalid escape sequence: \\[u02B]",
            "invalid escape sequence: \\[u2B]",
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| position.column)
            .collect::<Vec<_>>(),
        [13, 6]
    );
}

#[test]
fn zero_width_escape_retains_its_following_no_space_escape_in_package_ast() {
    let name = SourceName::new("zero-width-escape.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH ZERO 1\n.SH DESCRIPTION\nzero width: \\z\\c\nfollowing line\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty());
    let text = report
        .document
        .preorder()
        .find(|node| node.text() == Some("zero width: \\z\\c"))
        .unwrap();
    assert!(text.flags().line_continuation);
}
