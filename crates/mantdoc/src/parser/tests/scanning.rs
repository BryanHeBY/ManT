use super::*;

#[test]
fn physical_os_request_detection_distinguishes_absent_and_bare_forms() {
    assert!(crate::parser::source_has_mdoc_operating_system_request(
        b".Os\n"
    ));
    assert!(crate::parser::source_has_mdoc_operating_system_request(
        b".Os OpenBSD\n"
    ));
    assert!(!crate::parser::source_has_mdoc_operating_system_request(
        b".Dt TEST 1\n"
    ));
}

#[test]
fn bare_control_lines_are_empty_requests_not_ast_elements() {
    let name = SourceName::new("empty-request.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH EMPTY 1 28-Aug-2026\n.\n.   \n.SH DESCRIPTION\nbody\n",
        ))
        .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some(""))
    );
    let root = report.document.node(report.document.root()).unwrap();
    assert_eq!(root.children().count(), 1);
}

#[test]
fn tbl_projection_keeps_utf8_and_malformed_byte_origins_distinct() {
    assert_eq!(
        crate::parser::legacy_table_input_text(b"\\[u0080]\xc2\x80"),
        "\\[u0080]\\[u0080]"
    );
    assert_eq!(crate::parser::legacy_table_input_text(b"\xc2x"), "?x");
    assert_eq!(
        crate::parser::legacy_table_input_text(b"\xc2\xc3\x80"),
        "?\\[u00C0]"
    );
}

#[test]
fn m2_scanner_accepts_arbitrary_bytes_without_utf8_replacement() {
    let name = SourceName::new("arbitrary.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".TH TEST 1 28-Aug-2026\n\xff"))
        .unwrap();
    assert_eq!(report.document.macro_set(), MacroSet::Man);
    assert_eq!(
        report
            .document
            .source_name(report.document.root_source())
            .map(crate::SourceName::as_str),
        Some("arbitrary.1")
    );
    assert_eq!(report.statistics.source_bytes, 24);
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
        .parse(Source::new(
            &name,
            b".TH bar-man 1 28-Aug-2026\n.SH DESCRIPTION\nbody\n",
        ))
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
            b".TH TABS 1 28-Aug-2026\n.SH DESCRIPTION\nleft\tright\n",
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
fn filled_text_tabs_are_published_after_the_input_scan() {
    let name = SourceName::new("filled-tab-order.1").unwrap();
    let long_line = b"word word word word word word word word word word word word word word word word word word word\n";
    let mut input = b".TH TABS 1 2026-08-28\n.SH DESCRIPTION\n".to_vec();
    input.extend_from_slice(long_line);
    input.extend_from_slice(b"left\tright\n");
    input.extend_from_slice(long_line);
    let report = Parser::default().parse(Source::new(&name, &input)).unwrap();
    let codes = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        vec![
            DiagnosticCode::INPUT_LINE_TOO_LONG,
            DiagnosticCode::INPUT_LINE_TOO_LONG,
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
        ]
    );
}

#[test]
fn unterminated_quote_points_at_the_unclosed_argument_not_a_prior_quote() {
    let name = SourceName::new("unterminated-quote.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH QUOTE 1 28-Aug-2026\n.SH DESCRIPTION\n.BI \"-x\" \" transliteration\n",
        ))
        .unwrap();
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE)
        .unwrap();
    assert_eq!(diagnostic.message.as_ref(), "unterminated quoted argument");
    let position = diagnostic
        .primary
        .as_ref()
        .and_then(|span| report.document.source_position(span))
        .unwrap();
    assert_eq!((position.line, position.column), (3, 10));
}

#[test]
fn copy_mode_string_tabs_survive_expansion_and_warn_in_filled_text() {
    let name = SourceName::new("string-tab.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH TABS 1 28-Aug-2026\n.SH DESCRIPTION\n.ds value\ttext\n>>\\*[value]<<\n",
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
            b".TH STRING 1 28-Aug-2026\n.SH DESCRIPTION\n>>>\\*[missing]<<<\n",
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
            b".TH STRING 1 28-Aug-2026\n.SH DESCRIPTION\n\\*[first] and \\*[second]\n",
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
            b".TH EXAMPLE 1 28-Aug-2026\n.SH DESCRIPTION\n.EX\nouter\n.EX\ninner\n.EE\nouter\n.EE\n",
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
        .parse(Source::new(
            &name,
            b".TH FILL 1 28-Aug-2026\n.SH DESCRIPTION\n.fi\n",
        ))
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
        .parse(Source::new(&name, b".TH ignored 1 28-Aug-2026\n"))
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
        .parse(Source::new(
            &name,
            b".TH RAW 1 28-Aug-2026\n.SH BODY\ntext\n",
        ))
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
fn formatter_metric_requests_are_non_public() {
    let name = SourceName::new("point-size.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ps 36\n.pl 8000\n.ss \\n[.ss] 0\n.if dps active\nvisible\n",
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
                b"'\\\" t\r\n.\\\" comment\r\n.ie \"\\f[CB]x\\f[]\"x\" \\{\\\r\n. ftr V B\r\n.\\}\r\n.el \\{\\\r\n. ftr V CR\r\n.\\}\r\n.TH CONDITIONAL 1 \"August 28, 2026\"\r\nvisible\r\n",
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
                b".TH AN-MARGIN 1 28-Aug-2026\n.SH DESCRIPTION\n.RS 0.0\n\\n[an-margin]\n.RS 3.5\n\\n[an-margin]\n.RE\n\\n[an-margin]\n.RE\n\\n[an-margin]\n",
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
            b".TH FALLBACK-OS 1 28-Aug-2026\n.SH NAME\nfallback-os\n",
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
