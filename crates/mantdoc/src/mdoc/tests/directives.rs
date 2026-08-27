use super::*;

#[test]
fn fd_rebases_later_argument_locations_after_string_expansion() {
    let name = SourceName::new("fd-expansion-location.2").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FD 2\n.Os\n.Sh DESCRIPTION\n.ds s \\(sh\n.Fd \\*sunquoted unescaped\n",
            ))
            .unwrap();
    let fd = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Fd"))
        .unwrap();
    let second = fd.children().nth(1).unwrap();
    assert_eq!(second.text(), Some("unescaped"));
    let position = report
        .document
        .source_position(second.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (6, 18));
}

#[test]
fn empty_fd_is_diagnosed_then_removed_from_public_flow() {
    let name = SourceName::new("fd-empty.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FD-EMPTY 1\n.Os\n.Sh SYNOPSIS\n.Fd\n.In stdlib.h\n.Sh DESCRIPTION\nleading\n.Fd\ntrailing\n",
            ))
            .unwrap();
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("Fd"))
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Fd"),
            (DiagnosticCode::MDOC_EMPTY_MACRO, "skipping empty macro: Fd"),
        ]
    );
}

#[test]
fn inline_macro_rebases_later_locations_after_string_expansion() {
    let name = SourceName::new("inline-expansion-location.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.Fl isolated \\*(Ba em\\*(Babedded\n",
            ))
            .unwrap();
    let expanded = report
        .document
        .preorder()
        .find(|node| node.text() == Some(r"em\fR|\fPbedded"))
        .unwrap_or_else(|| {
            panic!(
                "{:?}",
                report
                    .document
                    .preorder()
                    .map(|node| (node.macro_name(), node.text()))
                    .collect::<Vec<_>>()
            )
        });
    let position = report
        .document
        .source_position(expanded.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (5, 22));
}

#[test]
fn option_rebases_nested_children_after_string_expansion() {
    let name = SourceName::new("option-expansion-location.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OPTION 1\n.Os\n.Sh SYNOPSIS\n.Op Fl c Ar string \\*(Ba Fl s \\*(Ba Ar file Op Ar argument ...\n",
            ))
            .unwrap();
    let ellipsis = report
        .document
        .preorder()
        .find(|node| node.text() == Some("..."))
        .unwrap();
    let position = report
        .document
        .source_position(ellipsis.location().unwrap())
        .unwrap();

    assert_eq!((position.line, position.column), (5, 64));
}

#[test]
fn empty_ad_is_discarded_before_delimiter_style_validation() {
    let name = SourceName::new("mdoc-ad-empty.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt AD-EMPTY 1\n.Os\n.Sh DESCRIPTION\n.Ad 0x3bc.\n.Ad\nend\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping empty macro: Ad",
            "no blank before trailing delimiter: Ad 0x3bc.",
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
    assert_eq!(positions, [(6, 2), (5, 10)]);
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Ad"))
            .count(),
        1
    );
}

#[test]
fn an_options_are_private_and_validate_empty_duplicate_and_excess_forms() {
    let name = SourceName::new("mdoc-an-options.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AN-OPTIONS 1\n.Os\n.Sh AUTHORS\n.An -split -nosplit author\n.An\n.An Ingo,\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping duplicate argument: An -nosplit",
            "skipping excess arguments: An ... author",
            "skipping empty macro: An",
            "no blank before trailing delimiter: An Ingo,",
        ]
    );
    let author = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("An") && node.author_mode().is_some())
        .unwrap();
    assert_eq!(author.author_mode(), Some(AuthorMode::Split));
    assert_eq!(
        author
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("author")]
    );
}

#[test]
fn structures_metadata_sections_lists_displays_and_fonts() {
    let name = SourceName::new("mdoc-structure.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SAMPLE 1\n.Os ExampleOS\n.Sh NAME\n.Nm sample\n.Nd sample manual\n.Sh DESCRIPTION\n.Pp\nparagraph\n.Bl -bullet -compact -offset indent\n.It\nitem\n.El\n.Bd -literal -offset 2n\nliteral\n.Ed\n.Bf -emphasis\nstyled\n.Ef\n",
            ))
            .unwrap();
    let document = &report.document;
    assert_eq!(document.macro_set(), MacroSet::Mdoc);
    assert_eq!(document.metadata().title.as_deref(), Some("SAMPLE"));
    assert_eq!(document.metadata().section.as_deref(), Some("1"));
    assert_eq!(document.metadata().os.as_deref(), Some("ExampleOS"));
    assert_eq!(document.metadata().name.as_deref(), Some("sample"));
    assert_eq!(document.metadata().date.as_deref(), Some("August 25, 2026"));

    let nodes = document.preorder().collect::<Vec<_>>();
    for control in ["Dd", "Dt", "Os"] {
        assert!(
            nodes
                .iter()
                .any(|node| node.macro_name() == Some(control) && node.flags().no_print)
        );
    }
    let list = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
        .unwrap();
    assert_eq!(list.list_kind(), Some(NormalizedListKind::Bullet));
    assert!(list.compact());
    assert_eq!(list.offset(), Some("indent"));
    assert_eq!(list.width(), Some("2n"));
    let item = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .unwrap();
    assert_eq!(
        item.children()
            .nth(1)
            .unwrap()
            .children()
            .next()
            .unwrap()
            .text(),
        Some("item")
    );

    let display = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
        .unwrap();
    assert_eq!(display.display_kind(), Some(DisplayKind::Literal));
    assert_eq!(display.offset(), Some("2n"));
    assert!(
        display
            .children()
            .nth(1)
            .unwrap()
            .children()
            .next()
            .unwrap()
            .flags()
            .no_fill
    );
    let font = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bf"))
        .unwrap();
    assert_eq!(font.font(), Some(NormalizedFont::Emphasis));
}

