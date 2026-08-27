use crate::{DiagnosticCode, NodeKind, NodeRef, Parser, Source, SourceName};

#[test]
fn leading_comments_do_not_receive_filled_sentence_punctuation() {
    let name = SourceName::new("man-comment-sentence.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".\\\" Copyright sentence.\n.TH COMMENT 1\n.SH NAME\ncomment - prose.\n",
        ))
        .unwrap();
    let comment = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Comment)
        .unwrap();
    assert_eq!(comment.text(), Some(" Copyright sentence."));
    assert!(!comment.flags().sentence_end);
    let prose = report
        .document
        .preorder()
        .find(|node| node.text() == Some("comment - prose."))
        .unwrap();
    assert!(prose.flags().sentence_end);
}

#[test]
fn structures_sections_terms_and_indents_from_executed_scanner_nodes() {
    let name = SourceName::new("man-structure.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH STRUCTURE 1 \"August 25, 2026\" x Manual\n.SH FIRST\nouter\n.SS CHILD\n.TP\nterm\ndefinition\n.RS\nindented\n.RE\n",
            ))
            .unwrap();
    let document = &report.document;
    assert_eq!(document.metadata().title.as_deref(), Some("STRUCTURE"));
    assert_eq!(document.metadata().section.as_deref(), Some("1"));
    assert_eq!(document.metadata().date.as_deref(), Some("August 25, 2026"));
    assert_eq!(document.metadata().os.as_deref(), Some("x"));
    assert_eq!(document.metadata().volume.as_deref(), Some("Manual"));

    let root = document.node(document.root()).unwrap();
    let section = root
        .children()
        .find(|node| node.macro_name() == Some("SH"))
        .unwrap();
    assert_eq!(section.kind(), NodeKind::Block);
    let mut section_parts = section.children();
    let head = section_parts.next().unwrap();
    let body = section_parts.next().unwrap();
    assert_eq!(head.kind(), NodeKind::Head);
    assert_eq!(head.macro_name(), Some("SH"));
    assert_eq!(head.children().next().unwrap().text(), Some("FIRST"));
    assert_eq!(body.kind(), NodeKind::Body);
    let subsection = body
        .children()
        .find(|node| node.macro_name() == Some("SS"))
        .unwrap();
    let subsection_body = subsection.children().nth(1).unwrap();
    let term = subsection_body
        .children()
        .find(|node| node.macro_name() == Some("TP"))
        .unwrap();
    assert_eq!(term.kind(), NodeKind::Block);
    let term_head = term.children().next().unwrap();
    assert_eq!(term_head.kind(), NodeKind::Head);
    assert_eq!(term_head.children().next().unwrap().text(), Some("term"));
    assert!(section.children().next().unwrap().flags().deep_link_target);
    assert!(section.children().next().unwrap().flags().permalink);
}

#[test]
fn structures_man_indents_emitted_by_a_user_macro() {
    let name = SourceName::new("man-macro-indent.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de1 INDENT\n. RS \\\\$1\n..\n.de UNINDENT\n. RE\n..\n.TH INDENT 1 28-Aug-2026\n.SH DESCRIPTION\nintro\n.INDENT 0.0\n.TP\nterm\ndescription\n.UNINDENT\n",
            ))
            .unwrap();
    let indent = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("RS"))
        .expect("macro-generated RS block");
    assert_eq!(indent.kind(), NodeKind::Block);
    let mut parts = indent.children();
    let head = parts.next().expect("RS head");
    assert_eq!(head.kind(), NodeKind::Head);
    assert_eq!(head.children().next().and_then(NodeRef::text), Some("0.0"));
    let body = parts.next().expect("RS body");
    assert_eq!(body.kind(), NodeKind::Body);
    assert!(body.children().any(|node| node.macro_name() == Some("TP")));
}

#[test]
fn normalizes_abbreviated_title_months_in_metadata() {
    let name = SourceName::new("man-title-date.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH TITLE 1 \"Jul 31, 2026\"\n.SH NAME\ntitle\n",
        ))
        .unwrap();
    assert_eq!(
        report.document.metadata().date.as_deref(),
        Some("July 31, 2026")
    );
}

#[test]
fn mr_is_a_recognized_inline_man_macro() {
    let name = SourceName::new("man-mr.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH MR 1 28-Aug-2026\n.SH DESCRIPTION\n.MR printf 3\nafter\n",
        ))
        .unwrap();
    let reference = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("MR"))
        .expect("MR stays a recognized man inline macro");
    assert_eq!(reference.kind(), NodeKind::Element);
    assert_eq!(
        reference
            .children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["printf", "3"]
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_str() != DiagnosticCode::ROFF_UNKNOWN_MACRO)
    );
}

