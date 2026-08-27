use super::*;

#[test]
fn empty_lists_remain_visible_and_report_their_openers() {
    let name = SourceName::new("mdoc-empty-lists.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-LISTS 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.El\n.Bl -column one two\n.El\n.Bl -diag\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["empty block: Bl", "empty block: Bl", "empty block: Bl"]
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
    assert_eq!(positions, [(5, 2), (7, 2), (9, 2)]);
    let lists = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 3);
    assert!(lists.iter().all(|list| {
        list.children()
            .find(|node| node.kind() == NodeKind::Body)
            .is_some_and(|body| body.children().next().is_none())
    }));
}

#[test]
fn term_and_tag_list_kinds_report_an_empty_item_head() {
    let name = SourceName::new("mdoc-empty-list-heads.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-HEADS 1\n.Os\n.Sh DESCRIPTION\n.Bl -hang\n.It\nbody\n.El\n.Bl -ohang\n.It\nbody\n.El\n.Bl -inset\n.It\nbody\n.El\n.Bl -diag\n.It\nbody\n.El\n.Bl -tag -width Ds\n.It\nbody\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "empty head in list item: Bl -hang It",
            "empty head in list item: Bl -ohang It",
            "empty head in list item: Bl -inset It",
            "empty head in list item: Bl -diag It",
            "empty head in list item: Bl -tag It",
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
    assert_eq!(positions, [(6, 2), (10, 2), (14, 2), (18, 2), (22, 2)]);
}

#[test]
fn marker_list_items_validate_at_the_next_structural_boundary() {
    let name = SourceName::new("mdoc-empty-marker-items.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-ITEMS 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.It head argument\none\n.It\n.It\nthree\n.El\n.Bl -dash\n.It\none\n.It head argument\n.It\nthree\n.El\n.Bl -enum\n.It\none\n.It\n.It head argument\nthree\n.El\n.Bl -hyphen\n.It Sy head argument\none\n.It\n.It\nthree\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping all arguments: It head argument",
            "empty list item: Bl -bullet It",
            "empty list item: Bl -dash It",
            "skipping all arguments: It head argument",
            "empty list item: Bl -enum It",
            "skipping all arguments: It head argument",
            "skipping all arguments: It Sy",
            "empty list item: Bl -hyphen It",
        ]
    );
}

#[test]
fn item_list_heads_are_syntax_only_without_empty_item_warnings() {
    let name = SourceName::new("mdoc-item-list-heads.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ITEM-LISTS 1\n.Os\n.Sh DESCRIPTION\n.Bl -item\n.It ignored\nbody\n.El\n.Bl -item -compact\n.It ignored\nbody\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping all arguments: It ignored",
            "skipping all arguments: It ignored",
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
    assert_eq!(positions, [(6, 2), (10, 2)]);
}

#[test]
fn tag_list_missing_width_reports_the_private_default_without_publishing_it() {
    let name = SourceName::new("mdoc-tag-list-width.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAG-WIDTH 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It tag\nbody\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["missing -width in -tag list, using 6n: Bl -tag"]
    );
    let list = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
        .unwrap();
    assert_eq!(list.width(), None);
}

#[test]
fn leading_list_content_moves_out_at_item_and_close_boundaries() {
    let name = SourceName::new("mdoc-list-content-before-item.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LIST-CONTENT 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\nstray text\n.Em stray macro\n.It tag\nbody\n.El\n.Bl -dash\nstray text\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "moving content out of list: text",
            "moving content out of list: Em",
            "moving content out of list: text",
            "empty block: Bl",
        ]
    );
    let lists = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2);
    assert_eq!(
        lists[0]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap()
            .children()
            .filter(|node| node.macro_name() == Some("It"))
            .count(),
        1
    );
    assert!(
        lists[1]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap()
            .children()
            .next()
            .is_none()
    );
}

#[test]
fn trailing_spacing_state_stays_with_the_first_list_item() {
    let name = SourceName::new("mdoc-list-spacing-state.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LIST-SPACING 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\nstray text\n.Sm off\n.It\nbody\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["moving content out of list: text"]
    );
    let list_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
        .unwrap();
    assert_eq!(
        list_body
            .children()
            .filter_map(crate::NodeRef::macro_name)
            .collect::<Vec<_>>(),
        ["Sm", "It"]
    );
}

