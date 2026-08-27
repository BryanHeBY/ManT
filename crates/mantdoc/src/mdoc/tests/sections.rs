use super::*;

#[test]
fn section_and_subsection_titles_are_single_semantic_phrases() {
    let name = SourceName::new("mdoc-section-phrases.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh SEE ALSO\n.Ss Further Reading\n",
        ))
        .unwrap();
    let headings = report
        .document
        .preorder()
        .filter(|node| {
            node.kind() == NodeKind::Head && matches!(node.macro_name(), Some("Sh" | "Ss"))
        })
        .collect::<Vec<_>>();
    assert_eq!(headings.len(), 2);
    assert_eq!(
        headings
            .iter()
            .map(|head| head.children().next().and_then(crate::NodeRef::text))
            .collect::<Vec<_>>(),
        [Some("SEE ALSO"), Some("Further Reading")]
    );
    assert!(headings.iter().all(|head| head.children().nth(1).is_none()));
}

#[test]
fn section_title_validation_uses_inline_visible_text() {
    let name = SourceName::new("mdoc-section-inline-visible.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh SEE ALSO\n.Sh SEE Em ALSO\n",
        ))
        .unwrap();
    let head = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Sh"))
        .nth(1)
        .unwrap();
    assert_eq!(head.children().count(), 2);
    assert_eq!(
        head.children().next().and_then(crate::NodeRef::text),
        Some("SEE")
    );
    assert_eq!(
        head.children().nth(1).and_then(crate::NodeRef::macro_name),
        Some("Em")
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::MDOC_DUPLICATE_SECTION,
            "duplicate section title: Sh SEE ALSO",
        )]
    );
}

#[test]
fn first_section_validation_uses_the_visible_heading() {
    let name = SourceName::new("mdoc-first-section.1").unwrap();
    let report = crate::Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh DESCRIPTION\ntext\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME,
            "first section is not \"NAME\": Sh DESCRIPTION",
        )]
    );
}

#[test]
fn empty_section_headers_report_without_creating_blocks() {
    let name = SourceName::new("mdoc-empty-section-heads.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SECTIONS 1\n.Os\n.Sh NAME\n.Nm sections\n.Nd example\n.Sh\n.Ss\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Sh"),
            (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Ss"),
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| {
                node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("Sh" | "Ss"))
            })
            .count(),
        1
    );
}

#[test]
fn a_section_header_partial_block_is_closed_by_the_next_section() {
    let name = SourceName::new("mdoc-section-header-partial.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt HEADER-PARTIAL 1\n.Os\n.Sh SYNOPSIS\n.Sh DESCRIPTION Xo\n.Sh BUGS\nknown issue\n",
            ))
            .unwrap();
    let description = report
        .document
        .preorder()
        .find(|node| {
            node.kind() == NodeKind::Block
                && node.macro_name() == Some("Sh")
                && node
                    .children()
                    .find(|child| child.kind() == NodeKind::Head)
                    .and_then(|head| head.children().next())
                    .and_then(crate::NodeRef::text)
                    == Some("DESCRIPTION")
        })
        .unwrap();
    let description_head = description
        .children()
        .find(|node| node.kind() == NodeKind::Head)
        .unwrap();
    let xo = description_head
        .children()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Xo"))
        .unwrap();
    assert_eq!(xo.children().count(), 2);
    let description_body = description
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    assert!(description_body.flags().line_start);
    let bugs = report
        .document
        .preorder()
        .find(|node| {
            node.kind() == NodeKind::Block
                && node.macro_name() == Some("Sh")
                && node
                    .children()
                    .find(|child| child.kind() == NodeKind::Head)
                    .and_then(|head| head.children().next())
                    .and_then(crate::NodeRef::text)
                    == Some("BUGS")
        })
        .unwrap();
    assert!(!bugs.flags().line_start);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::MDOC_BROKEN_BLOCK,
            "inserting missing end of block: Sh breaks Xo",
        )]
    );
}

#[test]
fn a_mismatched_partial_closer_reports_without_closing_the_active_scope() {
    let name = SourceName::new("mdoc-partial-not-open.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt PARTIAL-NOT-OPEN 1\n.Os\n.Sh DESCRIPTION\n.Ao ao\n.Bo bo pc\n.Pc bc\n.Bc ac\n.Ac tail\n",
            ))
            .unwrap();
    let bracket_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        .unwrap();
    assert_eq!(
        bracket_body
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("bo pc bc")]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::MDOC_UNMATCHED_CLOSE,
            "skipping end of block that is not open: Pc",
        )]
    );
}

