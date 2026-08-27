use super::*;

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