#[test]
fn trailing_explicit_tag_stays_with_the_first_list_item() {
    let name = SourceName::new("mdoc-list-item-tag.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LIST-TAG 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.Tg item\n.It\nbody\n.El\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty());
    let list_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
        .unwrap();
    assert_eq!(
        list_body
            .children()
            .filter_map(crate::NodeRef::macro_name)
            .collect::<Vec<_>>(),
        ["Tg", "It"]
    );
    let item_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("It"))
        .unwrap();
    assert_eq!(item_body.tag(), Some("item"));
    assert!(item_body.flags().deep_link_target);
}

#[test]
fn marker_list_item_targets_move_from_the_inline_term_to_the_head() {
    let name = SourceName::new("mdoc-marker-item-target.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt MARKER-TARGET 1\n.Os\n.Sh DESCRIPTION\n.Bl -hyphen\n.It Sy head argument\nbody\n.El\n",
            ))
            .unwrap();
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
        .unwrap();
    assert_eq!(head.tag(), Some("head"));
    assert!(head.flags().deep_link_target);
    assert!(!head.flags().permalink);
    let sy = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
        .unwrap();
    assert_eq!(sy.tag(), Some("head"));
    assert!(!sy.flags().deep_link_target);
    assert!(sy.flags().permalink);
}

#[test]
fn explicit_tg_before_display_moves_destination_to_body_and_permalink_to_text() {
    let name = SourceName::new("mdoc-tg-display.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TGDISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg display\n.Bd -literal\nvisible text\n.Ed\n",
            ))
            .unwrap();
    let display_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bd"))
        .unwrap();
    assert!(display_body.flags().deep_link_target);
    assert!(!display_body.flags().permalink);
    assert_eq!(display_body.tag(), Some("display"));
    let text = display_body.children().next().unwrap();
    assert!(text.flags().permalink);
    assert_eq!(text.tag(), Some("display"));
    let tg = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(tg.flags().no_print);
}

#[test]
fn one_line_displays_retain_partial_block_phrases_targets_and_empty_warnings() {
    for macro_name in ["D1", "Dl"] {
        let name = SourceName::new(format!("mdoc-{macro_name}-display.1")).unwrap();
        let input = format!(
            ".Dd August 25, 2026\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg display\n.{macro_name} spacing  in  and around one-line displays\n.{macro_name}\n"
        );
        let report = Parser::default()
            .parse(Source::new(&name, input.as_bytes()))
            .unwrap();
        let displays = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some(macro_name))
            .collect::<Vec<_>>();
        assert_eq!(displays.len(), 2);
        let first_body = displays[0]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        assert!(first_body.flags().deep_link_target);
        assert!(!first_body.flags().permalink);
        assert_eq!(first_body.tag(), Some("display"));
        let phrases = first_body
            .children()
            .map(|node| (node.text(), node.flags(), node.tag()))
            .collect::<Vec<_>>();
        assert_eq!(phrases.len(), 2);
        assert_eq!(phrases[0].0, Some("spacing"));
        assert!(phrases[0].1.permalink);
        assert_eq!(phrases[0].2, Some("display"));
        assert_eq!(phrases[1].0, Some("in and around one-line displays"));
        assert!(
            displays[1]
                .children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap()
                .children()
                .next()
                .is_none()
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MDOC_EMPTY_BLOCK)
        );
    }
}

#[test]
fn literal_display_marks_bare_parentheses_as_attached_delimiters() {
    let name = SourceName::new("mdoc-display-delimiters.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Dl name ( ) command\n",
        ))
        .unwrap();
    let display = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Dl"))
        .expect("Dl display");
    let body = display
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .expect("Dl body");
    let children = body.children().collect::<Vec<_>>();
    assert_eq!(
        children
            .iter()
            .filter_map(|node| node.text())
            .collect::<Vec<_>>(),
        ["name", "(", ")", "command"]
    );
    assert!(children[1].flags().delimiter_open);
    assert!(children[2].flags().delimiter_close);
}

#[test]
fn reference_fields_coalesce_direct_text_without_erasing_inline_boundaries() {
    let name = SourceName::new("mdoc-reference-fields.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%A author name\n.%B book title\n.Re\n",
            ))
            .unwrap();
    let fields = report
        .document
        .preorder()
        .filter(|node| matches!(node.macro_name(), Some("%A" | "%B")))
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields
            .iter()
            .map(|field| {
                field
                    .children()
                    .map(crate::NodeRef::text)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![Some("author name")], vec![Some("book title")]]
    );
}