#[test]
fn inline_conditional_dispatches_man_request_body() {
    let name = SourceName::new("man-conditional-pod.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH OPTION 1 28-Aug-2026\n.SH DESCRIPTION\n.ie n .IP \"*<\"\"\\-fallthrough\"\">\" 4\nbody\n.el .IP *<\\f(CW\\-other\\fR> 4\n",
            ))
            .unwrap();
    let heads = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("IP"))
        .map(|head| head.children().map(NodeRef::text).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(
        heads,
        [[Some("*<\"\\-fallthrough\">"), Some("4")],].as_slice()
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn filled_c_before_a_blank_line_discards_only_the_recovery_pair() {
    let name = SourceName::new("man-c-blank.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH C-BLANK 1 28-Aug-2026\n.SH DESCRIPTION\nfilled\\c\n\nnext\n.nf\nliteral\\c\n\nlater\n.fi\n",
        ))
        .unwrap();
    let texts = report
        .document
        .preorder()
        .filter_map(NodeRef::text)
        .collect::<Vec<_>>();
    assert!(texts.contains(&"filled"));
    assert!(!texts.contains(&"filled\\c"));
    assert!(texts.contains(&"literal\\c"));
    assert_eq!(texts.iter().filter(|text| text.is_empty()).count(), 1);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn a_continued_line_keeps_next_line_scopes_open() {
    let name = SourceName::new("man-c-scope.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH C-SCOPE 1 28-Aug-2026\n.SH DESCRIPTION\n.B\none\\c\nword\n.TP\nterm\\c\nword\ndefinition\n",
        ))
        .unwrap();
    let bold = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("B"))
        .unwrap();
    assert_eq!(
        bold.children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["one\\c", "word"]
    );
    let term = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
        .unwrap();
    assert_eq!(
        term.children()
            .next()
            .unwrap()
            .children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["term\\c", "word"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn a_physical_text_continuation_stays_in_its_tp_head() {
    let name = SourceName::new("man-physical-continuation.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CONTINUATION 1 28-Aug-2026\n.SH DESCRIPTION\n.TP\nfirst\\\nsecond\ndefinition\n",
        ))
        .unwrap();
    let term = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
        .expect("TP block");
    let mut parts = term.children();
    let head = parts.next().expect("TP head");
    let body = parts.next().expect("TP body");
    assert_eq!(
        head.children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["firstsecond"]
    );
    assert_eq!(
        body.children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["definition"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn unmatched_re_breaks_out_of_the_current_implicit_term() {
    let name = SourceName::new("man-unmatched-re.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH UNMATCHED-RE 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 6n\ntag\nbody\n.RE\noutside\n",
        ))
        .unwrap();
    let body = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .find(|node| node.macro_name() == Some("SH"))
        .unwrap()
        .children()
        .nth(1)
        .unwrap();
    let children = body.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].macro_name(), Some("TP"));
    assert_eq!(children[1].kind(), NodeKind::Element);
    assert_eq!(children[1].macro_name(), Some("br"));
    assert_eq!(children[2].text(), Some("outside"));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MAN_UNMATCHED_CLOSE)
    );
}

#[test]
fn paragraph_distance_keeps_next_line_man_scopes_open() {
    let name = SourceName::new("man-pd-nextline.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH PD-NEXTLINE 1 28-Aug-2026\n.SH\n.PD 0v\nSECTION\n.TP\n.PD 0v\ntag\nbody\n.B\n.PD 0v\nbold\n",
        ))
        .unwrap();
    let section = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .find(|node| node.macro_name() == Some("SH"))
        .unwrap();
    let head = section.children().next().unwrap();
    let head_children = head.children().collect::<Vec<_>>();
    assert_eq!(head_children[0].macro_name(), Some("PD"));
    assert_eq!(head_children[1].text(), Some("SECTION"));

    let body = section.children().nth(1).unwrap();
    let term = body
        .children()
        .find(|node| node.macro_name() == Some("TP"))
        .unwrap();
    let term_head = term.children().next().unwrap();
    let term_children = term_head.children().collect::<Vec<_>>();
    assert_eq!(term_children[0].macro_name(), Some("PD"));
    assert_eq!(term_children[1].text(), Some("tag"));

    let term_body = term.children().nth(1).unwrap();
    let bold = term_body
        .children()
        .find(|node| node.macro_name() == Some("B"))
        .unwrap();
    let bold_children = bold.children().collect::<Vec<_>>();
    assert_eq!(bold_children[0].macro_name(), Some("PD"));
    assert_eq!(bold_children[1].text(), Some("bold"));
}

#[test]
fn rs_closes_an_implicit_indent_before_restoring_outer_flow() {
    let name = SourceName::new("man-rs-implicit-parent.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH RS-IMPLICIT-PARENT 1 28-Aug-2026\n.SH DESCRIPTION\n.IP tag 6n\nterm body\n.RS\nindented\n.RE\nafter indent\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let body = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .find(|node| node.macro_name() == Some("SH"))
        .unwrap()
        .children()
        .nth(1)
        .unwrap();
    let children = body.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].macro_name(), Some("IP"));
    assert_eq!(children[1].macro_name(), Some("RS"));
    assert_eq!(children[2].text(), Some("after indent"));
}

