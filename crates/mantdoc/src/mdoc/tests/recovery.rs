use super::*;

#[test]
fn no_fill_toggles_are_scoped_by_mdoc_display_blocks() {
    let name = SourceName::new("mdoc-display-fill-state.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FILL 1\n.Os\n.Sh DESCRIPTION\n.nf\nouter literal\n.fi\n.Bd -unfilled\ndisplay literal\n.fi\ndisplay filled\n.Ed\n.Bd -filled\n.nf\ninner literal\n.Ed\nouter filled\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    for (text, expected_no_fill) in [
        ("outer literal", true),
        ("display literal", true),
        ("display filled", false),
        ("inner literal", true),
        ("outer filled", false),
    ] {
        let node = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some(text))
            .unwrap();
        assert_eq!(node.flags().no_fill, expected_no_fill, "{text}");
    }
}

#[test]
fn literal_text_does_not_receive_filled_sentence_punctuation() {
    let name = SourceName::new("mdoc-literal-sentence.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 28, 2026\n.Dt LITERAL 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n\\&...\n.Ed\n",
        ))
        .unwrap();
    let literal = report
        .document
        .preorder()
        .find(|node| node.text() == Some("\\&..."))
        .unwrap();
    assert!(literal.flags().no_fill);
    assert!(!literal.flags().sentence_end);
}

#[test]
fn filled_c_blank_recovery_omits_only_the_filled_pair() {
    let name = SourceName::new("mdoc-c-blank.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt C-BLANK 1\n.Os\n.Sh DESCRIPTION\nfilled\\c\n\nnext\n.Bd -literal\nliteral\\c\n\nnext literal\n.Ed\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let filled = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("filled"))
        .unwrap();
    assert!(!filled.flags().line_continuation);
    assert!(
        !nodes
            .iter()
            .any(|node| node.text() == Some("") && !node.flags().no_fill)
    );

    let literal = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("literal\\c"))
        .unwrap();
    assert!(literal.flags().no_fill);
    assert!(literal.flags().line_continuation);
    assert!(
        nodes
            .iter()
            .any(|node| node.text() == Some("") && node.flags().no_fill)
    );
}

#[test]
fn filled_blank_lines_and_transparent_tags_share_paragraph_control_recovery() {
    let name = SourceName::new("mdoc-blank-layout-tags.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt BLANK-TAGS 1\n.Os\n.Sh NAME\n.Nm blank-tags\n.Nd paragraph layout\n.Sh DESCRIPTION\n.br\n.Tg direct\n.sp\n.Pp\n.Tg paragraph\n\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            "input.blank-line-in-filled-text",
            "mdoc.paragraph-before-block",
            "mdoc.paragraph-before-block",
        ]
    );

    let nodes = report.document.preorder().collect::<Vec<_>>();
    let tag = |name| {
        nodes
            .iter()
            .copied()
            .find(|node| {
                node.macro_name() == Some("Tg")
                    && node.children().any(|child| child.text() == Some(name))
            })
            .unwrap()
    };
    let direct = tag("direct");
    assert!(direct.flags().deep_link_target);
    assert!(!direct.flags().no_print);
    let paragraph = tag("paragraph");
    assert!(paragraph.flags().no_print);
    let paragraph_owner = nodes
        .iter()
        .copied()
        .find(|node| node.macro_name() == Some("Pp") && node.tag() == Some("paragraph"))
        .unwrap();
    assert!(paragraph_owner.flags().deep_link_target);
}

#[test]
fn list_tail_paragraphs_move_before_outer_paragraph_validation() {
    let name = SourceName::new("mdoc-list-tail-paragraphs.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt LIST-TAILS 1\n.Os\n.Sh NAME\n.Nm list-tails\n.Nd paragraph layout\n.Sh DESCRIPTION\n.Bl -item\n.It\nfirst\n.Pp\n.It\nsecond\n.Pp\n.El\n.Pp\nend\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| { (diagnostic.code.as_str(), diagnostic.message.as_ref(),) })
            .collect::<Vec<_>>(),
        [
            (
                "mdoc.paragraph-before-block",
                "skipping paragraph macro: Pp before It",
            ),
            (
                "mdoc.paragraph-moved-out-of-list",
                "moving paragraph macro out of list: Pp",
            ),
            (
                "mdoc.paragraph-before-block",
                "skipping paragraph macro: Pp before Pp",
            ),
        ]
    );
    let item_bodies = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(item_bodies.len(), 2);
    assert!(item_bodies.iter().all(|body| {
        body.children()
            .all(|child| child.macro_name() != Some("Pp"))
    }));
}