#[test]
fn configuration_directives_join_plain_arguments_before_trailing_punctuation() {
    let name = SourceName::new("mdoc-cd-phrase.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt CONFIG 1\n.Os\n.Sh DESCRIPTION\n.Cd options INSECURE .\n",
        ))
        .unwrap();
    let directive = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Cd"))
        .unwrap();
    assert_eq!(
        directive
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("options INSECURE")]
    );
    let period = report
        .document
        .preorder()
        .find(|node| node.text() == Some("."))
        .unwrap();
    assert!(period.flags().delimiter_close);
    assert!(period.flags().sentence_end);
}

#[test]
fn empty_configuration_directive_is_discarded_with_a_typed_warning() {
    let name = SourceName::new("mdoc-empty-cd.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt EMPTY-CD 1\n.Os\n.Sh DESCRIPTION\n.Cd\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["skipping empty macro: Cd"]
    );
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("Cd"))
    );
}

#[test]
fn empty_command_modifiers_report_without_leaking_private_elements() {
    let name = SourceName::new("mdoc-cm-noarg.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt CM 1\n.Os\n.Sh DESCRIPTION\n.Nm mt Fl f Ar device Cm\n.Nm ps Fl x Cm Fl o Cm command.\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping empty macro: Cm",
            "skipping empty macro: Cm",
            "no blank before trailing delimiter: Cm command.",
        ]
    );
    assert!(
        report
            .document
            .preorder()
            .all(|node| { node.macro_name() != Some("Cm") || node.children().next().is_some() })
    );
}

#[test]
fn cd_leading_delimiters_stay_in_outer_flow_before_reopening() {
    let name = SourceName::new("mdoc-cd-leading-delimiters.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt CD 1\n.Os\n.Sh DESCRIPTION\n.Cd ) z\n.Cd ( a\n.Cd | m\n.Cd )\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    for punctuation in [")", "(", "|"] {
        let node = nodes
            .iter()
            .copied()
            .find(|node| node.text() == Some(punctuation) && node.flags().line_start)
            .unwrap();
        assert!(!node.flags().sentence_end);
        assert!(!node.flags().delimiter_close);
    }
    let opening = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("(") && node.flags().line_start)
        .unwrap();
    assert!(opening.flags().delimiter_open);
    assert_eq!(
        nodes
            .iter()
            .copied()
            .filter(|node| node.macro_name() == Some("Cd"))
            .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
            .collect::<Vec<_>>(),
        ["z", "a", "m"]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["skipping empty macro: Cd"]
    );
}

#[test]
fn ic_delimiters_reopen_only_after_visible_words() {
    let name = SourceName::new("mdoc-ic-leading-delimiters.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt IC 1\n.Os\n.Sh DESCRIPTION\n.Ic ) z\n.Ic ( a\n.Ic | m\n.Ic )\n.Ic ) )\n",
            ))
            .unwrap();
    let body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
        .unwrap();
    let children = body.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 9);
    for (index, punctuation) in [(0, ")"), (2, "("), (4, "|"), (6, ")"), (7, ")")] {
        assert_eq!(children[index].text(), Some(punctuation));
    }
    assert!(children[0].flags().line_start);
    assert!(!children[0].flags().delimiter_close);
    assert!(children[2].flags().delimiter_open);
    assert!(!children[4].flags().delimiter_close);
    assert!(children[6].flags().line_start);
    assert!(!children[6].flags().delimiter_close);
    assert!(children[7].flags().line_start);
    assert!(!children[7].flags().delimiter_close);
    assert!(children[8].flags().delimiter_close);
    assert_eq!(
        [children[1], children[3], children[5]]
            .into_iter()
            .map(|node| (
                node.macro_name(),
                node.children().next().and_then(crate::NodeRef::text),
            ))
            .collect::<Vec<_>>(),
        [
            (Some("Ic"), Some("z")),
            (Some("Ic"), Some("a")),
            (Some("Ic"), Some("m")),
        ]
    );
    assert!(
        children
            .iter()
            .all(|node| node.macro_name() != Some("Ic") || node.children().next().is_some())
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["skipping empty macro: Ic", "skipping empty macro: Ic"]
    );
}

#[test]
fn nested_tag_trailing_punctuation_marks_only_a_terminal_sentence() {
    let name = SourceName::new("mdoc-nested-tag-terminal-punctuation.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt TAGS 1\n.Os\n.Sh DESCRIPTION\n.Li a Li .\n.Li a Li . Li b\n",
        ))
        .unwrap();
    let periods = report
        .document
        .preorder()
        .filter(|node| node.text() == Some("."))
        .collect::<Vec<_>>();
    assert_eq!(periods.len(), 2);
    assert!(periods[0].flags().sentence_end);
    assert!(!periods[0].flags().delimiter_close);
    assert!(!periods[1].flags().sentence_end);
    assert!(!periods[1].flags().delimiter_close);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::MDOC_EMPTY_MACRO,
            DiagnosticCode::MDOC_EMPTY_MACRO
        ]
    );
}