#[test]
fn centering_and_right_adjustment_own_their_following_input_lines() {
    let name = SourceName::new("man-center.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CENTER 1 28-Aug-2026\n.SH DESCRIPTION\n.ce 2\nfirst centered\nsecond centered\n.rj 1\nright adjusted\nafter\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let elements = report
        .document
        .preorder()
        .filter(|node| matches!(node.macro_name(), Some("ce" | "rj")))
        .collect::<Vec<_>>();
    assert_eq!(elements.len(), 2);
    assert_eq!(
        elements[0]
            .children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["2", "first centered", "second centered"]
    );
    assert_eq!(
        elements[1]
            .children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["1", "right adjusted"]
    );
}

#[test]
fn th_is_metadata_only_and_derives_a_known_section_volume() {
    let name = SourceName::new("metadata.3").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH METADATA 3 25-Aug-2026\n.SH NAME\nmetadata\n",
        ))
        .unwrap();
    assert_eq!(
        report.document.metadata().title.as_deref(),
        Some("METADATA")
    );
    assert_eq!(report.document.metadata().section.as_deref(), Some("3"));
    assert_eq!(
        report.document.metadata().volume.as_deref(),
        Some("Library Functions Manual")
    );
    assert!(
        report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .all(|node| node.macro_name() != Some("TH"))
    );
}

#[test]
fn section_openers_select_man_without_th_and_recover_missing_metadata() {
    let name = SourceName::new("no-th.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".SH NAME\nno-th \\- title macro missing\n.SH DESCRIPTION\ntext\n",
        ))
        .unwrap();
    assert_eq!(report.document.macro_set(), crate::MacroSet::Man);
    assert_eq!(report.document.metadata().title.as_deref(), Some(""));
    assert_eq!(report.document.metadata().section.as_deref(), Some(""));
    assert_eq!(report.document.metadata().date.as_deref(), Some(""));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "missing manual title, using \"\"",
            "missing date, using \"\""
        ]
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| { node.kind() == NodeKind::Block && node.macro_name() == Some("SH") })
    );
}

#[test]
fn complete_title_without_visible_body_reports_the_legacy_warning() {
    let name = SourceName::new("man-no-body.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".TH NO-BODY 1 \"August 25, 2026\"\n"))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [DiagnosticCode::MAN_NO_DOCUMENT_BODY]
    );
    assert_eq!(report.diagnostics[0].message.as_ref(), "no document body");
}

#[test]
fn unparseable_th_dates_remain_metadata_and_report_their_argument() {
    let name = SourceName::new("bad-th-date.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH BAD-DATE 1 \"May 2001\"\n.SH NAME\nbad-date\n",
        ))
        .unwrap();
    assert_eq!(report.document.metadata().date.as_deref(), Some("May 2001"));
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_TITLE_DATE_UNPARSEABLE
    );
    assert_eq!(
        diagnostic.message.as_ref(),
        "cannot parse date, using it verbatim: TH May 2001"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (1, 16));
}

#[test]
fn empty_th_date_remains_metadata_and_reports_the_empty_argument() {
    let name = SourceName::new("empty-th-date.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH EMPTY-DATE 1 \"\" source\n.SH NAME\nempty-date\n",
        ))
        .unwrap();
    assert_eq!(report.document.metadata().date.as_deref(), Some(""));
    let diagnostic = report.diagnostics.first().unwrap();
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_TITLE_DATE_MISSING
    );
    assert_eq!(diagnostic.message.as_ref(), "missing date, using \"\": TH");
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (1, 18));
}

#[test]
fn omitted_th_date_reports_the_title_control() {
    let name = SourceName::new("omitted-th-date.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH OMITTED-DATE 1\n.SH NAME\nomitted-date\n",
        ))
        .unwrap();
    assert_eq!(report.document.metadata().date.as_deref(), Some(""));
    let diagnostic = report.diagnostics.first().unwrap();
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_TITLE_DATE_MISSING
    );
    assert_eq!(diagnostic.message.as_ref(), "missing date, using \"\": TH");
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (1, 2));
}

#[test]
fn empty_ip_is_removed_before_the_next_paragraph_boundary() {
    let name = SourceName::new("empty-ip.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH EMPTY-IP 1 28-Aug-2026\n.SH DESCRIPTION\n.IP\n.IP tag\nbody\n",
        ))
        .unwrap();
    let ips = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("IP"))
        .collect::<Vec<_>>();
    assert_eq!(ips.len(), 1);
    assert_eq!(
        ips[0]
            .children()
            .next()
            .unwrap()
            .children()
            .next()
            .unwrap()
            .text(),
        Some("tag")
    );
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_EMPTY_PARAGRAPH
    );
    assert_eq!(
        diagnostic.message.as_ref(),
        "skipping paragraph macro: IP empty"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (3, 2));
}