#[test]
fn literal_display_normalizes_whitespace_only_lines_without_losing_indent() {
    let name = SourceName::new("mdoc-literal-whitespace.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LITERAL 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n \n \t \n x  \n.Ed\n",
            ))
            .unwrap();
    let literal_lines = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text && node.flags().no_fill)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(literal_lines, ["", "", " x"]);
}

#[test]
fn reports_mismatched_and_unclosed_mdoc_scope_blocks() {
    let name = SourceName::new("mdoc-recovery.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt RECOVERY 1\n.Os\n.Sh DESCRIPTION\n.El\n.Bl -bullet\n.Bd -literal\n",
            ))
            .unwrap();
    let codes = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        [
            crate::DiagnosticCode::MDOC_UNMATCHED_CLOSE,
            crate::DiagnosticCode::MDOC_UNCLOSED_BLOCK,
            crate::DiagnosticCode::MDOC_UNCLOSED_BLOCK,
        ]
    );
    assert!(!report.statistics.truncated);
}

#[test]
fn outer_mdoc_closers_report_a_badly_nested_full_block() {
    let name = SourceName::new("mdoc-break.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.Bd -literal\ntext\n.El\n.Ed\n",
            ))
            .unwrap();
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        crate::DiagnosticCode::MDOC_BADLY_NESTED_BLOCK
    );
}

#[test]
fn explicit_partial_closers_report_crossed_partial_blocks() {
    let name = SourceName::new("mdoc-partial-break.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\n.Bo\n.Ec >>\n.Bc\n.Bo\n.Eo <<\n.Bc\n.Ec >>\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "blocks badly nested: Eo breaks Bo",
            "blocks badly nested: Bo breaks Eo",
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>(),
        [(7, 2), (11, 2)]
    );
    let enclosures = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
        .collect::<Vec<_>>();
    assert_eq!(enclosures.len(), 2);
    assert_eq!(enclosures[0].children().count(), 2);
    assert_eq!(enclosures[1].children().count(), 3);
    let first_outer_body = enclosures[0]
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    let first_inner_body = first_outer_body
        .children()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bo"))
        .unwrap()
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    assert!(first_inner_body.children().any(|node| {
        node.kind() == NodeKind::Body
            && node.macro_name() == Some("Eo")
            && node.children().any(|child| child.text() == Some(">>"))
    }));
    let second_outer_body = enclosures[1]
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    assert!(
        second_outer_body
            .children()
            .any(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
    );
}

#[test]
fn implicit_partial_body_preserves_a_crossed_explicit_closer_boundary() {
    let name = SourceName::new("mdoc-implicit-crossed-closer.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt CROSSED 1\n.Os\n.Sh DESCRIPTION\n.Ao ao\n.Bo bo\n.Pq pq bc Bc ac\n.Ac\n",
            ))
            .unwrap();
    let parenthetical = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
        .unwrap();
    let body = parenthetical
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    assert_eq!(
        body.children()
            .map(|node| (node.kind(), node.macro_name(), node.text()))
            .collect::<Vec<_>>(),
        [
            (NodeKind::Text, None, Some("pq bc")),
            (NodeKind::Body, Some("Bo"), None),
            (NodeKind::Text, None, Some("ac")),
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["blocks badly nested: Bo breaks Pq"]
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (7, 11));
}

#[test]
fn validates_the_first_mdoc_root_content_before_a_section() {
    let display_name = SourceName::new("mdoc-before-section.1").unwrap();
    let display_report = Parser::default()
            .parse(Source::new(
                &display_name,
                b".Dd August 25, 2026\n.Dt BEFORE 1\n.Os\n.Bd -filled\nintro\n.Ed\n.Sh DESCRIPTION\nbody\n",
            ))
            .unwrap();
    assert_eq!(
        display_report.diagnostics[0].code.as_str(),
        crate::DiagnosticCode::MDOC_CONTENT_BEFORE_SECTION
    );
    assert_eq!(
        display_report.diagnostics[0].message.as_ref(),
        "content before first section header: Bd"
    );

    let paragraph_name = SourceName::new("mdoc-paragraph-before-section.1").unwrap();
    let paragraph_report = Parser::default()
        .parse(Source::new(
            &paragraph_name,
            b".Dd August 25, 2026\n.Dt PARAGRAPH 1\n.Os\n.Pp\n.Sh DESCRIPTION\nbody\n",
        ))
        .unwrap();
    assert_eq!(
        paragraph_report.diagnostics[0].code.as_str(),
        crate::DiagnosticCode::MDOC_PARAGRAPH_BEFORE_BLOCK
    );
    assert!(
        !paragraph_report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("Pp"))
    );
}