#[test]
fn reference_fields_follow_the_legacy_bibliography_order() {
    let name = SourceName::new("mdoc-reference-order.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%O note\n.%A author\n.%D date\n.%T title\n.Re\n",
            ))
            .unwrap();
    let fields = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Rs"))
        .unwrap()
        .children()
        .map(crate::NodeRef::macro_name)
        .collect::<Vec<_>>();
    assert_eq!(fields, [Some("%A"), Some("%T"), Some("%D"), Some("%O")]);
}

#[test]
fn non_joining_reference_fields_keep_individual_words() {
    let name = SourceName::new("mdoc-reference-word-boundaries.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%N number of journal\n.%A author name\n.Re\n",
            ))
            .unwrap();
    let fields = report
        .document
        .preorder()
        .filter(|node| matches!(node.macro_name(), Some("%A" | "%N")))
        .collect::<Vec<_>>();
    assert_eq!(
        fields[0]
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("author name")]
    );
    assert_eq!(
        fields[1]
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("number"), Some("of"), Some("journal")]
    );
}

#[test]
fn reference_blocks_report_direct_text_and_inline_content() {
    let name = SourceName::new("mdoc-reference-content.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%A author\nunexpected prose\n.Em unexpected emphasis\n.Re\n",
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
                DiagnosticCode::MDOC_REFERENCE_CONTENT,
                "invalid content in Rs block: text",
            ),
            (
                DiagnosticCode::MDOC_REFERENCE_CONTENT,
                "invalid content in Rs block: Em",
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
    assert_eq!(positions, [(7, 1), (8, 2)]);
}

#[test]
fn reference_blocks_report_any_non_bibliographic_direct_macro() {
    let name = SourceName::new("mdoc-reference-macro-content.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.%A author\n.Tg target\n.Re\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::MDOC_REFERENCE_CONTENT,
            "invalid content in Rs block: Tg",
        )]
    );
}

#[test]
fn reference_blocks_leave_their_first_direct_child_unvalidated() {
    let name = SourceName::new("mdoc-reference-first-child.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.Tg target\n.%A author\n.Re\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty());
}

#[test]
fn transparent_tags_remain_destinations_around_reference_blocks() {
    let name = SourceName::new("mdoc-reference-transparent-tags.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Tg before\n.Rs\n.%A author\n.Re\n.Rs\n.%A author\n.Tg inside\n.Re\n",
            ))
            .unwrap();
    let targets = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("Tg"))
        .map(|node| (node.flags().deep_link_target, node.tag()))
        .collect::<Vec<_>>();
    assert_eq!(targets, [(true, None), (true, None)]);
}

#[test]
fn empty_reference_blocks_report_at_their_openers() {
    let name = SourceName::new("mdoc-empty-reference-blocks.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\n.Rs\n.Re\n.Rs\n",
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
                DiagnosticCode::MDOC_EMPTY_REFERENCE_BLOCK,
                "empty reference block: Rs",
            ),
            (
                DiagnosticCode::MDOC_EMPTY_REFERENCE_BLOCK,
                "empty reference block: Rs",
            ),
            (
                DiagnosticCode::MDOC_UNCLOSED_BLOCK,
                "appending missing end of block: Rs",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.primary.as_ref())
        .filter_map(|span| report.document.source_position(span))
        .map(|position| (position.line, position.column))
        .collect::<Vec<_>>();
    assert_eq!(positions, [(5, 2), (7, 2), (7, 2)]);
}

#[test]
fn reference_heads_discard_arguments_after_the_leading_selector_diagnostic() {
    let name = SourceName::new("mdoc-reference-head-arguments.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt REFERENCES 1\n.Os\n.Sh SEE ALSO\n.Rs bogus\n.%A author\n.Re\n.Rs Sy bogus\n.%A author\n.Re\n",
            ))
            .unwrap();
    let heads = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Rs"))
        .collect::<Vec<_>>();
    assert_eq!(heads.len(), 2);
    assert!(heads.iter().all(|head| head.children().next().is_none()));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::MDOC_ARGUMENTS,
                "skipping all arguments: Rs bogus"
            ),
            (
                DiagnosticCode::MDOC_ARGUMENTS,
                "skipping all arguments: Rs Sy"
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
    assert_eq!(positions, [(5, 5), (8, 5)]);
}