#[test]
fn mt_validates_uri_arguments_and_returns_me_tail_to_outer_flow() {
    let name = SourceName::new("mt-args.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH MT-ARGS 1 28-Aug-2026\n.SH DESCRIPTION\n.MT first second\ntext\n.ME tail args\n",
        ))
        .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("MT"))
        .unwrap();
    assert_eq!(
        block
            .children()
            .next()
            .unwrap()
            .children()
            .next()
            .unwrap()
            .text(),
        Some("first")
    );
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::MAN_EXCESS_ARGUMENTS
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "skipping excess arguments: MT ... second"
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("tail args"))
    );
}

#[test]
fn op_reports_missing_and_superfluous_option_arguments_without_rewriting_flow() {
    let name = SourceName::new("op-args.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH OP-ARGS 1 28-Aug-2026\n.SH DESCRIPTION\n.OP\n.OP -f arg bogus\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::MAN_MISSING_OPTION,
            DiagnosticCode::MAN_EXCESS_ARGUMENTS,
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "missing option string, using \"\": OP",
            "skipping excess arguments: OP ... bogus",
        ]
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("bogus"))
    );
}

#[test]
fn pd_reports_and_removes_its_first_excess_argument() {
    let name = SourceName::new("pd-args.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH PD-ARGS 1 28-Aug-2026\n.SH DESCRIPTION\n.PD 0 zzz\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_EXCESS_ARGUMENTS
    );
    assert_eq!(
        diagnostic.message.as_ref(),
        "skipping excess arguments: PD ... zzz"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (3, 7));
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.text() == Some("zzz"))
    );
}

#[test]
fn sp_reports_and_removes_its_first_excess_argument() {
    let name = SourceName::new("sp-args.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH SP-ARGS 1 28-Aug-2026\n.SH DESCRIPTION\nbody\n.sp 3v 2i\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_EXCESS_ARGUMENTS
    );
    assert_eq!(
        diagnostic.message.as_ref(),
        "skipping excess arguments: sp ... 2i"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (4, 8));
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.text() == Some("2i"))
    );
}

#[test]
fn paragraph_controls_report_but_retain_ignored_arguments() {
    let name = SourceName::new("paragraph-args.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH PARAGRAPH-ARGS 1 28-Aug-2026\n.SH DESCRIPTION\n.PP arg\n.LP arg1 arg2\n.P arg\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::MAN_ALL_ARGUMENTS,
            DiagnosticCode::MAN_ALL_ARGUMENTS,
            DiagnosticCode::MAN_ALL_ARGUMENTS,
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping all arguments: PP arg",
            "skipping all arguments: PP arg1 ...",
            "skipping all arguments: PP arg",
        ]
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("arg2"))
    );
}

#[test]
fn empty_paragraph_controls_report_empty_and_after_section_recovery() {
    let name = SourceName::new("paragraph-empty.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH PARAGRAPH-EMPTY 1 28-Aug-2026\n.SH DESCRIPTION\n.PP\nheading paragraph\n.PP\n.PP\nbody\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping paragraph macro: PP empty",
            "skipping paragraph macro: PP after SH",
        ]
    );
    let locations = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let location = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (location.line, location.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(locations, [(5, 2), (3, 2)]);
}

#[test]
fn terminal_section_break_is_removed_and_reported() {
    let name = SourceName::new("terminal-break.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH TERMINAL-BREAK 1 28-Aug-2026\n.SH DESCRIPTION\nvisible text\n.br\n",
        ))
        .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "skipping paragraph macro: br at the end of SH"
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (4, 2));
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("br"))
    );
}

#[test]
fn structures_paragraphs_tq_and_next_line_term_heads() {
    let name = SourceName::new("man-lists.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH LISTS 1 28-Aug-2026\n.SH DESCRIPTION\n.PP\nfirst paragraph\n.TP\nfirst term\nfirst definition\n.TQ\nsecond term\nsecond definition\n.IP marker 4\nindented definition\n.HP 4\nhanging definition\n",
            ))
            .unwrap();
    let section_body = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .find(|node| node.macro_name() == Some("SH"))
        .unwrap()
        .children()
        .nth(1)
        .unwrap();
    let blocks = section_body.children().collect::<Vec<_>>();
    assert_eq!(
        blocks
            .iter()
            .map(|node| node.macro_name())
            .collect::<Vec<_>>(),
        [None, Some("TP"), Some("TQ"), Some("IP"), Some("HP")]
    );
    assert_eq!(blocks[0].kind(), NodeKind::Text);
    assert!(
        blocks[1..]
            .iter()
            .all(|node| node.kind() == NodeKind::Block)
    );

    assert_eq!(blocks[0].text(), Some("first paragraph"));

    let term_head = blocks[1].children().next().unwrap();
    assert_eq!(
        term_head.children().next().unwrap().text(),
        Some("first term")
    );
    let tq_head = blocks[2].children().next().unwrap();
    assert_eq!(
        tq_head.children().next().unwrap().text(),
        Some("second term")
    );
    let ip_head = blocks[3].children().next().unwrap();
    assert_eq!(
        ip_head.children().map(NodeRef::text).collect::<Vec<_>>(),
        [Some("marker"), Some("4")]
    );
    let hp_head = blocks[4].children().next().unwrap();
    assert_eq!(hp_head.children().next().unwrap().text(), Some("4"));
    assert!(term_head.flags().deep_link_target);
    assert!(term_head.flags().permalink);
    assert!(ip_head.flags().deep_link_target);
}

