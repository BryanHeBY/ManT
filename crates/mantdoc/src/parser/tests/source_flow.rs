use super::*;

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