#[test]
fn retains_an_explicit_partial_scope_across_a_broken_display_close() {
    let name = SourceName::new("mdoc-broken-display-close.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bd -filled\n.Bo\ninside\n.Ed\nafter display\n.Bc\nafter both\n",
            ))
            .unwrap();
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        crate::DiagnosticCode::MDOC_BADLY_NESTED_BLOCK
    );
    let bracket_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        .unwrap();
    let children = bracket_body.children().collect::<Vec<_>>();
    assert!(
        children
            .iter()
            .any(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bd"))
    );
    assert!(
        children
            .iter()
            .any(|node| node.text() == Some("after display"))
    );
    assert!(
        !children
            .iter()
            .any(|node| node.text() == Some("after both"))
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("after both"))
    );
}

#[test]
fn retains_a_full_display_scope_across_a_broken_partial_close() {
    let name = SourceName::new("mdoc-broken-partial-close.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bo\n.Bd -filled\ninside\n.Bc\nafter bracket\n.Ed\nafter both\n",
            ))
            .unwrap();
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        crate::DiagnosticCode::MDOC_BADLY_NESTED_BLOCK
    );
    let display_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bd"))
        .unwrap();
    let children = display_body.children().collect::<Vec<_>>();
    assert!(
        children
            .iter()
            .any(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
    );
    assert!(
        children
            .iter()
            .any(|node| node.text() == Some("after bracket"))
    );
}

#[test]
fn removes_a_noncompact_preceding_layout_control_before_a_display() {
    let name = SourceName::new("mdoc-display-previous-paragraph.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\ntext\n.br\n.Bd -filled\nbody\n.Ed\n",
            ))
            .unwrap();
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        crate::DiagnosticCode::MDOC_PARAGRAPH_BEFORE_BLOCK
    );
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("br"))
    );

    let compact_name = SourceName::new("mdoc-compact-display-previous-paragraph.1").unwrap();
    let compact_report = Parser::default()
            .parse(Source::new(
                &compact_name,
                b".Dd August 25, 2026\n.Dt COMPACT 1\n.Os\n.Sh DESCRIPTION\ntext\n.br\n.Bd -filled -compact\nbody\n.Ed\n",
            ))
            .unwrap();
    assert!(compact_report.diagnostics.is_empty());
    assert!(
        compact_report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("br"))
    );
}

#[test]
fn reports_each_normally_closed_empty_display_without_removing_it() {
    let name = SourceName::new("mdoc-empty-displays.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY 1\n.Os\n.Sh DESCRIPTION\n.Bd -filled\n.Ed\n.Bd -literal\n.Ed\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            crate::DiagnosticCode::MDOC_EMPTY_BLOCK,
            crate::DiagnosticCode::MDOC_EMPTY_BLOCK,
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
            .count(),
        2
    );
}

#[test]
fn library_catalogue_expands_known_names_and_rehomes_outer_punctuation() {
    let name = SourceName::new("mdoc-library.3").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt LIBRARY 3\n.Os\n.Sh LIBRARY\n.Lb libbsd\n.Lb mylib .\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [crate::DiagnosticCode::MDOC_UNKNOWN_LIBRARY]
    );
    let libraries = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("Lb"))
        .collect::<Vec<_>>();
    assert_eq!(libraries.len(), 2);

    let known = libraries[0].children().collect::<Vec<_>>();
    assert_eq!(
        known.iter().map(|node| node.text()).collect::<Vec<_>>(),
        [
            Some("Utility functions from BSD systems (libbsd, \\-lbsd)"),
            Some("libbsd"),
        ]
    );
    assert!(known[0].flags().generated);
    assert!(known[1].flags().no_print);

    let unknown = libraries[1].children().collect::<Vec<_>>();
    assert_eq!(
        unknown.iter().map(|node| node.text()).collect::<Vec<_>>(),
        [Some("library"), Some(r"\(lq"), Some("mylib"), Some(r"\(rq")]
    );
    let siblings = libraries[1]
        .parent()
        .expect("library has a semantic parent")
        .children()
        .collect::<Vec<_>>();
    let position = siblings
        .iter()
        .position(|node| node.id() == libraries[1].id())
        .expect("library stays in its parent");
    let outer_period = siblings
        .get(position + 1)
        .copied()
        .expect("period was moved to outer flow");
    assert_eq!(outer_period.text(), Some("."));
    assert!(outer_period.flags().delimiter_close);
    assert!(outer_period.flags().sentence_end);
}