#[test]
fn nested_empty_font_macros_finish_a_pending_tp_term_at_its_text() {
    let name = SourceName::new("man-tp-nested-font-term.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH NESTED 1 28-Aug-2026\n.SH DESCRIPTION\n.TP\n.B\n.I\nterm\ndefinition\n",
        ))
        .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
        .unwrap();
    let mut parts = block.children();
    let head = parts.next().unwrap();
    let body = parts.next().unwrap();
    let bold = head.children().next().unwrap();
    let italic = bold.children().next().unwrap();
    let term = italic.children().next().unwrap();
    assert_eq!(term.text(), Some("term"));
    assert_eq!(
        body.location().unwrap().start,
        term.location().unwrap().start
    );
}

#[test]
fn pending_tp_head_retains_indent_request_before_its_term() {
    let name = SourceName::new("man-tp-indent.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH INDENT 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 8n\n.in 3n\ntag\nbody\n",
        ))
        .unwrap();
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("TP"))
        .unwrap();
    let children = head.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].text(), Some("8n"));
    assert_eq!(children[1].macro_name(), Some("in"));
    assert_eq!(children[1].children().next().unwrap().text(), Some("+3n"));
    assert_eq!(children[2].text(), Some("tag"));
}

#[test]
fn structures_explicit_link_mail_and_synopsis_blocks() {
    let name = SourceName::new("man-explicit.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH EXPLICIT 1 28-Aug-2026\n.SH LINKS\n.UR https://example.test\nlink body\n.UE\n.MT mail@example.test\nmail body\n.ME\n.SY command\nargument\n.YS\n.B\nbold next line\n",
            ))
            .unwrap();
    let section_body = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .find(|node| node.macro_name() == Some("SH"))
        .unwrap()
        .children()
        .nth(1)
        .unwrap();
    let children = section_body.children().collect::<Vec<_>>();
    assert_eq!(
        children
            .iter()
            .map(|node| node.macro_name())
            .collect::<Vec<_>>(),
        [Some("UR"), Some("MT"), Some("SY"), Some("YS"), Some("B")]
    );
    for block in &children[..3] {
        assert_eq!(block.kind(), NodeKind::Block);
        assert_eq!(block.children().nth(1).unwrap().kind(), NodeKind::Body);
    }
    assert_eq!(
        children[0]
            .children()
            .nth(1)
            .unwrap()
            .children()
            .next()
            .unwrap()
            .text(),
        Some("link body")
    );
    assert_eq!(
        children[4].children().next().unwrap().text(),
        Some("bold next line")
    );
    assert!(
        report
            .document
            .preorder()
            .all(|node| !matches!(node.macro_name(), Some("UE" | "ME")))
    );
}

#[test]
fn eof_drops_an_unfilled_next_line_font_scope_with_a_typed_warning() {
    let name = SourceName::new("man-font-eof.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH FONT-EOF 1 28-Aug-2026\n.SH DESCRIPTION\ntext before scope\n.B\n",
        ))
        .unwrap();
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("B"))
    );
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_LINE_SCOPE_BROKEN
    );
    assert_eq!(
        diagnostic.message.as_ref(),
        "line scope broken: EOF breaks B"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (4, 2));
}

#[test]
fn blank_lines_are_skipped_without_closing_a_next_line_font_scope() {
    let name = SourceName::new("man-font-blank.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH FONT-BLANK 1 28-Aug-2026\n.SH DESCRIPTION\n.B\n\nbold\nafter\n",
        ))
        .unwrap();
    let bold = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("B"))
        .unwrap();
    assert_eq!(bold.children().next().unwrap().text(), Some("bold"));
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::MAN_BLANK_LINE_SCOPE
    );
    assert_eq!(
        diagnostic.message.as_ref(),
        "skipping blank line in line scope"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (4, 1));
}