#[test]
fn mdoc_retains_only_preamble_comments_in_the_public_tree() {
    let name = SourceName::new("mdoc-comments.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".\\\" preamble\n.Dd August 25, 2026\n.Dt COMMENTS 1\n.Os\n.\\\" internal\n.Sh DESCRIPTION\nbody\n",
            ))
            .unwrap();
    let comments = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Comment)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(comments, [" preamble"]);
}

#[test]
fn name_metadata_excludes_zero_width_formatter_spelling() {
    let name = SourceName::new("metadata-nm.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt SAMPLE 1\n.Os\n.Sh NAME\n.Nm \\&sample-name\n",
        ))
        .unwrap();
    assert_eq!(
        report.document.metadata().name.as_deref(),
        Some("sample-name")
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("\\&sample-name"))
    );
}

#[test]
fn normalizes_mdoc_macro_layout_widths_without_rewriting_source_arguments() {
    let name = SourceName::new("mdoc-layout-width.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt WIDTH 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It term\nbody\n.El\n.Bl -inset\n.It term\nbody\n.El\n.Bl -enum\n.It item\nbody\n.El\n.Bd -offset Fl\nbody\n.Ed\n",
            ))
            .unwrap();
    let list = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
        .unwrap();
    assert_eq!(list.width(), Some("6n"));
    let widths = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bl"))
        .map(crate::NodeRef::width)
        .collect::<Vec<_>>();
    assert_eq!(widths, [Some("6n"), None, Some("3n")]);
    let display = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
        .unwrap();
    assert_eq!(display.offset(), Some("10n"));
}

#[test]
fn display_options_use_first_type_and_keep_validation_out_of_the_public_tree() {
    let name = SourceName::new("mdoc-display-options.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DISPLAY-OPTIONS 1\n.Os\n.Sh DESCRIPTION\n.Bd -ragged -compact -unfilled\nvisible\n.Ed tail\n.Bd\nrelinked\n.Ed\n",
            ))
            .unwrap();
    let displays = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bd"))
        .collect::<Vec<_>>();
    assert_eq!(displays.len(), 1);
    assert_eq!(displays[0].display_kind(), Some(DisplayKind::Filled));
    assert!(displays[0].compact());
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("relinked"))
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping all arguments: Ed tail",
            "skipping duplicate display type: Bd -unfilled",
            "skipping display without arguments: Bd",
        ]
    );
}

#[test]
fn list_item_heads_coalesce_plain_phrases_but_preserve_column_cells() {
    let name = SourceName::new("mdoc-list-item-heads.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt LISTHEADS 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It outer tag\nbody\n.El\n.Bl -column first second\n.It left right\n.El\n",
            ))
            .unwrap();
    let item_heads = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(item_heads.len(), 2);
    assert_eq!(
        item_heads[0]
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("outer tag")]
    );
    assert_eq!(
        item_heads[1]
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        []
    );
    let column_item = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .nth(1)
        .unwrap();
    let column_cells = column_item
        .children()
        .filter(|node| node.kind() == NodeKind::Body)
        .map(|body| {
            body.children()
                .map(crate::NodeRef::text)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(column_cells, vec![vec![Some("left"), Some("right")]]);
}

#[test]
fn diagnostic_list_item_heads_remain_literal_and_skip_empty_no() {
    let name = SourceName::new("mdoc-diag-list-literals.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DIAG 1\n.Os\n.Sh DESCRIPTION\n.Bl -diag\n.It Nx\n.No Nx\n.It Fl flag\nbody\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["skipping empty macro: No"]
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (7, 2));

    let items = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    let first_head = items[0]
        .children()
        .find(|node| node.kind() == NodeKind::Head)
        .unwrap();
    assert_eq!(
        first_head
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("Nx")]
    );
    let second_head = items[1]
        .children()
        .find(|node| node.kind() == NodeKind::Head)
        .unwrap();
    assert_eq!(
        second_head
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("Fl flag")]
    );
    let first_body = items[0]
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    let nx = first_body.children().next().unwrap();
    assert_eq!(nx.macro_name(), Some("Nx"));
    assert!(nx.flags().line_start);
    assert_eq!(
        nx.children().next().and_then(crate::NodeRef::text),
        Some("NetBSD")
    );
}

#[test]
fn empty_no_requests_are_removed_and_keep_source_ordered_findings() {
    let name = SourceName::new("mdoc-empty-no.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY-NO 1\n.Os\n.Sh DESCRIPTION\n.No ( No b\n.No a No (\n.No \".\"\n.No a.\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping empty macro: No",
            "skipping empty macro: No",
            "no blank before trailing delimiter: No a.",
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .map(|position| (position.line, position.column))
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [Some((6, 7)), Some((7, 2)), Some((8, 6))]);

    let nodes = report.document.preorder().collect::<Vec<_>>();
    assert!(
        !nodes
            .iter()
            .any(|node| { node.macro_name() == Some("No") && node.children().next().is_none() })
    );
    assert!(nodes.iter().any(|node| node.text() == Some("(")));
    assert!(nodes.iter().any(|node| node.text() == Some(".")));
}

#[test]
fn no_space_macro_reports_only_invalid_source_positions() {
    let name = SourceName::new("mdoc-no-space-position.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt NO-SPACE 1\n.Os\n.Sh DESCRIPTION\n.Ns Op after\n.Oo before Oc Ns : Op after\n.Oo before Oc : Ns Op after\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.no-space-macro", "skipping no-space macro"),
            ("mdoc.no-space-macro", "skipping no-space macro"),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .map(|position| (position.line, position.column))
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [Some((5, 2)), Some((6, 15))]);
}