#[test]
fn link_macros_retain_internal_delimiters_and_validate_empty_forms() {
    let name = SourceName::new("mdoc-link-recovery.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LINKS 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.test/ ,\n.Lk https://example.test/ label,\n.Lk\n",
            ))
            .unwrap();
    let links = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Lk"))
        .collect::<Vec<_>>();
    assert_eq!(links.len(), 2);
    let first_children = links[0].children().collect::<Vec<_>>();
    assert_eq!(first_children.len(), 2);
    assert_eq!(first_children[0].text(), Some("https://example.test/"));
    assert_eq!(first_children[1].text(), Some(","));
    assert!(first_children[1].flags().delimiter_close);
    assert_eq!(
        links[1]
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["https://example.test/", "label,"]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping empty macro: Lk",
            "no blank before trailing delimiter: Lk ... label,",
        ]
    );
}

#[test]
fn explicit_tg_before_a_column_list_moves_destination_to_its_body() {
    let name = SourceName::new("mdoc-tg-column-list.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TGLIST 1\n.Os\n.Sh DESCRIPTION\n.Tg list\n.Bl -column one two\n.It one Ta two\n.El\n",
            ))
            .unwrap();
    let list_body = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bl"))
        .unwrap();
    assert!(list_body.flags().deep_link_target);
    assert!(!list_body.flags().permalink);
    assert_eq!(list_body.tag(), Some("list"));
    let tg = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(tg.flags().no_print);
}

#[test]
fn column_lists_materialize_rows_without_explicit_item_controls() {
    let name = SourceName::new("mdoc-column-implicit-items.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column one two\n.Sy a Ta b\n.Em c Ta d\n.El\n.Bl -column one two\na\tb\nc\td\n.El\n",
            ))
            .unwrap();
    let items = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 4);
    assert!(items[0].flags().deep_link_target);
    assert_eq!(items[0].tag(), Some("a"));
    assert!(
        items[0]
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap()
            .children()
            .any(|node| node.macro_name() == Some("Sy") && node.flags().permalink)
    );
    for item in items {
        assert_eq!(
            item.children()
                .filter(|node| node.kind() == NodeKind::Body)
                .count(),
            2
        );
    }
}

#[test]
fn column_lists_group_consecutive_tbl_rows_in_one_implicit_item() {
    let name = SourceName::new("mdoc-column-tbl-item.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN-TBL 1\n.Os\n.Sh DESCRIPTION\n.Bl -column one two\n.Sy a Ta b\n.TS\nll.\n1\t2\n3\t4\n.TE\n.Em c Ta d\n.El\n",
            ))
            .unwrap();
    let items = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 3);
    let table_item = items[1];
    let head = table_item
        .children()
        .find(|node| node.kind() == NodeKind::Head)
        .unwrap();
    assert_eq!(head.children().count(), 0);
    let tables = table_item
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap()
        .children()
        .filter(|node| node.kind() == NodeKind::Table)
        .count();
    assert_eq!(tables, 2);
}

#[test]
fn list_item_headers_can_extend_through_explicit_partial_blocks() {
    let name = SourceName::new("mdoc-item-header-extension.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EXTEND 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Ao\n.No extended tag\n.Ac\nextended text\n.It prefix Ao\n.No prefixed tag\n.Ac\nprefixed text\n.El\n",
            ))
            .unwrap();
    let items = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    for item in items {
        let head = item
            .children()
            .find(|node| node.kind() == NodeKind::Head)
            .unwrap();
        let enclosure = head
            .children()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Ao"))
            .unwrap();
        assert!(
            enclosure
                .children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap()
                .children()
                .any(|node| node.macro_name() == Some("No"))
        );
        assert!(
            item.children()
                .find(|node| node.kind() == NodeKind::Body)
                .unwrap()
                .flags()
                .line_start
        );
    }
}

#[test]
fn explicit_tg_before_list_items_selects_the_legacy_item_part() {
    let name = SourceName::new("mdoc-tg-list-item.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TGLISTITEM 1\n.Os\n.Sh DESCRIPTION\n.Bl -dash\n.Tg bullet\n.It\nbody\n.El\n.Bl -tag\n.Tg term\n.It name\nbody\n.El\n",
            ))
            .unwrap();
    let items = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    let bullet_body = items[0]
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    assert!(bullet_body.flags().deep_link_target);
    assert!(bullet_body.flags().permalink);
    assert_eq!(bullet_body.tag(), Some("bullet"));
    let definition_head = items[1]
        .children()
        .find(|node| node.kind() == NodeKind::Head)
        .unwrap();
    assert!(definition_head.flags().deep_link_target);
    assert!(definition_head.flags().permalink);
    assert_eq!(definition_head.tag(), Some("term"));
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Tg"))
            .filter(|node| node.flags().no_print)
            .count(),
        2
    );
}