#[test]
fn propagates_no_fill_and_sentence_state_in_source_order() {
    let name = SourceName::new("man-presentation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH PRESENTATION 1 28-Aug-2026\n.SH EXAMPLES\n.nf\nfirst literal.\n.B bold literal\n.fi\nfilled sentence.\n.EX\nexample line\n.EE\nfinal sentence.\n",
            ))
            .unwrap();
    let section_body = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .find(|node| node.macro_name() == Some("SH"))
        .unwrap()
        .children()
        .nth(1)
        .unwrap();
    let nodes = section_body.children().collect::<Vec<_>>();
    let first_literal = nodes
        .iter()
        .find(|node| node.text() == Some("first literal."))
        .unwrap();
    assert!(first_literal.flags().no_fill);
    assert!(!first_literal.flags().sentence_end);
    let bold = nodes
        .iter()
        .find(|node| node.macro_name() == Some("B"))
        .unwrap();
    assert!(bold.flags().no_fill);
    assert!(bold.children().next().unwrap().flags().no_fill);
    let filled = nodes
        .iter()
        .find(|node| node.text() == Some("filled sentence."))
        .unwrap();
    assert!(!filled.flags().no_fill);
    assert!(filled.flags().sentence_end);
    let example_start = nodes
        .iter()
        .find(|node| node.macro_name() == Some("EX"))
        .unwrap();
    assert!(!example_start.flags().no_fill);
    let example = nodes
        .iter()
        .find(|node| node.text() == Some("example line"))
        .unwrap();
    assert!(example.flags().no_fill);
    let example_end = nodes
        .iter()
        .find(|node| node.macro_name() == Some("EE"))
        .unwrap();
    assert!(example_end.flags().no_fill);
    let final_sentence = nodes
        .iter()
        .find(|node| node.text() == Some("final sentence."))
        .unwrap();
    assert!(!final_sentence.flags().no_fill);
    assert!(final_sentence.flags().sentence_end);
}

#[test]
fn assigns_and_suppresses_man_destination_tags_like_libmandoc() {
    let name = SourceName::new("man-tags.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TAGS 1 28-Aug-2026\n.SH NAME\ntags\n.SH \"SEE ALSO\"\nfirst\n.SS \"SEE ALSO\"\nsecond\n.TP\n-term\ndefinition\n",
            ))
            .unwrap();
    let document = &report.document;
    let section_heads = document
        .preorder()
        .filter(|node| matches!(node.macro_name(), Some("SH" | "SS")))
        .filter(|node| node.kind() == NodeKind::Head)
        .collect::<Vec<_>>();
    assert_eq!(section_heads.len(), 3);
    assert!(section_heads[0].flags().deep_link_target);
    assert_eq!(section_heads[0].tag(), None);
    assert!(
        section_heads[1..]
            .iter()
            .all(|head| !head.flags().deep_link_target && head.tag().is_none())
    );

    let term_head = document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("TP"))
        .unwrap();
    assert!(term_head.flags().deep_link_target);
    assert!(term_head.flags().permalink);
    assert_eq!(term_head.tag(), Some("term"));

    let escaped_heading_name = SourceName::new("man-escaped-heading-tag.1").unwrap();
    let escaped_heading_report = Parser::default()
        .parse(Source::new(
            &escaped_heading_name,
            b".TH TAGS 1 28-Aug-2026\n.SH NAME\ntags\n.SS \"Options Controlling Objective\\-C and Objective\\-C++ Dialects\"\nbody\n",
        ))
        .unwrap();
    let escaped_heading = escaped_heading_report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("SS"))
        .unwrap();
    assert!(escaped_heading.flags().deep_link_target);
    assert_eq!(escaped_heading.tag(), Some("Options_Controlling_Objective"));

    let width_name = SourceName::new("man-width-tag.1").unwrap();
    let width_report = Parser::default()
        .parse(Source::new(
            &width_name,
            b".TH WIDTH 1 28-Aug-2026\n.SH NAME\nwidth\n.SH DESCRIPTION\n.TP 6n\n.BI bold italic\nbody\n",
        ))
        .unwrap();
    let width_term_head = width_report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("TP"))
        .unwrap();
    assert!(width_term_head.flags().deep_link_target);
    assert_eq!(width_term_head.tag(), Some("bold"));

    let priority_name = SourceName::new("man-tag-priority.1").unwrap();
    let priority_report = Parser::default()
            .parse(Source::new(
                &priority_name,
                b".TH TAGS 1 28-Aug-2026\n.SH DESCRIPTION\n.TP\n.I \" plain\"\nfirst\n.TP\nplain\nsecond\n.TP\n.I \"plain \"\nthird\n.HP\n.B not-a-term\nhanging\n.IP \" weak\"\nfirst indent\n.IP -weak\nsecond indent\n",
            ))
            .unwrap();
    let heads = priority_report
        .document
        .preorder()
        .filter(|node| {
            node.kind() == NodeKind::Head && matches!(node.macro_name(), Some("TP" | "HP" | "IP"))
        })
        .collect::<Vec<_>>();
    assert_eq!(heads.len(), 6);
    assert!(
        !heads[0].flags().deep_link_target
            && heads[1].flags().deep_link_target
            && !heads[2].flags().deep_link_target
    );
    assert_eq!(heads[1].tag(), None);
    assert_eq!(heads[2].tag(), None);
    assert!(!heads[3].flags().deep_link_target);
    assert_eq!(heads[3].children().count(), 0);
    assert!(
        !heads[4].flags().deep_link_target
            && heads[5].flags().deep_link_target
            && heads[5].tag() == Some("weak")
    );
}