#[test]
fn item_breaks_nested_list_scope_and_relocates_pre_item_content() {
    let name = SourceName::new("mdoc-item-break.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bl -item\nstray text\n.Ao\nnested text\n.It\nitem text\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            crate::DiagnosticCode::MDOC_BROKEN_BLOCK,
            crate::DiagnosticCode::MDOC_CONTENT_OUTSIDE_LIST,
            crate::DiagnosticCode::MDOC_CONTENT_OUTSIDE_LIST,
        ]
    );
    let list_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
        .unwrap();
    assert!(
        list_body
            .children()
            .all(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("stray text"))
    );
}

#[test]
fn nm_validates_attached_trailing_delimiters_after_name_recovery() {
    let name = SourceName::new("mdoc-nm-delimiter.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt NM-DELIMITER 1\n.Os\n.Sh NAME\n.Nm nm-delimiter\n.Nd test\n.Sh DESCRIPTION\n.Nm nm-delimiter.\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| { (diagnostic.code.as_str(), diagnostic.message.as_ref(),) })
            .collect::<Vec<_>>(),
        [(
            "mdoc.trailing-delimiter-spacing",
            "no blank before trailing delimiter: Nm nm-delimiter.",
        )]
    );
    let location = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (8, 17));
}

#[test]
fn nm_leading_delimiters_select_empty_recovery_or_reopened_name_flow() {
    let name = SourceName::new("mdoc-nm-leading-delimiters.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt NM 1\n.Os\n.Sh NAME\n.Nm base\n.Nd test\n.Sh DESCRIPTION\n.Nm ) z\n.Nm ( a\n.Nm | m\n",
            ))
            .unwrap();
    let names = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("Nm"))
        .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
        .collect::<Vec<_>>();
    assert_eq!(names, ["base", "base", "a", "m"]);
    let outer_z = report
        .document
        .preorder()
        .find(|node| node.text() == Some("z"))
        .unwrap();
    assert_eq!(
        outer_z.parent().and_then(crate::NodeRef::macro_name),
        Some("Sh")
    );
    let opening = report
        .document
        .preorder()
        .find(|node| node.text() == Some("(") && node.flags().line_start)
        .unwrap();
    assert!(opening.flags().delimiter_open);
    let reopened = report
        .document
        .preorder()
        .find(|node| {
            node.macro_name() == Some("Nm")
                && node.children().next().and_then(crate::NodeRef::text) == Some("a")
        })
        .unwrap();
    assert!(!reopened.flags().line_start);
}

#[test]
fn pa_validates_attached_trailing_delimiters() {
    let name = SourceName::new("mdoc-pa-delimiter.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt PA-DELIMITER 1\n.Os\n.Sh DESCRIPTION\n.Pa path.\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            "mdoc.trailing-delimiter-spacing",
            "no blank before trailing delimiter: Pa path.",
        )]
    );
    let location = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (5, 9));
}

#[test]
fn tn_discards_empty_forms_and_defers_its_useless_macro_style_finding() {
    let name = SourceName::new("mdoc-tn-validation.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt TN-VALIDATION 1\n.Os\n.Sh DESCRIPTION\n.Tn IBM\n.Tn\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.empty-macro", "skipping empty macro: Tn"),
            ("mdoc.useless-macro", "useless macro: Tn"),
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Tn"))
            .count(),
        1
    );
}

#[test]
fn ud_and_bt_keep_compatibility_nodes_but_validate_their_arguments() {
    let name = SourceName::new("mdoc-useless-compatibility.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt USELESS 1\n.Os\n.Sh DESCRIPTION\n.Ud\n.Bt value\n.Ud first second\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.useless-macro", "useless macro: Ud"),
            ("mdoc.useless-macro", "useless macro: Bt"),
            ("mdoc.arguments", "skipping all arguments: Bt value"),
            ("mdoc.useless-macro", "useless macro: Ud"),
            ("mdoc.arguments", "skipping all arguments: Ud first"),
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("Ud" | "Bt")))
            .count(),
        3
    );
    let generated_sentences = report
        .document
        .preorder()
        .filter(|node| node.flags().generated)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        generated_sentences,
        [
            "currently under development.",
            "is currently in beta test.",
            "currently under development.",
        ]
    );
    assert!(
        report
            .document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("Ud" | "Bt")))
            .all(|node| node.children().next().is_none())
    );
}