#[test]
fn list_item_long_option_prefix_collapses_adjacent_fl_macros() {
    let name = SourceName::new("mdoc-list-long-option.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LONGOPTION 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It Fl Fl long\nbody\n.El\n",
            ))
            .unwrap();
    let item_head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
        .unwrap();
    let flag = item_head.children().next().unwrap();
    assert_eq!(flag.macro_name(), Some("Fl"));
    assert_eq!(
        flag.children().next().and_then(crate::NodeRef::text),
        Some("\\-long")
    );
    assert_eq!(item_head.children().count(), 1);
    assert_eq!(item_head.tag(), Some("long"));
    assert!(flag.flags().permalink);
}

#[test]
fn font_blocks_accept_legacy_macro_name_aliases() {
    let name = SourceName::new("mdoc-bf-aliases.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf Em\n.Bf Li\n.Bf Sy\n.Ef\n.Ef\n.Ef\n",
            ))
            .unwrap();
    let blocks = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bf"))
        .collect::<Vec<_>>();
    let fonts = blocks.iter().map(|block| block.font()).collect::<Vec<_>>();
    assert_eq!(
        fonts,
        [
            Some(NormalizedFont::Emphasis),
            Some(NormalizedFont::Literal),
            Some(NormalizedFont::Symbolic),
        ]
    );
    assert_eq!(
        blocks
            .iter()
            .map(|block| block
                .children()
                .next()
                .and_then(|head| head.children().next()))
            .map(|word| word.and_then(crate::NodeRef::text))
            .collect::<Vec<_>>(),
        [Some("Em"), Some("Li"), Some("Sy")]
    );
}

#[test]
fn emphasis_coalesces_a_plain_argument_phrase() {
    let name = SourceName::new("mdoc-em-phrase.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt EM 1\n.Os\n.Sh DESCRIPTION\n.Em several plain words\n",
        ))
        .unwrap();
    let emphasis = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Em"))
        .unwrap();
    assert_eq!(emphasis.children().count(), 1);
    assert_eq!(
        emphasis.children().next().and_then(crate::NodeRef::text),
        Some("several plain words")
    );
}

#[test]
fn paragraphs_are_elements_and_tg_moves_its_permalink_to_following_text() {
    let name = SourceName::new("mdoc-tg-paragraph.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAG 1\n.Os\n.Sh NAME\n.Nm tag\n.Nd tag test\n.Sh DESCRIPTION\n.Tg anchor\n.Pp\nalpha beta\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let paragraph = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(paragraph.flags().deep_link_target);
    assert!(!paragraph.flags().permalink);
    assert_eq!(paragraph.tag(), Some("anchor"));

    let tg = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(tg.flags().no_print);

    let alpha = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("alpha"))
        .unwrap();
    let beta = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("beta"))
        .unwrap();
    assert!(alpha.flags().permalink);
    assert_eq!(alpha.tag(), Some("anchor"));
    assert!(!beta.flags().permalink);
    assert_eq!(beta.tag(), None);
    assert!(!beta.flags().line_start);
}

#[test]
fn tg_recovers_invalid_spelling_and_keeps_consecutive_destination_topology() {
    let name = SourceName::new("mdoc-tg-recovery.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt TG-RECOVERY 1\n.Os\n.Sh DESCRIPTION\nintro\n.Pp\n.Tg start\ntext\n.Tg sub\n.Tg double\n.Ss Details\n.Tg \"\" ignored\n.Tg \\&bad\n.Tg\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .filter(|code| {
                matches!(
                    *code,
                    "mdoc.empty-macro" | "mdoc.arguments" | "mdoc.invalid-tag"
                )
            })
            .collect::<Vec<_>>(),
        [
            "mdoc.empty-macro",
            "mdoc.arguments",
            "mdoc.invalid-tag",
            "mdoc.empty-macro",
        ]
    );

    let nodes = report.document.preorder().collect::<Vec<_>>();
    let paragraph = nodes
        .iter()
        .copied()
        .find(|node| node.macro_name() == Some("Pp"))
        .unwrap();
    assert_eq!(paragraph.tag(), Some("start"));
    let sub = nodes
        .iter()
        .copied()
        .find(|node| {
            node.macro_name() == Some("Tg")
                && node.children().next().and_then(crate::NodeRef::text) == Some("sub")
        })
        .unwrap();
    assert!(sub.flags().deep_link_target);
    assert_eq!(sub.tag(), None);
    let subsection = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Ss"))
        .unwrap();
    assert_eq!(subsection.tag(), Some("double"));
    assert!(nodes.iter().copied().all(|node| {
        !(node.macro_name() == Some("Tg")
            && node.children().next().and_then(crate::NodeRef::text) == Some("\\&bad"))
    }));
}