#[test]
fn reports_unmatched_closers_and_end_of_input_open_blocks() {
    let name = SourceName::new("man-recovery.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH RECOVERY 1 28-Aug-2026\n.RE\n.UR https://example.test\nunclosed link\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::MAN_UNMATCHED_CLOSE,
            DiagnosticCode::MAN_UNCLOSED_BLOCK,
        ]
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.primary.is_some())
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping end of block that is not open: RE",
            "appending missing end of block: UR",
        ]
    );
}

#[test]
fn reports_eof_for_a_pending_section_title_and_removes_the_empty_section() {
    let name = SourceName::new("section-eof.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH SECTION-EOF 1 28-Aug-2026\n.SH DESCRIPTION\ntext\n.SH\n",
        ))
        .unwrap();
    let sections = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("SH"))
        .collect::<Vec<_>>();
    assert_eq!(sections.len(), 1);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::MAN_LINE_SCOPE_BROKEN
    );
    assert_eq!(
        report.diagnostics[0].message.as_ref(),
        "line scope broken: EOF breaks SH"
    );
}

#[test]
fn propagates_eof_through_an_empty_font_scope_in_a_pending_section_title() {
    let name = SourceName::new("section-font-eof.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH SECTION-FONT-EOF 1 28-Aug-2026\n.SH DESCRIPTION\ntext\n.SH\n.B\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "line scope broken: EOF breaks B",
            "line scope broken: EOF breaks SH"
        ]
    );
}

#[test]
fn empty_section_heads_use_fill_toggles_to_start_the_body() {
    let name = SourceName::new("section-macro-break.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH SECTION-BREAK 1 28-Aug-2026\n.SH DESCRIPTION\n.SH\n.nf\nliteral\n.SH\n.fi\nfilled\n",
        ))
        .unwrap();
    let literal = report
        .document
        .preorder()
        .find(|node| node.text() == Some("literal"))
        .unwrap();
    let filled = report
        .document
        .preorder()
        .find(|node| node.text() == Some("filled"))
        .unwrap();
    let fill_restore = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("fi"))
        .unwrap();
    assert!(literal.flags().no_fill);
    assert!(!fill_restore.flags().no_fill);
    assert!(!filled.flags().no_fill);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [DiagnosticCode::MAN_REDUNDANT_FILL_MODE]
    );
}

#[test]
fn fill_toggles_preserve_macro_and_argument_state_boundaries() {
    let name = SourceName::new("man-fill-toggle.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FILL 1 28-Aug-2026\n.SH DESCRIPTION\n.EX opening argument\nliteral\n.EE closing argument\nregular\n",
            ))
            .unwrap();
    let ex = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("EX"))
        .unwrap();
    assert!(!ex.flags().no_fill);
    assert!(ex.children().all(|argument| argument.flags().no_fill));

    let ee = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("EE"))
        .unwrap();
    assert!(ee.flags().no_fill);
    assert!(ee.children().all(|argument| !argument.flags().no_fill));
}

#[test]
fn fill_mode_requests_discard_and_report_their_complete_argument_tail() {
    let name = SourceName::new("man-fill-arguments.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH FILL-ARGS 1 28-Aug-2026\n.SH DESCRIPTION\n.nf arg1 arg2 arg3\nliteral\n.fi arg1 arg2 arg3\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping all arguments: nf arg1 arg2 arg3",
            "skipping all arguments: fi arg1 arg2 arg3",
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
    assert_eq!(positions, [(3, 5), (5, 5)]);
    assert!(
        report
            .document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("nf" | "fi")))
            .all(|node| node.children().next().is_none())
    );
}

#[test]
fn line_break_requests_discard_and_report_their_complete_argument_tail() {
    let name = SourceName::new("man-break-arguments.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH BR-ARGS 1 28-Aug-2026\n.SH DESCRIPTION\nsome\ntext\n.br arg1 arg2 arg3\nmore\ntext\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["skipping all arguments: br arg1 arg2 arg3"]
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (5, 5));
    let break_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("br"))
        .unwrap();
    assert!(break_node.children().next().is_none());
}

#[test]
fn no_fill_keeps_man_term_structure_filled_but_marks_body_flow() {
    let name = SourceName::new("man-no-fill-term.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH FILLTERM 1 28-Aug-2026\n.SH DESCRIPTION\n.nf\n.TP 4n\nterm\nliteral body\n",
        ))
        .unwrap();
    let term = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
        .unwrap();
    assert!(!term.flags().no_fill);
    let mut parts = term.children();
    let head = parts.next().unwrap();
    let body = parts.next().unwrap();
    assert!(!head.flags().no_fill);
    assert!(head.children().all(|node| !node.flags().no_fill));
    assert!(!body.flags().no_fill);
    assert!(body.children().all(|node| node.flags().no_fill));
}

#[test]
fn fill_toggle_after_tp_stays_in_the_pending_term_head() {
    let name = SourceName::new("man-no-fill-pending-term.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH FILLTERM 1 28-Aug-2026\n.SH DESCRIPTION\n.TP\n.nf\nterm\nliteral body\n",
        ))
        .unwrap();
    let term = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
        .unwrap();
    let mut parts = term.children();
    let head = parts.next().unwrap();
    let body = parts.next().unwrap();
    assert_eq!(
        head.children()
            .map(|node| (node.macro_name(), node.text()))
            .collect::<Vec<_>>(),
        [(Some("nf"), None), (None, Some("term"))]
    );
    assert_eq!(
        body.children().next().and_then(NodeRef::text),
        Some("literal body")
    );
}

#[test]
fn ip_tab_separated_tag_stays_one_head_argument_before_the_width() {
    let name = SourceName::new("man-ip-tab.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH IPTAB 1 28-Aug-2026\n.SH DESCRIPTION\n.IP single\ttab 3n\nbody\n",
        ))
        .unwrap();
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("IP"))
        .unwrap();
    assert_eq!(
        head.children().map(NodeRef::text).collect::<Vec<_>>(),
        [Some("single\ttab"), Some("3n")]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT]
    );
    assert_eq!(report.diagnostics[0].message.as_ref(), "tab in filled text");
}

#[test]
fn man_argument_cursor_counts_direct_but_not_copy_mode_string_expansion() {
    let name = SourceName::new("man-copy-mode-cursor.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CURSOR 1 28-Aug-2026\n.SH DESCRIPTION\n.ds s foo\n.IB \"\\\\*[s]\\*(Aq\" after\n",
        ))
        .unwrap();
    let after = report
        .document
        .preorder()
        .find(|node| node.text() == Some("after"))
        .unwrap();
    let position = report
        .document
        .source_position(after.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (4, 14));
}

#[test]
fn man_argument_cursor_keeps_copy_mode_output_at_its_authored_width() {
    let name = SourceName::new("man-copy-mode-width.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CURSOR 1 28-Aug-2026\n.SH DESCRIPTION\n.IB \"one\\\\ one\" \"\\\\ \"\n",
        ))
        .unwrap();
    let escaped_blank = report
        .document
        .preorder()
        .find(|node| node.text() == Some("\\ "))
        .unwrap();
    let position = report
        .document
        .source_position(escaped_blank.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (3, 17));
}

#[test]
fn man_argument_cursor_rebases_after_a_direct_string_expansion() {
    let name = SourceName::new("man-direct-string-cursor.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CURSOR 1 28-Aug-2026\n.SH DESCRIPTION\n.IP \"one\\*(Aqt\" 4\nbody\n",
        ))
        .unwrap();
    let width = report
        .document
        .preorder()
        .find(|node| node.text() == Some("4"))
        .unwrap();
    let position = report
        .document
        .source_position(width.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (3, 12));
}

#[test]
fn section_title_punctuation_is_not_a_flow_sentence_boundary() {
    let name = SourceName::new("man-heading-punctuation.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH HEADING 1 28-Aug-2026\n.SH \"A heading.\"\ntext\n",
        ))
        .unwrap();
    let heading = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("SH"))
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert!(!heading.flags().sentence_end);
}

#[test]
fn deferred_subsection_title_retains_its_text_sentence_boundary() {
    let name = SourceName::new("man-deferred-subsection.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH HEADING 1 28-Aug-2026\n.SH DESCRIPTION\n.SS\nA deferred subsection title.\nbody\n",
        ))
        .unwrap();
    let heading = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("SS"))
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert!(heading.flags().line_start);
    assert!(heading.flags().sentence_end);
}

#[test]
fn tbl_openers_break_pending_man_line_scopes_without_leaking_controls() {
    let name = SourceName::new("man-tbl-break.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TBL-BREAK 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 6n\n.TS\nl.\nfirst\n.TE\n.SH\n.TS\nl.\nsecond\n.TE\n.SS\n.TS\nl.\nthird\n.TE\n.B\n.TS\nl.\nfourth\n.TE\nfinal\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "line scope broken: TS breaks TP",
            "line scope broken: TS breaks SH",
            "line scope broken: TS breaks SS",
            "line scope broken: TS breaks B",
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .count(),
        4
    );
    assert!(
        !report
            .document
            .preorder()
            .any(|node| { matches!(node.macro_name(), Some("TP" | "SS" | "B" | "TS")) })
    );
}
