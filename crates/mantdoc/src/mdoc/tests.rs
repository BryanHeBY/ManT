use crate::{
    AuthorMode, DiagnosticCode, DisplayKind, MacroSet, NodeKind, NormalizedFont,
    NormalizedListKind, Severity, Source, SourceName,
};

/// Most mdoc unit fixtures intentionally start with the construct under
/// test, commonly a `DESCRIPTION` section.  Keep their assertions focused
/// on that construct; the production parser still emits the prologue
/// warning and `first_section_validation_uses_the_visible_heading` below
/// covers it directly.
#[derive(Default)]
struct Parser(crate::Parser);

impl Parser {
    fn parse(&self, source: Source<'_>) -> Result<crate::ParseReport, crate::FatalError> {
        let mut report = self.0.parse(source)?;
        report.diagnostics.retain(|diagnostic| {
            diagnostic.code.as_str() != DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
        });
        Ok(report)
    }
}

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

#[test]
fn parsed_inline_macros_diagnose_known_noncallable_spellings() {
    let name = SourceName::new("mdoc-non-callable-inline.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt NONCALLABLE 1\n.Os\n.Sh DESCRIPTION\n.Ic Dd\n.Ic \\&Dd\n.In Dd\n",
            ))
            .unwrap();
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MDOC_NON_CALLABLE_MACRO)
        .unwrap();
    assert_eq!(
        diagnostic.message.as_ref(),
        "macro neither callable nor escaped: Dd"
    );
    let position = diagnostic
        .primary
        .as_ref()
        .and_then(|span| report.document.source_position(span))
        .unwrap();
    assert_eq!((position.line, position.column), (5, 5));
    assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code.as_str()
                    == DiagnosticCode::MDOC_NON_CALLABLE_MACRO)
                .count(),
            1
        );
}

#[test]
fn callable_inline_macros_split_scanner_tokens_without_losing_delimiters() {
    let name = SourceName::new("mdoc-inline-sequence.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh NAME\n.Nm inline\n.Nd inline test\n.Sh DESCRIPTION\n.Nm tool Fl f Ar path Cm pid , Ns Cm command\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    for (macro_name, text) in [("Fl", "f"), ("Ar", "path"), ("Cm", "pid")] {
        assert!(nodes.iter().copied().any(|node| {
            node.kind() == NodeKind::Element
                && node.macro_name() == Some(macro_name)
                && node.children().next().and_then(crate::NodeRef::text) == Some(text)
        }));
    }
    let delimiter = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some(","))
        .unwrap();
    assert!(delimiter.flags().delimiter_close);
    let no_space = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ns"))
        .unwrap();
    assert_eq!(no_space.children().count(), 0);
}

#[test]
fn ar_reopens_around_mdoc_delimiters_without_synthesizing_empty_defaults() {
    let name = SourceName::new("mdoc-ar-punctuation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AR 1\n.Os\n.Sh DESCRIPTION\n.Ar | m\n.Ar ( a\n.Ar a \"(\" b\n.Ar . z\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let arguments = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ar"))
        .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
        .collect::<Vec<_>>();
    assert_eq!(arguments, ["m", "a", "a", "b", "file", "z"]);
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.text() == Some("file") && node.flags().generated)
            .count(),
        1,
        "only the closing-delimiter form defaults; the initial `|` does not"
    );
    let opening = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("(") && node.flags().delimiter_open)
        .unwrap();
    assert!(opening.flags().line_start);
    let dot = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("."))
        .unwrap();
    assert!(dot.flags().delimiter_close);
    assert!(!dot.flags().sentence_end);
}

#[test]
fn formatter_reset_wrapped_bar_reopens_mdoc_inline_macro() {
    let name = SourceName::new("mdoc-inline-reset-bar.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.Fl isolated | em|bedded \\fR|\\fP formatted\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let arguments = nodes
        .iter()
        .copied()
        .filter(|node| node.macro_name() == Some("Fl"))
        .filter_map(|node| node.children().next().and_then(crate::NodeRef::text))
        .collect::<Vec<_>>();
    assert_eq!(arguments, ["isolated", "em|bedded", "formatted"]);
    assert!(
        nodes
            .iter()
            .copied()
            .any(|node| node.text() == Some(r"\fR|\fP"))
    );
}

#[test]
fn symbolic_inline_macro_coalesces_an_unsplit_source_phrase() {
    let name = SourceName::new("mdoc-symbolic-phrase.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.Sy isolated \\(ba em\\(babedded \\fR\\(ba\\fP formatted\n",
            ))
            .unwrap();
    let symbolic = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Sy"))
        .unwrap();
    assert_eq!(symbolic.children().count(), 1);
    assert_eq!(
        symbolic.children().next().and_then(crate::NodeRef::text),
        Some(r"isolated \(ba em\(babedded \fR\(ba\fP formatted")
    );
}

#[test]
fn filled_mdoc_text_trims_physical_line_end_whitespace() {
    let name = SourceName::new("mdoc-trailing-whitespace.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt WHITESPACE 1\n.Os\n.Sh DESCRIPTION\nvisible  \n.Bd -literal\nliteral  \n.Ed\n",
            ))
            .unwrap();
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"visible"));
    assert!(text.contains(&"literal"));
    assert!(!text.iter().any(|value| value.ends_with([' ', '\t'])));
}

#[test]
fn system_name_macros_insert_generated_words_and_leave_periods_in_flow() {
    let name = SourceName::new("mdoc-system-names.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYSTEMS 1\n.Os\n.Sh DESCRIPTION\n.Ux .\n.Bx .\n.Bsx .\n.Nx .\n.Fx .\n.Ox .\n.Dx .\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    for (macro_name, generated) in [
        ("Ux", "UNIX"),
        ("Bx", "BSD"),
        ("Bsx", "BSD/OS"),
        ("Nx", "NetBSD"),
        ("Fx", "FreeBSD"),
        ("Ox", "OpenBSD"),
        ("Dx", "DragonFly"),
    ] {
        let system = nodes
            .iter()
            .copied()
            .find(|node| node.macro_name() == Some(macro_name))
            .unwrap();
        let word = system.children().next().unwrap();
        assert_eq!(word.text(), Some(generated));
        assert!(word.flags().generated);
    }
    let periods = nodes
        .iter()
        .copied()
        .filter(|node| node.text() == Some("."))
        .collect::<Vec<_>>();
    assert_eq!(periods.len(), 7);
    assert!(periods.iter().all(|node| node.flags().delimiter_close));
    assert!(periods.iter().all(|node| node.flags().sentence_end));
}

#[test]
fn compact_system_names_validate_attached_version_delimiters() {
    let name = SourceName::new("mdoc-system-name-delimiters.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt SYSTEMS 1\n.Os\n.Sh NAME\n.Nm systems\n.Nd delimiter validation\n.Sh DESCRIPTION\n.Bsx 5.1,\n.Dx 4.8.0,\n.Fx 11.0,\n.Nx 7.1,\n.Ox 6.1.\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "no blank before trailing delimiter: Bsx 5.1,",
            "no blank before trailing delimiter: Dx 4.8.0,",
            "no blank before trailing delimiter: Fx 11.0,",
            "no blank before trailing delimiter: Nx 7.1,",
            "no blank before trailing delimiter: Ox 6.1.",
        ]
    );
}

#[test]
fn implicit_partial_blocks_expand_nested_system_name_macros() {
    let name = SourceName::new("mdoc-partial-system-name.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt PARTIAL 1\n.Os\n.Sh DESCRIPTION\n.Op Fl Ux\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let ux = nodes
        .iter()
        .copied()
        .find(|node| node.macro_name() == Some("Ux"))
        .unwrap();
    let generated = ux.children().collect::<Vec<_>>();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].text(), Some("UNIX"));
    assert!(generated[0].flags().generated);

    let fl = nodes
        .iter()
        .copied()
        .find(|node| node.macro_name() == Some("Fl"))
        .unwrap();
    assert!(fl.children().next().is_none());
    assert_eq!(fl.parent().and_then(crate::NodeRef::macro_name), Some("Op"));
    assert_eq!(
        fl.parent()
            .unwrap()
            .children()
            .map(crate::NodeRef::macro_name)
            .collect::<Vec<_>>(),
        [Some("Fl"), Some("Ux")]
    );
    let ux_line = ux.source_position().unwrap().line;
    assert_eq!(fl.source_position().unwrap().line, ux_line);
}

#[test]
fn flags_validate_an_attached_trailing_delimiter_after_argument_expansion() {
    let name = SourceName::new("mdoc-flag-delimiter.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt FLAGS 1\n.Os\n.Sh DESCRIPTION\n.Fl a.\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["no blank before trailing delimiter: Fl a."]
    );
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (5, 6));
}

#[test]
fn a_flag_followed_by_es_keeps_the_enclosure_in_outer_flow() {
    let name = SourceName::new("mdoc-flag-es.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt FLAGS 1\n.Os\n.Sh DESCRIPTION\n.Fl Es < >\n",
        ))
        .unwrap();
    let children = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].macro_name(), Some("Fl"));
    assert!(children[0].children().next().is_none());
    assert_eq!(children[1].macro_name(), Some("Es"));
    assert_eq!(
        children[1]
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["<", ">"]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["obsolete macro: Es"]
    );
}

#[test]
fn cross_line_xo_closers_resume_their_control_line_tail() {
    let name = SourceName::new("mdoc-xo-tail.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt XO 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Xo Fl\n.Tg transparent\n.Xc suffix\n",
            ))
            .unwrap();
    let suffix = report
        .document
        .preorder()
        .find(|node| node.text() == Some("suffix"))
        .unwrap();
    assert_eq!(
        suffix.parent().and_then(crate::NodeRef::macro_name),
        Some("Sh")
    );
    assert!(suffix.flags().line_start);
    assert_eq!(
        suffix
            .location()
            .and_then(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column)),
        Some((8, 5))
    );
    let paragraph = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Pp"))
        .unwrap();
    assert_eq!(paragraph.tag(), Some("transparent"));
    assert!(paragraph.flags().deep_link_target);
    let transparent = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(transparent.flags().no_print);
}

#[test]
fn transparent_tags_after_empty_flags_split_targets_from_permalinks() {
    let name = SourceName::new("mdoc-transparent-flag-tag.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FLAGS 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Fl\n.Tg transparent\n.Em word\n",
            ))
            .unwrap();
    let paragraph = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Pp"))
        .unwrap();
    assert_eq!(paragraph.tag(), Some("transparent"));
    assert!(paragraph.flags().deep_link_target);
    let emphasis = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Em"))
        .unwrap();
    assert_eq!(emphasis.tag(), Some("transparent"));
    assert!(!emphasis.flags().deep_link_target);
    assert!(emphasis.flags().permalink);
}

#[test]
fn empty_function_declaration_macros_are_removed_after_validation() {
    let name = SourceName::new("mdoc-empty-function-declarations.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Fo function excess\n.Fa\n.Fc\n.Ft\n.Fn\n",
            ))
            .unwrap();
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
        .unwrap();
    assert_eq!(
        head.children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["function"]
    );
    assert!(
        report
            .document
            .preorder()
            .all(|node| { !matches!(node.macro_name(), Some("Fa" | "Fn" | "Ft")) })
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping empty macro: Fa",
            "skipping empty macro: Ft",
            "skipping empty macro: Fn",
            "skipping excess arguments: Fo ... excess",
        ]
    );
}

#[test]
fn repeated_automatic_function_spellings_keep_only_the_first_destination() {
    let name = SourceName::new("mdoc-repeated-function-targets.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Ft int\n.Fn abs \"int i\"\n.Ft int\n.Fn abs \"int i\"\n.Fo labs\n.Fc\n.Fo labs\n.Fc\n",
            ))
            .unwrap();
    let functions = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 2);
    assert!(functions[0].flags().deep_link_target);
    assert!(!functions[1].flags().deep_link_target);
    assert!(functions.iter().all(|node| node.tag().is_none()));

    let function_heads = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
        .collect::<Vec<_>>();
    assert_eq!(function_heads.len(), 2);
    assert!(function_heads[0].flags().deep_link_target);
    assert!(!function_heads[1].flags().deep_link_target);
    assert!(function_heads.iter().all(|node| node.tag().is_none()));
}

#[test]
fn empty_fo_head_retains_its_block_and_reports_the_missing_function_name() {
    let name = SourceName::new("mdoc-empty-fo-head.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Fo\n.Fa int\n.Fc\n",
        ))
        .unwrap();
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
        .unwrap();
    assert_eq!(head.children().count(), 0);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            "mdoc.function-name-missing",
            "missing function name, using \"\": Fo"
        )]
    );
}

#[test]
fn obsolete_function_macros_preserve_their_distinct_public_forms() {
    let name = SourceName::new("mdoc-obsolete-function-macros.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Ot fortran\n.Fr value\n",
        ))
        .unwrap();
    let macros = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::macro_name)
        .collect::<Vec<_>>();
    assert!(macros.contains(&"Ft"));
    assert!(macros.contains(&"Fr"));
    assert!(!macros.contains(&"Ot"));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.obsolete", "obsolete macro: Ot"),
            ("mdoc.obsolete", "obsolete macro: Fr"),
        ]
    );
}

#[test]
fn function_declaration_macros_defer_attached_punctuation_validation() {
    let name = SourceName::new("mdoc-function-punctuation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Ft double\n.Fn sin. \",\" cos \"Em\" italic\n.Pp\n.Fa x \",\" y: \"Sy\" bold\n.Pp\n.Ft int \",\" float: \"Sy\" bold\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.message.as_ref(),
                    diagnostic
                        .primary
                        .as_ref()
                        .and_then(|span| report.document.source_position(span))
                        .map(|position| (position.line, position.column)),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Fn sin.",
                Some((6, 8)),
            ),
            (
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Fa y:",
                Some((8, 12)),
            ),
            (
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Ft float:",
                Some((10, 18)),
            ),
        ]
    );
    let function = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Fn"))
        .unwrap();
    assert!(function.flags().deep_link_target);
    assert_eq!(function.tag(), None);
}

#[test]
fn mailto_macro_validates_attached_trailing_punctuation() {
    let name = SourceName::new("mdoc-mailto-punctuation.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt MAIL 1\n.Os\n.Sh DESCRIPTION\n.Mt punctuation@localhost.\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.message.as_ref(),
                    diagnostic
                        .primary
                        .as_ref()
                        .and_then(|span| report.document.source_position(span))
                        .map(|position| (position.line, position.column)),
                )
            })
            .collect::<Vec<_>>(),
        [(
            "mdoc.trailing-delimiter-spacing",
            "no blank before trailing delimiter: Mt punctuation@localhost.",
            Some((5, 26)),
        )]
    );
}

#[test]
fn empty_mailto_macro_generates_a_nonbreaking_space_word() {
    let name = SourceName::new("mdoc-empty-mailto.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt MAIL 1\n.Os\n.Sh DESCRIPTION\n.Mt .\n",
        ))
        .unwrap();
    let mailto = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Mt"))
        .unwrap();
    let default = mailto.children().next().unwrap();
    assert_eq!(default.text(), Some("~"));
    assert!(default.flags().generated);
}

#[test]
fn description_blocks_own_following_paragraphs_and_validate_after_closure() {
    let name = SourceName::new("mdoc-nd-paragraph.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".\\\" $OpenBSD: par.in,v 1.2 2017/07/04 14:53:25 schwarze Exp $\n.Dd $Mdocdate: July 4 2017 $\n.Dt ND-PAR 1\n.Os\n.Sh NAME\n.Nm Nd-par\n.Nd paragraph macro\nafter one-line description\n.Pp\nUsually, there shouldn't be additional text in the NAME section.\n.Sh DESCRIPTION\nThe text belongs here.\n.Nd stray\ndescription macro\n.Pp\nBack to normal state.\n",
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
                "mdoc.trailing-delimiter",
                "trailing delimiter: Nd ... Usually, there shouldn't be additional text in the NAME section.",
            ),
            (
                "mdoc.description-outside-name",
                "description line outside NAME section: Nd",
            ),
            (
                "mdoc.trailing-delimiter",
                "trailing delimiter: Nd ... Back to normal state.",
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
    assert_eq!(positions, [(10, 64), (13, 2), (16, 21)]);

    for text in [
        "Usually, there shouldn't be additional text in the NAME section.",
        "Back to normal state.",
    ] {
        let node = report
            .document
            .preorder()
            .find(|node| node.text() == Some(text))
            .unwrap();
        assert_eq!(
            node.parent().and_then(crate::NodeRef::macro_name),
            Some("Nd")
        );
    }
}

#[test]
fn empty_description_reports_when_its_body_closes() {
    let name = SourceName::new("mdoc-nd-empty.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt ND 1\n.Os\n.Sh NAME\n.Nm nd\n.Nd\n.Sh DESCRIPTION\ntext\n",
        ))
        .unwrap();

    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            "mdoc.description-missing",
            "missing description line, using \"\": Nd",
        )]
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (6, 2));
}

#[test]
fn bx_inserts_no_space_nodes_and_title_cases_its_second_argument() {
    let name = SourceName::new("mdoc-bx.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt BX 1\n.Os\n.Sh DESCRIPTION\n.Bx 4.3 tahoe\n.Bx nett.\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let bx = nodes
        .iter()
        .copied()
        .filter(|node| node.macro_name() == Some("Bx"))
        .collect::<Vec<_>>();
    assert_eq!(bx.len(), 2);
    assert_eq!(
        bx[0]
            .children()
            .map(|child| (child.macro_name(), child.text(), child.flags().generated))
            .collect::<Vec<_>>(),
        [
            (None, Some("4.3"), false),
            (Some("Ns"), None, true),
            (None, Some("BSD"), true),
            (Some("Ns"), None, true),
            (None, Some("-"), true),
            (Some("Ns"), None, true),
            (None, Some("Tahoe"), false),
        ]
    );
    assert_eq!(
        bx[1]
            .children()
            .map(|child| (child.macro_name(), child.text(), child.flags().generated))
            .collect::<Vec<_>>(),
        [
            (None, Some("nett."), false),
            (Some("Ns"), None, true),
            (None, Some("BSD"), true),
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["no blank before trailing delimiter: Bx nett."]
    );
}

#[test]
fn bx_quoted_trailing_delimiter_does_not_end_a_sentence() {
    let name = SourceName::new("mdoc-bx-quoted-delimiter.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt BX 1\n.Os\n.Sh DESCRIPTION\n.Bx 4.4 \".\"\n",
        ))
        .unwrap();
    let delimiter = report
        .document
        .preorder()
        .find(|node| node.text() == Some("."))
        .unwrap();
    assert!(delimiter.flags().delimiter_close);
    assert!(!delimiter.flags().sentence_end);
}

#[test]
fn word_keep_blocks_discard_options_and_scope_system_name_flow() {
    let name = SourceName::new("mdoc-word-keep.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt KEEP 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Ox 4.9 must remain together.\n.Ek\n",
            ))
            .unwrap();
    let keep = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bk"))
        .unwrap();
    let head = keep.children().next().unwrap();
    let body = keep.children().nth(1).unwrap();
    assert_eq!(head.kind(), NodeKind::Head);
    assert_eq!(head.children().count(), 0);
    assert_eq!(body.kind(), NodeKind::Body);
    assert!(body.children().next().is_some());
    let openbsd = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Ox"))
        .unwrap();
    assert_eq!(
        openbsd
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("OpenBSD"), Some("4.9")]
    );
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("Ek"))
    );
}

#[test]
fn synopsis_no_keeps_separate_words_and_fn_does_not_target_preceding_paragraph() {
    let name = SourceName::new("mdoc-synopsis-no.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS 1\n.Os\n.Sh SYNOPSIS\n.No two words\n.Pp\n.Fn example\n",
            ))
            .unwrap();
    let no = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("No"))
        .unwrap();
    assert_eq!(
        no.children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["two", "words"]
    );
    let paragraph = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(!paragraph.flags().deep_link_target);
}

#[test]
fn empty_bk_reports_then_disappears_from_the_public_tree() {
    let name = SourceName::new("mdoc-empty-bk.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt EMPTY-BK 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Ek\n",
        ))
        .unwrap();
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("Bk"))
    );
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::MDOC_EMPTY_BLOCK
    );
    assert_eq!(report.diagnostics[0].severity, Severity::Warning);
    assert_eq!(report.diagnostics[0].message.as_ref(), "empty block: Bk");
}

#[test]
fn standard_exit_status_expands_generated_prose_and_name_list() {
    let name = SourceName::new("mdoc-ex-standard.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt EXIT 1\n.Os\n.Sh EXIT STATUS\n.Ex -std first second\n",
        ))
        .unwrap();
    let exit_status = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ex"))
        .unwrap();
    let children = exit_status.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 6);
    assert_eq!(children[0].text(), Some("The"));
    assert_eq!(children[2].text(), Some("and"));
    assert_eq!(children[4].text(), Some("utilities exit\\~0"));
    assert_eq!(
        children[5].text(),
        Some("on success, and\\~>0 if an error occurs.")
    );
    assert!(children[0].flags().generated);
    assert!(children[5].flags().sentence_end);
    for (element, name) in [(children[1], "first"), (children[3], "second")] {
        assert_eq!(element.macro_name(), Some("Nm"));
        assert!(element.flags().generated);
        assert_eq!(
            element.children().next().and_then(crate::NodeRef::text),
            Some(name)
        );
    }
}

#[test]
fn standard_return_value_expands_function_list_and_errno_clause() {
    let name = SourceName::new("mdoc-rv-standard.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt RETURNS 3\n.Os\n.Sh RETURN VALUES\n.Rv -std first second\n",
        ))
        .unwrap();
    let return_value = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Rv"))
        .unwrap();
    let children = return_value.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 9);
    assert_eq!(children[0].text(), Some("The"));
    assert_eq!(children[2].text(), Some("and"));
    assert_eq!(children[4].text(), Some("functions return"));
    assert_eq!(children[5].text(), Some("the value\\~0 if successful;"));
    for (function, name) in [(children[1], "first"), (children[3], "second")] {
        assert_eq!(function.macro_name(), Some("Fn"));
        assert!(function.flags().generated);
        assert_eq!(
            function.children().next().and_then(crate::NodeRef::text),
            Some(name)
        );
    }
    assert_eq!(children[7].macro_name(), Some("Va"));
    assert_eq!(
        children[7].children().next().and_then(crate::NodeRef::text),
        Some("errno")
    );
    assert!(children[8].flags().sentence_end);
}

#[test]
fn missing_standard_selectors_recover_to_standard_exit_and_return_expansions() {
    let name = SourceName::new("mdoc-missing-standard-selector.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt STANDARD-RECOVERY 1\n.Os\n.Sh EXIT STATUS\n.Ex utility\n.Sh RETURN VALUES\n.Rv function\n",
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
                DiagnosticCode::MDOC_STANDARD_SELECTOR_MISSING,
                "missing -std argument, adding it: Ex",
            ),
            (
                DiagnosticCode::MDOC_SECTION_ORDER,
                "sections out of conventional order: Sh RETURN VALUES",
            ),
            (
                DiagnosticCode::MDOC_UNEXPECTED_SECTION,
                "unexpected section: Sh RETURN VALUES for 2, 3, 9 only",
            ),
            (
                DiagnosticCode::MDOC_STANDARD_SELECTOR_MISSING,
                "missing -std argument, adding it: Rv",
            ),
        ]
    );
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let exit_status = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ex"))
        .unwrap();
    assert_eq!(
        exit_status.children().next().and_then(crate::NodeRef::text),
        Some("The")
    );
    assert!(
        exit_status
            .children()
            .any(|child| child.macro_name() == Some("Nm")
                && child.children().next().and_then(crate::NodeRef::text) == Some("utility"))
    );
    let return_value = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Rv"))
        .unwrap();
    assert_eq!(
        return_value
            .children()
            .next()
            .and_then(crate::NodeRef::text),
        Some("The")
    );
    assert!(
        return_value
            .children()
            .any(|child| child.macro_name() == Some("Fn")
                && child.children().next().and_then(crate::NodeRef::text) == Some("function"))
    );
}

#[test]
fn pf_owns_exactly_one_literal_argument_before_inline_flow_resumes() {
    let name = SourceName::new("mdoc-pf-one-argument.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt PF 1\n.Os\n.Sh DESCRIPTION\n.Pf Ar Ns Ar path\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let prefix = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pf"))
        .unwrap();
    assert_eq!(
        prefix.children().next().and_then(crate::NodeRef::text),
        Some("Ar")
    );
    let no_space = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ns"))
        .unwrap();
    assert_eq!(no_space.children().count(), 0);
    let argument = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ar"))
        .unwrap();
    assert_eq!(
        argument.children().next().and_then(crate::NodeRef::text),
        Some("path")
    );
}

#[test]
fn pf_keeps_a_leading_closing_delimiter_as_its_literal_prefix() {
    let name = SourceName::new("mdoc-pf-leading-close.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 26, 2026\n.Dt PF 1\n.Os\n.Sh DESCRIPTION\n.Pf . right .\n.Em eos Pf .\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let prefixes = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pf"))
        .collect::<Vec<_>>();
    assert_eq!(prefixes.len(), 2);
    let literal = prefixes[0].children().next().unwrap();
    assert_eq!(literal.text(), Some("."));
    assert!(!literal.flags().delimiter_close);
    assert!(!literal.flags().sentence_end);
    let terminal_literal = prefixes[1].children().next().unwrap();
    assert_eq!(terminal_literal.text(), Some("."));
    assert!(terminal_literal.flags().sentence_end);
    let right = nodes
        .iter()
        .copied()
        .find(|node| node.text() == Some("right"))
        .unwrap();
    assert_eq!(
        right.parent().and_then(crate::NodeRef::macro_name),
        Some("Sh")
    );
}

#[test]
fn pf_reports_only_prefixes_without_same_line_following_content() {
    let name = SourceName::new("mdoc-pf-validation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt PF-VALIDATION 1\n.Os\n.Sh DESCRIPTION\n.Pf prefixed\n.Em eos Pf .\n.Po text Pf . Pc\n.Em end Pf\n",
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
                "mdoc.prefix-without-following",
                "nothing follows prefix: Pf prefixed",
            ),
            (
                "mdoc.prefix-without-following",
                "nothing follows prefix: Pf .",
            ),
            (
                "mdoc.prefix-without-following",
                "nothing follows prefix: Pf at eol",
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
        [Some((5, 2)), Some((6, 9)), Some((8, 9))]
    );
}

#[test]
fn fixed_argument_inline_macros_return_later_words_to_source_flow() {
    let name = SourceName::new("mdoc-in-fixed-argument.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt IN 1\n.Os\n.Sh DESCRIPTION\n.In header after\n",
        ))
        .unwrap();
    let children = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].macro_name(), Some("In"));
    assert_eq!(
        children[0].children().next().and_then(crate::NodeRef::text),
        Some("header")
    );
    assert_eq!(children[1].text(), Some("after"));
}

#[test]
fn closing_brace_is_not_a_mdoc_spacing_delimiter() {
    let name = SourceName::new("mdoc-brace-literal.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt BRACE 1\n.Os\n.Sh DESCRIPTION\n.No value Ns }\n",
        ))
        .unwrap();
    let brace = report
        .document
        .preorder()
        .find(|node| node.text() == Some("}"))
        .unwrap();
    assert!(!brace.flags().delimiter_close);
}

#[test]
fn fl_expands_each_argument_and_preserves_a_pipe_between_flags() {
    let name = SourceName::new("mdoc-fl-multiarg.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt FL 1\n.Os\n.Sh DESCRIPTION\n.Fl a b c\n.Op Fl x | y\n",
        ))
        .unwrap();
    let flags = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fl"))
        .collect::<Vec<_>>();
    assert_eq!(flags.len(), 5);
    assert_eq!(
        flags
            .iter()
            .filter_map(|flag| flag.children().next().and_then(crate::NodeRef::text))
            .collect::<Vec<_>>(),
        ["a", "b", "c", "x", "y"]
    );
    let option = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
        .unwrap();
    let body = option.children().nth(1).unwrap();
    assert!(body.children().any(|node| node.text() == Some("|")));
}

#[test]
fn fl_with_a_leading_pipe_keeps_an_empty_flag_element() {
    let name = SourceName::new("mdoc-fl-leading-pipe.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt FL 1\n.Os\n.Sh DESCRIPTION\n.Fl | and\n",
        ))
        .unwrap();
    let children = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].macro_name(), Some("Fl"));
    assert!(children[0].children().next().is_none());
    assert_eq!(children[1].text(), Some("|"));
    assert_eq!(children[2].macro_name(), Some("Fl"));
    assert_eq!(
        children[2].children().next().and_then(crate::NodeRef::text),
        Some("and")
    );
}

#[test]
fn middle_delimiter_reopens_the_same_inline_macro() {
    let name = SourceName::new("mdoc-cm-middle-delimiter.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt CM 1\n.Os\n.Sh DESCRIPTION\n.Cm one | two\n",
        ))
        .unwrap();
    let children = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].macro_name(), Some("Cm"));
    assert_eq!(
        children[0].children().next().and_then(crate::NodeRef::text),
        Some("one")
    );
    assert_eq!(children[1].text(), Some("|"));
    assert_eq!(children[2].macro_name(), Some("Cm"));
    assert_eq!(
        children[2].children().next().and_then(crate::NodeRef::text),
        Some("two")
    );
}

#[test]
fn middle_delimiter_drops_a_temporary_reopen_before_a_callable_macro() {
    let name = SourceName::new("mdoc-op-middle-delimiter.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh SYNOPSIS\n.Op Ar one \\*(Ba Fl two\n",
        ))
        .unwrap();
    let option = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
        .unwrap();
    let body = option
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    let children = body.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].macro_name(), Some("Ar"));
    assert_eq!(children[1].text(), Some(r"\fR|\fP"));
    assert_eq!(children[2].macro_name(), Some("Fl"));
    assert!(
        !children
            .iter()
            .any(|node| { node.macro_name() == Some("Ar") && node.children().next().is_none() })
    );
}

#[test]
fn closing_delimiter_reopens_the_same_inline_macro() {
    let name = SourceName::new("mdoc-ad-closing-delimiter.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt AD 1\n.Os\n.Sh DESCRIPTION\n.Ad before : after\n",
        ))
        .unwrap();
    let children = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].macro_name(), Some("Ad"));
    assert_eq!(
        children[0].children().next().and_then(crate::NodeRef::text),
        Some("before")
    );
    assert_eq!(children[1].text(), Some(":"));
    assert!(children[1].flags().delimiter_close);
    assert_eq!(children[2].macro_name(), Some("Ad"));
    assert_eq!(
        children[2].children().next().and_then(crate::NodeRef::text),
        Some("after")
    );
}

#[test]
fn ap_and_ns_have_no_owned_arguments() {
    let name = SourceName::new("mdoc-inline-no-arguments.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt INLINE 1\n.Os\n.Sh DESCRIPTION\n.No two words Ns tail\n.Xr mantdoc 1 Ap s\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let no = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("No"))
        .unwrap();
    assert_eq!(
        no.children().next().and_then(crate::NodeRef::text),
        Some("two words")
    );
    for macro_name in ["Ap", "Ns"] {
        assert!(nodes.iter().any(|node| {
            node.kind() == NodeKind::Element
                && node.macro_name() == Some(macro_name)
                && node.children().next().is_none()
        }));
    }
    assert!(nodes.iter().any(|node| node.text() == Some("tail")));
    assert!(nodes.iter().any(|node| node.text() == Some("s")));
}

#[test]
fn vt_is_a_synopsis_partial_block_with_inline_children() {
    let name = SourceName::new("mdoc-vt-literal.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt VT 1\n.Os\n.Sh SYNOPSIS\n.Vt extern Sy int Li errno\n",
        ))
        .unwrap();
    let vt = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Vt"))
        .unwrap();
    assert!(vt.flags().synopsis_pretty);
    let mut children = vt.children();
    let head = children.next().unwrap();
    let body = children.next().unwrap();
    assert_eq!(head.kind(), NodeKind::Head);
    assert_eq!(body.kind(), NodeKind::Body);
    assert!(head.flags().synopsis_pretty);
    assert!(body.flags().synopsis_pretty);
    assert_eq!(body.children().count(), 3);
    assert_eq!(
        body.children().next().and_then(crate::NodeRef::text),
        Some("extern")
    );
    assert_eq!(
        body.children().nth(1).and_then(crate::NodeRef::macro_name),
        Some("Sy")
    );
    assert_eq!(
        body.children().nth(2).and_then(crate::NodeRef::macro_name),
        Some("Li")
    );
}

#[test]
fn body_vt_discards_empty_forms_and_validates_attached_delimiters() {
    let name = SourceName::new("mdoc-vt-validation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt VT-VALIDATION 1\n.Os\n.Sh NAME\n.Nm vt-validation\n.Nd test\n.Sh DESCRIPTION\n.Vt signed int.\n.Vt unsigned long;\n.Vt\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.empty-macro", "skipping empty macro: Vt"),
            (
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Vt ... int.",
            ),
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Vt"))
            .count(),
        2
    );
    let location = report
        .document
        .source_position(report.diagnostics[1].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (8, 15));
}

#[test]
fn body_vt_retains_released_nested_macro_delimiters() {
    let name = SourceName::new("mdoc-vt-nested-delimiter.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt VT-NESTED 1\n.Os\n.Sh NAME\n.Nm vt-nested\n.Nd test\n.Sh DESCRIPTION\n.Vt unsigned Sy int ,\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty());
    let body = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Sh"))
        .nth(1)
        .unwrap();
    let children = body.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].macro_name(), Some("Vt"));
    assert_eq!(children[1].macro_name(), Some("Sy"));
    assert_eq!(children[2].text(), Some(","));
    assert!(children[2].flags().delimiter_close);
}

#[test]
fn xr_validates_fixed_arguments_and_releases_leading_delimiters() {
    let name = SourceName::new("mdoc-xr-validation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt XR-VALIDATION 1\n.Os\n.Sh NAME\n.Nm xr-validation\n.Nd test\n.Sh DESCRIPTION\n.Xr ( echo 1\n.Xr echo 1)\n.Xr echo\n.Xr echo,\n.Xr ,\n.Xr\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.empty-macro", "skipping empty macro: Xr"),
            ("mdoc.empty-macro", "skipping empty macro: Xr"),
            (
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Xr ... 1)",
            ),
            (
                "mdoc.reference-section-missing",
                "missing section argument: Xr echo",
            ),
            (
                "mdoc.reference-section-missing",
                "missing section argument: Xr echo,",
            ),
            (
                "mdoc.trailing-delimiter-spacing",
                "no blank before trailing delimiter: Xr echo,",
            ),
        ]
    );
    let xrs = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("Xr"))
        .collect::<Vec<_>>();
    assert_eq!(xrs.len(), 4);
    assert!(!xrs[0].flags().line_start);
    assert_eq!(xrs[0].children().count(), 2);
    let opening = report
        .document
        .preorder()
        .find(|node| node.text() == Some("(") && node.flags().line_start)
        .unwrap();
    assert!(opening.flags().delimiter_open);
}

#[test]
fn empty_synopsis_nm_generates_the_document_name_and_owns_following_flow() {
    let name = SourceName::new("mdoc-synopsis-name.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS-NM 1\n.Os\n.Sh NAME\n.Nm utility\n.Nd synopsis test\n.Sh SYNOPSIS\n.Nm\n.Fl f\n.Pp\n.Fl g\n",
            ))
            .unwrap();
    let nm = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Nm"))
        .unwrap();
    assert!(nm.flags().synopsis_pretty);
    let mut children = nm.children();
    let head = children.next().unwrap();
    let body = children.next().unwrap();
    let generated = head.children().next().unwrap();
    assert_eq!(generated.text(), Some("utility"));
    assert!(generated.flags().generated);
    assert!(generated.flags().synopsis_pretty);
    assert_eq!(body.kind(), NodeKind::Body);
    assert_eq!(body.children().count(), 3);
    assert!(
        body.children()
            .filter(|node| node.macro_name() == Some("Fl"))
            .all(|node| node.flags().synopsis_pretty)
    );
}

#[test]
fn authored_synopsis_nm_falls_back_to_document_name_after_an_invalid_name_entry() {
    let name = SourceName::new("mdoc-synopsis-authored-name.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS-NM 1\n.Os\n.Sh NAME\n.Nm Bx\n.Nd invalid NAME entry\n.Sh SYNOPSIS\n.Nm utility\n",
            ))
            .unwrap();

    assert_eq!(report.document.metadata().name.as_deref(), Some("utility"));
}

#[test]
fn synopsis_nm_keeps_same_line_partial_blocks_in_its_head() {
    let name = SourceName::new("mdoc-synopsis-name-partial.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SYNOPSIS-NM 1\n.Os\n.Sh SYNOPSIS\n.Nm before Bo within\n.Sh DESCRIPTION\n",
            ))
            .unwrap();
    let name = report
        .document
        .preorder()
        .find(|node| {
            node.kind() == NodeKind::Block
                && node.macro_name() == Some("Nm")
                && node.children().next().is_some_and(|head| {
                    head.children()
                        .any(|child| child.macro_name() == Some("Bo"))
                })
        })
        .unwrap();
    let mut children = name.children();
    let head = children.next().unwrap();
    let body = children.next().unwrap();

    assert!(
        head.children()
            .any(|child| child.macro_name() == Some("Bo"))
    );
    assert!(body.flags().line_start);
    assert_eq!(body.children().count(), 0);
}

#[test]
fn private_ns_register_drives_synopsis_topology_without_an_ast_request() {
    let name = SourceName::new("mdoc-ns-register.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt NS-REGISTER 1\n.Os\n.Sh NAME\n.Nm ns-register\n.Nd private synopsis state\n.Sh DESCRIPTION\n.nr nS 1\n.Nm\n.Fl a\n.nr nS 0\n.Pp\n.Fl b\n.nr nS 1\n.Nm\n.Oo Fl a\n.nr nS 0\n.Pp\n.Fl b Oc\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    assert!(nodes.iter().all(|node| node.macro_name() != Some("nr")));

    let names = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Nm"))
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    assert!(names.iter().all(|node| node.flags().synopsis_pretty));
    for name in names {
        let generated = name.children().next().unwrap().children().next().unwrap();
        assert!(generated.flags().generated);
        assert!(!generated.flags().synopsis_pretty);
    }

    let paragraphs = nodes
        .iter()
        .copied()
        .filter(|node| node.macro_name() == Some("Pp"))
        .collect::<Vec<_>>();
    assert_eq!(paragraphs.len(), 2);
    assert!(
        paragraphs
            .iter()
            .all(|paragraph| !paragraph.flags().synopsis_pretty)
    );

    let optional = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Oo"))
        .unwrap();
    assert!(optional.flags().synopsis_pretty);
    assert!(
        optional
            .children()
            .all(|child| child.flags().synopsis_pretty)
    );
}

#[test]
fn implicit_partial_blocks_follow_inline_macros_as_siblings() {
    let name = SourceName::new("mdoc-op-sibling.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Fl Op flag\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let fl = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fl"))
        .unwrap();
    assert_eq!(fl.children().count(), 0);
    let op = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
        .unwrap();
    let body = op.children().nth(1).unwrap();
    assert_eq!(body.kind(), NodeKind::Body);
    assert_eq!(
        body.children().next().and_then(crate::NodeRef::text),
        Some("flag")
    );
}

#[test]
fn callable_partial_blocks_end_an_inline_scope_and_parse_nested_mailto() {
    let name = SourceName::new("mdoc-an-partial-blocks.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AUTHORS 1\n.Os\n.Sh DESCRIPTION\n.An Name Ao Mt addr Ac An Name Aq Mt addr\n",
            ))
            .unwrap();
    let authors = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("An"))
        .collect::<Vec<_>>();
    assert_eq!(authors.len(), 2);
    assert!(authors.iter().all(|author| {
        author.children().count() == 1
            && author.children().next().and_then(crate::NodeRef::text) == Some("Name")
    }));
    for enclosure in ["Ao", "Aq"] {
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some(enclosure))
            .unwrap();
        let body = block
            .children()
            .find(|node| node.kind() == NodeKind::Body)
            .unwrap();
        let mailto = body.children().next().unwrap();
        assert_eq!(mailto.macro_name(), Some("Mt"));
        assert_eq!(
            mailto.children().next().and_then(crate::NodeRef::text),
            Some("addr")
        );
    }
}

#[test]
fn implicit_partial_blocks_recurse_inside_parsed_arguments() {
    let name = SourceName::new("mdoc-op-nested.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Op outer Op inner\n",
        ))
        .unwrap();
    let mut options = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"));
    let outer = options.next().unwrap();
    let inner = options.next().unwrap();
    assert_eq!(outer.children().nth(1).unwrap().children().count(), 2);
    assert_eq!(
        inner
            .children()
            .nth(1)
            .unwrap()
            .children()
            .next()
            .and_then(crate::NodeRef::text),
        Some("inner")
    );
}

#[test]
fn implicit_partial_blocks_keep_a_leading_open_delimiter_outside_the_body() {
    let name = SourceName::new("mdoc-dq-open.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt DQ 1\n.Os\n.Sh DESCRIPTION\n.Dq \"(\" user@host)\n",
        ))
        .unwrap();
    let dq = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Dq"))
        .unwrap();
    let mut children = dq.children();
    assert_eq!(children.next().unwrap().kind(), NodeKind::Head);
    let opening = children.next().unwrap();
    assert_eq!(opening.text(), Some("("));
    assert!(opening.flags().delimiter_open);
    let body = children.next().unwrap();
    assert_eq!(body.kind(), NodeKind::Body);
    assert_eq!(
        body.children().next().and_then(crate::NodeRef::text),
        Some("user@host)")
    );
}

#[test]
fn implicit_partial_blocks_publish_unescaped_closing_punctuation_as_a_tail() {
    let name = SourceName::new("mdoc-pq-tail.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt PQ 1\n.Os\n.Sh DESCRIPTION\n.Pq quite lonely .\n.Pq \\&.\n",
        ))
        .unwrap();
    let parens = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
        .collect::<Vec<_>>();
    let first = parens[0].children().collect::<Vec<_>>();
    assert_eq!(first[1].kind(), NodeKind::Body);
    assert_eq!(
        first[1].children().next().and_then(crate::NodeRef::text),
        Some("quite lonely")
    );
    assert_eq!(first[2].text(), Some("."));
    assert!(first[2].flags().delimiter_close);
    assert!(first[2].flags().sentence_end);

    let second_body = parens[1].children().nth(1).unwrap();
    assert_eq!(
        second_body.children().next().and_then(crate::NodeRef::text),
        Some("\\&.")
    );
    assert_eq!(parens[1].children().count(), 2);
}

#[test]
fn implicit_partial_blocks_preserve_internal_and_repeated_delimiter_boundaries() {
    let name = SourceName::new("mdoc-op-punctuation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Op | z\n.Op a ( z\n.Op . z\n.Op ( (\n.Op . .\n.Op a (\n",
            ))
            .unwrap();
    let options = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Op"))
        .collect::<Vec<_>>();

    for (option, expected) in options.iter().take(3).zip([
        ["|", "z"].as_slice(),
        ["a", "(", "z"].as_slice(),
        [".", "z"].as_slice(),
    ]) {
        let body = option
            .children()
            .find(|child| child.kind() == NodeKind::Body)
            .unwrap();
        assert_eq!(
            body.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let middle_open = options[1]
        .children()
        .find(|child| child.kind() == NodeKind::Body)
        .unwrap()
        .children()
        .nth(1)
        .unwrap();
    assert!(middle_open.flags().delimiter_open);
    let leading_close = options[2]
        .children()
        .find(|child| child.kind() == NodeKind::Body)
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert!(!leading_close.flags().delimiter_close);

    let repeated_open = options[3].children().collect::<Vec<_>>();
    assert_eq!(repeated_open[0].kind(), NodeKind::Head);
    assert_eq!(repeated_open[1].text(), Some("("));
    assert_eq!(repeated_open[2].text(), Some("("));
    assert!(repeated_open[1].flags().delimiter_open);
    assert!(repeated_open[2].flags().delimiter_open);
    assert_eq!(repeated_open[3].kind(), NodeKind::Body);

    let repeated_close = options[4].children().collect::<Vec<_>>();
    assert_eq!(repeated_close[1].kind(), NodeKind::Body);
    for tail in &repeated_close[2..] {
        assert_eq!(tail.text(), Some("."));
        assert!(tail.flags().delimiter_close);
        assert!(tail.flags().sentence_end);
    }

    let terminal_open = options[5]
        .children()
        .find(|child| child.kind() == NodeKind::Body)
        .unwrap()
        .children()
        .nth(1)
        .unwrap();
    assert_eq!(terminal_open.text(), Some("("));
    assert!(!terminal_open.flags().delimiter_open);
}

#[test]
fn column_cells_keep_cross_line_explicit_partial_scopes() {
    let name = SourceName::new("mdoc-column-partial.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column one two\n.It it Aq aq Ta ta Bo bo bc\n.Bc Pq pq\n.El\n",
            ))
            .unwrap();
    let item = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .unwrap();
    let second_cell = item.children().nth(2).unwrap();
    let names = second_cell
        .children()
        .filter_map(crate::NodeRef::macro_name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["Bo", "Pq"]);
    assert!(
        second_cell
            .children()
            .filter(|node| node.macro_name().is_some())
            .all(|node| node.kind() == NodeKind::Block)
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn column_lists_validate_cells_and_preserve_tab_phrase_semantics() {
    let name = SourceName::new("mdoc-column-validation.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column \"a\" \"b\"\n.It\n.It \"a\"\n.It \"a\" Ta \"b\"\n.It \"a\" Ta \"b\" Ta \"c\"\n.It \"a\" Ta \"b\" Ta \"c\" Ta \"d\"\n.It \"a\" Ta \"b\" Ta \"c\" Ta \"d\" Ta \"e\"\n.It\n.El\n.Bl -column \"a\" \"b\" \"cc\"\n.It \"a\tb\"\tcc\n.El\n.Bl -column \"a\" \"b\"\n.It a \tb\n.El\n.Bl -column \"aa\" -width 6n -compact \"bb\" \"cc\"\n.It aa Ta bb Ta cc Ta dd\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "skipping empty macro: It",
            "wrong number of cells: 2 columns, 1 cells",
            "wrong number of cells: 2 columns, 4 cells",
            "wrong number of cells: 2 columns, 5 cells",
            "skipping empty macro: It",
            "skipping -width argument: Bl -column",
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
    assert_eq!(
        positions,
        [(6, 2), (7, 2), (10, 2), (11, 2), (12, 2), (20, 18)]
    );
}

#[test]
fn column_cells_accept_inline_and_physical_ta_recovery() {
    let name = SourceName::new("mdoc-column-ta-recovery.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column \"first column\" \"second column\"\n.It\ntext\n.No macro Ta after tab\n.El\n.Bl -column aa bb\n.It aa\n.Ta bb\n.El\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "missing argument, using next line: Bl -column It",
            "first macro on line: Ta",
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
    assert_eq!(positions, [(6, 2), (12, 2)]);

    let items = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    let first_cells = items[0]
        .children()
        .filter(|node| node.kind() == NodeKind::Body)
        .collect::<Vec<_>>();
    assert_eq!(first_cells.len(), 2);
    let no = first_cells[0]
        .children()
        .find(|node| node.macro_name() == Some("No"))
        .unwrap();
    assert_eq!(
        no.children().map(crate::NodeRef::text).collect::<Vec<_>>(),
        [Some("macro")]
    );
    assert_eq!(
        first_cells[1]
            .children()
            .map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        [Some("after tab")]
    );
    let second_cells = items[1]
        .children()
        .filter(|node| node.kind() == NodeKind::Body)
        .collect::<Vec<_>>();
    assert_eq!(second_cells.len(), 2);
    assert_eq!(
        second_cells
            .iter()
            .map(|cell| cell.children().next().and_then(crate::NodeRef::text))
            .collect::<Vec<_>>(),
        [Some("aa"), Some("bb")]
    );
    assert!(second_cells[1].flags().line_start);
}

#[test]
fn column_cells_expand_each_system_name_macro() {
    let name = SourceName::new("mdoc-column-system-name.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt COLUMN 1\n.Os\n.Sh DESCRIPTION\n.Bl -column \"aa\" \"OpenBSD OpenBSD OpenBSD\" \"tail\"\n.It aa Ta Ox Ox Ox Ta tab-tab\n.It aa\t Ox Ox Ox\tta/bl-ta\n.It aa\tbb\t\ntab at eol\n.El\n",
            ))
            .unwrap();
    let systems = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ox"))
        .collect::<Vec<_>>();
    let first_row = systems
        .iter()
        .copied()
        .filter(|node| {
            node.location()
                .and_then(|span| report.document.source_position(span))
                .is_some_and(|position| position.line == 6)
        })
        .collect::<Vec<_>>();
    assert_eq!(first_row.len(), 3);
    assert!(first_row.iter().all(|node| {
        node.children()
            .next()
            .is_some_and(|child| child.text() == Some("OpenBSD") && child.flags().generated)
    }));
    assert_eq!(
        systems
            .iter()
            .filter(|node| {
                node.location()
                    .and_then(|span| report.document.source_position(span))
                    .is_some_and(|position| position.line == 7)
            })
            .count(),
        2
    );
    let retained = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(|node| {
            let position = node
                .location()
                .and_then(|span| report.document.source_position(span))?;
            Some((node.text(), position.line, position.column))
        })
        .collect::<Vec<_>>();
    assert!(retained.contains(&(Some(""), 7, 8)));
    assert!(retained.contains(&(Some(r"\&"), 8, 2)));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn sm_spacing_controls_partial_block_text_coalescing() {
    let name = SourceName::new("mdoc-sm-spacing.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SM 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.Pq now off\n.Sm\n.Pq now on\n.Sm off\n.No macro2 macro3\n.Sm\n.No macro4 macro5\n",
            ))
            .unwrap();
    let parens = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
        .collect::<Vec<_>>();
    assert_eq!(parens.len(), 2);

    let disabled = parens[0].children().nth(1).unwrap();
    assert_eq!(
        disabled
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["now", "off"]
    );

    let enabled = parens[1].children().nth(1).unwrap();
    assert_eq!(
        enabled
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["now on"]
    );

    let no_space = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("No"))
        .collect::<Vec<_>>();
    assert_eq!(no_space.len(), 2);
    assert_eq!(
        no_space[0]
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["macro2", "macro3"]
    );
    assert_eq!(
        no_space[1]
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["macro4 macro5"]
    );
}

#[test]
fn invalid_sm_boolean_argument_warns_without_changing_spacing_state() {
    let name = SourceName::new("mdoc-sm-invalid.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SM 1\n.Os\n.Sh NAME\n.Nm sm\n.Nd spacing control\n.Sh DESCRIPTION\n.Sm off\n.Sm bad\n.Pq still off\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.message.as_ref(),
                    diagnostic
                        .primary
                        .as_ref()
                        .and_then(|span| report.document.source_position(span)),
                )
            })
            .collect::<Vec<_>>(),
        [(
            "mdoc.boolean-argument",
            "invalid Boolean argument: Sm bad",
            Some(crate::SourcePosition { line: 9, column: 5 }),
        )]
    );
    let parens = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Pq"))
        .unwrap();
    let body = parens.children().nth(1).unwrap();
    assert_eq!(
        body.children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["still", "off"]
    );
}

#[test]
fn sm_off_disables_em_and_sy_word_joining() {
    let name = SourceName::new("mdoc-sm-join.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SM 1\n.Os\n.Sh NAME\n.Nm sm\n.Nd spacing control\n.Sh DESCRIPTION\n.Em enabled words\n.Sy symbolic words\n.Sm off\n.Em disabled words\n.Sy literal words\n",
            ))
            .unwrap();
    let contents = report
        .document
        .preorder()
        .filter(|node| {
            node.kind() == NodeKind::Element && matches!(node.macro_name(), Some("Em" | "Sy"))
        })
        .map(|node| {
            (
                node.macro_name().unwrap(),
                node.children()
                    .filter_map(crate::NodeRef::text)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        [
            ("Em", vec!["enabled words"]),
            ("Sy", vec!["symbolic words"]),
            ("Em", vec!["disabled", "words"]),
            ("Sy", vec!["literal", "words"]),
        ]
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn st_expands_known_selectors_and_defers_unknown_selector_diagnostics() {
    let name = SourceName::new("mdoc-st.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ST 1\n.Os\n.Sh NAME\n.Nm st\n.Nd standard selector\n.Sh STANDARDS\n.St -p1003.1-2004\n.St -murks\n.St\n",
            ))
            .unwrap();
    let standards = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("St"))
        .collect::<Vec<_>>();
    assert_eq!(standards.len(), 1);
    let children = standards[0].children().collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert_eq!(
        children[0].text(),
        Some("IEEE Std 1003.1-2004 (\\(lqPOSIX.1\\(rq)")
    );
    assert!(children[0].flags().generated);
    assert_eq!(children[1].text(), Some("-p1003.1-2004"));
    assert!(children[1].flags().no_print);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.message.as_ref(),
                    diagnostic
                        .primary
                        .as_ref()
                        .and_then(|span| report.document.source_position(span)),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                "mdoc.empty-macro",
                "skipping empty macro: St",
                Some(crate::SourcePosition {
                    line: 10,
                    column: 2,
                }),
            ),
            (
                "mdoc.unknown-standard",
                "unknown standard specifier: St -murks",
                Some(crate::SourcePosition { line: 9, column: 5 }),
            ),
        ]
    );
}

#[test]
fn empty_ar_synthesizes_generated_file_ellipsis_words() {
    let name = SourceName::new("mdoc-ar-default.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt AR 1\n.Os\n.Sh SYNOPSIS\n.Ar\n",
        ))
        .unwrap();
    let argument = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Ar"))
        .unwrap();
    let words = argument.children().collect::<Vec<_>>();
    assert_eq!(
        words
            .iter()
            .filter_map(|word| word.text())
            .collect::<Vec<_>>(),
        ["file", "..."]
    );
    assert!(
        words
            .iter()
            .all(|word| word.flags().generated && word.flags().synopsis_pretty)
    );
}

#[test]
fn explicit_partial_blocks_consume_same_line_closers_and_restore_tail_flow() {
    let name = SourceName::new("mdoc-do-close.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DO 1\n.Os\n.Sh DESCRIPTION\n.Do \"(\" full) Dc one Sy bold .\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let block = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Do"))
        .unwrap();
    let mut children = block.children();
    let opening = children.next().unwrap();
    assert_eq!(opening.text(), Some("("));
    assert!(opening.flags().delimiter_open);
    assert_eq!(children.next().unwrap().kind(), NodeKind::Head);
    let body = children.next().unwrap();
    assert_eq!(body.kind(), NodeKind::Body);
    assert_eq!(
        body.children().next().and_then(crate::NodeRef::text),
        Some("full)")
    );
    assert!(!nodes.iter().any(|node| node.macro_name() == Some("Dc")));
    assert!(nodes.iter().any(|node| node.text() == Some("one")));
    assert!(nodes.iter().any(|node| node.macro_name() == Some("Sy")));
}

#[test]
fn explicit_partial_scopes_pair_nested_inline_and_cross_line_closers() {
    let name = SourceName::new("mdoc-oo-nested-lines.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt OO 1\n.Os\n.Sh SYNOPSIS\n.Bk -words\n.Oo\n.Oo No a Oc Oo No b Oc Oc Pq tail\n.Ek\n",
            ))
            .unwrap();
    let keep = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bk"))
        .unwrap();
    let keep_body = keep
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    let keep_children = keep_body.children().collect::<Vec<_>>();
    assert_eq!(keep_children.len(), 2);
    assert_eq!(keep_children[0].macro_name(), Some("Oo"));
    assert_eq!(keep_children[1].macro_name(), Some("Pq"));
    let outer_body = keep_children[0]
        .children()
        .find(|node| node.kind() == NodeKind::Body)
        .unwrap();
    assert_eq!(
        outer_body
            .children()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Oo"))
            .count(),
        2
    );
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.text() == Some("Oc"))
    );
}

#[test]
fn bro_uses_the_brc_partial_close_pair() {
    let name = SourceName::new("mdoc-bro-close.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt BRO 1\n.Os\n.Sh DESCRIPTION\n.Bro \"(\" full) Brc one\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let block = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Bro"))
        .unwrap();
    let mut children = block.children();
    let opening = children.next().unwrap();
    assert_eq!(opening.text(), Some("("));
    assert!(opening.flags().delimiter_open);
    assert_eq!(children.next().unwrap().kind(), NodeKind::Head);
    assert_eq!(
        children
            .next()
            .unwrap()
            .children()
            .next()
            .and_then(crate::NodeRef::text),
        Some("full)")
    );
    assert!(!nodes.iter().any(|node| node.macro_name() == Some("Brc")));
    assert!(nodes.iter().any(|node| node.text() == Some("one")));
}

#[test]
fn eo_scope_uses_a_head_body_and_tail_across_physical_lines() {
    let name = SourceName::new("mdoc-eo-tail.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo open\nbody\n.Ec close\nnext\n",
            ))
            .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
        .unwrap();
    let mut children = block.children();
    let head = children.next().unwrap();
    let body = children.next().unwrap();
    let tail = children.next().unwrap();
    assert_eq!(head.kind(), NodeKind::Head);
    assert_eq!(
        head.children().next().and_then(crate::NodeRef::text),
        Some("open")
    );
    assert_eq!(body.kind(), NodeKind::Body);
    assert_eq!(
        body.children().next().and_then(crate::NodeRef::text),
        Some("body")
    );
    assert_eq!(tail.kind(), NodeKind::Tail);
    assert_eq!(tail.macro_name(), Some("Eo"));
    assert_eq!(
        tail.children().next().and_then(crate::NodeRef::text),
        Some("close")
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("next"))
    );
}

#[test]
fn inline_eo_after_no_and_ns_opens_a_scope() {
    let name = SourceName::new("mdoc-inline-eo.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.No prefix Ns Eo\n.Ec close\n",
        ))
        .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
        .unwrap();
    assert_eq!(block.children().count(), 3);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn ec_tail_stops_before_a_following_callable_macro() {
    let name = SourceName::new("mdoc-ec-tail-inline.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EC 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\nbody\n.Ec >> \"Sy\" bold\n",
            ))
            .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
        .unwrap();
    let tail = block.children().nth(2).unwrap();
    assert_eq!(tail.kind(), NodeKind::Tail);
    assert_eq!(tail.children().count(), 1);
    assert_eq!(
        tail.children().next().and_then(crate::NodeRef::text),
        Some(">>")
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
    );
}

#[test]
fn inline_ec_closes_eo_and_a_stray_ec_becomes_br() {
    let name = SourceName::new("mdoc-inline-ec.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EC 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\n.No prefix Ns Ec\n.Ec >>\n",
            ))
            .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
        .unwrap();
    assert_eq!(block.children().count(), 3);
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("br"))
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some(">>"))
    );
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code.as_str(), "mdoc.unmatched-close");
}

#[test]
fn inline_fc_closes_a_function_scope() {
    let name = SourceName::new("mdoc-inline-fc.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt FC 1\n.Os\n.Sh SYNOPSIS\n.Fo call\n.Nm name Fc tail\n",
        ))
        .unwrap();
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "mdoc.unclosed-block")
    );
}

#[test]
fn unclosed_eo_retains_only_its_head_and_body_prefix() {
    let name = SourceName::new("mdoc-eo-unclosed.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo open\n",
        ))
        .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Eo"))
        .unwrap();
    assert_eq!(block.children().count(), 2);
    assert_eq!(report.diagnostics[0].code.as_str(), "mdoc.unclosed-block");
}

#[test]
fn fo_parts_inherit_synopsis_presentation() {
    let name = SourceName::new("mdoc-fo-synopsis.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh SYNOPSIS\n.Fo call\n.Fa void\n.Fc\n",
        ))
        .unwrap();
    let block = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Fo"))
        .unwrap();
    assert!(block.flags().synopsis_pretty);
    assert!(block.children().all(|child| child.flags().synopsis_pretty));
}

#[test]
fn fo_head_is_a_non_synopsis_target_and_consumes_a_pending_tg() {
    let name = SourceName::new("mdoc-fo-tag.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Tg manual\n.Fo call\n.Fa void\n.Fc\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let head = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
        .unwrap();
    assert!(head.flags().deep_link_target);
    assert!(head.flags().permalink);
    assert_eq!(head.tag(), Some("manual"));
    let tg = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(tg.flags().no_print);
}

#[test]
fn paragraph_precedes_fo_function_target_and_fc_inline_macro_keeps_line_context() {
    let name = SourceName::new("mdoc-fo-paragraph-target.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Fo prefix\\\\fIname\\\\fPsuffix\n.Fa void\n.Fc \"Sy\" bold\n",
            ))
            .unwrap();
    let paragraph = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(paragraph.flags().deep_link_target);
    assert_eq!(paragraph.tag(), Some("prefix"));
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
        .unwrap();
    assert!(!head.flags().deep_link_target);
    assert!(head.flags().permalink);
    assert_eq!(head.tag(), Some("prefix"));
    let symbolic = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
        .unwrap();
    assert!(!symbolic.flags().line_start);
}

#[test]
fn roff_break_keeps_a_preceding_paragraph_eligible_for_fo_targets() {
    let name = SourceName::new("mdoc-fo-break-target.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Pp\nfunction declaration:\n.br\n.Fo call\n.Fa void\n.Fc\n",
            ))
            .unwrap();
    let paragraph = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(paragraph.flags().deep_link_target);
    assert_eq!(paragraph.tag(), Some("call"));
    let head = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
        .unwrap();
    assert!(head.flags().permalink);
    assert_eq!(head.tag(), None);
}

#[test]
fn fn_uses_an_eligible_paragraph_as_its_target() {
    let name = SourceName::new("mdoc-fn-tag.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Tg manual\n.Fn call void\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let paragraph = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(paragraph.flags().deep_link_target);
    assert_eq!(paragraph.tag(), Some("manual"));
    let function = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .unwrap();
    assert!(!function.flags().deep_link_target);
    assert!(function.flags().permalink);
    assert_eq!(function.tag(), Some("manual"));
}

#[test]
fn later_functions_in_one_paragraph_do_not_gain_a_second_automatic_target() {
    let name = SourceName::new("mdoc-fn-one-target.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Fn first\nand\n.Fn second\n",
            ))
            .unwrap();
    let functions = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 2);
    assert!(!functions[0].flags().deep_link_target);
    assert!(functions[0].flags().permalink);
    assert!(!functions[1].flags().deep_link_target);
    assert!(!functions[1].flags().permalink);
}

#[test]
fn standalone_fn_is_a_target_only_outside_synopsis() {
    let name = SourceName::new("mdoc-fn-synopsis.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh SYNOPSIS\n.Fn synopsis void\n.Sh DESCRIPTION\n.Fn detail void\n",
            ))
            .unwrap();
    let functions = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 2);
    assert!(!functions[0].flags().deep_link_target);
    assert!(functions[1].flags().deep_link_target);
    assert!(functions[1].flags().permalink);
}

#[test]
fn standalone_function_targets_use_the_first_phrase_word_and_fc_releases_punctuation() {
    let name = SourceName::new("mdoc-fn-fc-eos.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Fn \"double sin\" \"double x\" .\n.Fo cos\n.Fa double x\n.Fc .\n",
            ))
            .unwrap();
    let function = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .unwrap();
    assert!(function.flags().deep_link_target);
    assert_eq!(function.tag(), Some("double"));
    let periods = report
        .document
        .preorder()
        .filter(|node| node.text() == Some("."))
        .collect::<Vec<_>>();
    assert_eq!(periods.len(), 2);
    assert!(periods.iter().all(|period| period.flags().sentence_end));
    assert!(periods[1].flags().line_start);
    assert!(periods[1].flags().delimiter_close);
}

#[test]
fn function_type_declarations_restart_standalone_function_targeting() {
    let name = SourceName::new("mdoc-fn-type-targets.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Ft int\n.Fn first void\n.Ft int\n.Fn second void\n",
            ))
            .unwrap();
    let functions = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 2);
    assert!(
        functions
            .iter()
            .all(|function| function.flags().deep_link_target && function.flags().permalink)
    );
}

#[test]
fn tg_inside_fo_is_a_visible_destination() {
    let name = SourceName::new("mdoc-fo-tg.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.Tg argument\n.Fc\n",
        ))
        .unwrap();
    let tg = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(tg.flags().deep_link_target);
    assert!(!tg.flags().no_print);
    assert_eq!(tg.tag(), None);
}

#[test]
fn fo_transparent_targets_are_limited_only_in_synopsis() {
    let name = SourceName::new("mdoc-fo-tg-context.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh SYNOPSIS\n.Fo synopsis\n.Tg first\n.Tg second\n.Fc\n.Sh DESCRIPTION\n.Fo detail\n.Tg third\n.Tg fourth\n.Fc\n",
            ))
            .unwrap();
    let targets = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
        .map(|node| (node.flags().deep_link_target, node.flags().no_print))
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        [(true, false), (false, true), (true, false), (true, false)]
    );
}

#[test]
fn pending_tg_can_name_the_following_section_head() {
    let name = SourceName::new("mdoc-tg-section.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt TG 1\n.Os\n.Sh NAME\n.Tg section-tag\n.Sh DESCRIPTION\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let head = nodes
        .iter()
        .copied()
        .find(|node| {
            node.kind() == NodeKind::Head
                && node.macro_name() == Some("Sh")
                && node.children().next().and_then(crate::NodeRef::text) == Some("DESCRIPTION")
        })
        .unwrap();
    assert_eq!(head.tag(), Some("section-tag"));
    let tg = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(tg.flags().no_print);
}

#[test]
fn pending_tg_can_name_the_following_subsection_head() {
    let name = SourceName::new("mdoc-tg-subsection.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TG 1\n.Os\n.Sh DESCRIPTION\n.Tg subsection-tag\n.Ss DETAILS\n",
            ))
            .unwrap();
    let subsection = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Ss"))
        .unwrap();
    assert!(subsection.flags().deep_link_target);
    assert!(subsection.flags().permalink);
    assert_eq!(subsection.tag(), Some("subsection-tag"));
    let tg = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Tg"))
        .unwrap();
    assert!(tg.flags().no_print);
}

#[test]
fn normalizes_deterministic_mdocdate_without_consulting_host_time() {
    let name = SourceName::new("mdoc-date.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd $Mdocdate: Jul 6 2017 $\n.Dt DATE 1\n.Os\n.Sh NAME\ndate\n",
        ))
        .unwrap();
    assert_eq!(
        report.document.metadata().date.as_deref(),
        Some("July 6, 2017")
    );
    let date = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("Dd"))
        .unwrap();
    assert_eq!(date.children().count(), 1);
    assert_eq!(
        date.children().next().and_then(crate::NodeRef::text),
        Some("$Mdocdate: Jul 6 2017 $")
    );

    let literal = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd $Mdocdate$\n.Dt DATE 1\n.Os\n.Sh NAME\ndate\n",
        ))
        .unwrap();
    assert_eq!(
        literal.document.metadata().date.as_deref(),
        Some("$Mdocdate$")
    );
}

#[test]
fn assigns_and_suppresses_mdoc_section_destination_tags() {
    let name = SourceName::new("mdoc-tags.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAGS 1\n.Sh NAME\nname\n.Sh \"SEE ALSO\"\nfirst\n.Ss \"SEE ALSO\"\nsecond\n",
            ))
            .unwrap();
    let heads = report
        .document
        .preorder()
        .filter(|node| matches!(node.macro_name(), Some("Sh" | "Ss")))
        .filter(|node| node.kind() == NodeKind::Head)
        .collect::<Vec<_>>();
    assert_eq!(heads.len(), 3);
    assert!(heads[0].flags().deep_link_target);
    assert_eq!(heads[0].tag(), None);
    assert!(
        heads[1..]
            .iter()
            .all(|head| !head.flags().deep_link_target && head.tag().is_none())
    );
}

#[test]
fn section_targets_preserve_discretionary_hyphen_and_deroff_heading_spellings() {
    let name = SourceName::new("mdoc-section-tag-spelling.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt SECTION-TAGS 1\n.Os\n.Sh DESCRIPTION\n.Ss Sub-section\n.Sh \\&\\t WEIRD SECTION\\t \n",
            ))
            .unwrap();
    let heads = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Head)
        .filter(|node| matches!(node.macro_name(), Some("Sh" | "Ss")))
        .collect::<Vec<_>>();

    assert_eq!(heads.len(), 3);
    assert_eq!(heads[1].tag(), Some("Sub-section"));
    assert_eq!(heads[2].tag(), Some("WEIRD_SECTION"));
}

#[test]
fn assigns_unique_emphasis_fallback_targets_like_libmandoc() {
    let name = SourceName::new("mdoc-emphasis-tags.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Em unique\\fBbold\\fP\n.Em duplicate\n.Em duplicate\n",
            ))
            .unwrap();
    let elements = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Em"))
        .collect::<Vec<_>>();
    assert_eq!(elements.len(), 3);
    assert!(elements[0].flags().deep_link_target);
    assert_eq!(elements[0].tag(), Some("unique"));
    assert!(
        elements[1..]
            .iter()
            .all(|element| !element.flags().deep_link_target && element.tag().is_none())
    );
}

#[test]
fn emphasis_fallback_moves_its_destination_to_a_preceding_paragraph() {
    let name = SourceName::new("mdoc-emphasis-paragraph-tag.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Pp\ncontext\n.Sy target\n",
            ))
            .unwrap();
    let paragraph = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(paragraph.flags().deep_link_target);
    assert!(!paragraph.flags().permalink);
    assert_eq!(paragraph.tag(), Some("target"));
    let emphasis = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
        .unwrap();
    assert!(!emphasis.flags().deep_link_target);
    assert!(emphasis.flags().permalink);
}

#[test]
fn meaningful_emphasis_fallback_replaces_a_moved_punctuation_target() {
    let name = SourceName::new("mdoc-emphasis-punctuation-target.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Em \". b Nm\"\n.Sy bold\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let paragraph = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(paragraph.flags().deep_link_target);
    assert_eq!(paragraph.tag(), Some("bold"));
    let emphasis = nodes
        .iter()
        .copied()
        .find(|node| {
            node.kind() == NodeKind::Element
                && node.macro_name() == Some("Em")
                && node.tag() == Some(".")
        })
        .unwrap();
    assert!(emphasis.flags().deep_link_target);
    assert!(emphasis.flags().permalink);
    let symbolic = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
        .unwrap();
    assert!(!symbolic.flags().deep_link_target);
    assert!(symbolic.flags().permalink);
}

#[test]
fn duplicate_emphasis_fallback_does_not_leave_a_paragraph_target() {
    let name = SourceName::new("mdoc-emphasis-duplicate-paragraph.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPHASIS 1\n.Os\n.Sh DESCRIPTION\n.Pp\ncontext\n.Sy duplicate\n.Sy duplicate\n",
            ))
            .unwrap();
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("Pp"))
    );
    assert!(
        report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Sy"))
            .all(|node| !node.flags().deep_link_target && !node.flags().permalink)
    );
}

#[test]
fn resolves_mdoc_author_and_stateful_enclosure_semantics() {
    let name = SourceName::new("mdoc-enclosure.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ENCLOSURE 1\n.Os\n.Sh AUTHORS\n.An -nosplit Alice Example\n.Es << >>\n.En enclosed\n.An -split Bob Example\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let authors = nodes
        .iter()
        .filter(|node| node.macro_name() == Some("An"))
        .collect::<Vec<_>>();
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0].author_mode(), Some(AuthorMode::NoSplit));
    assert_eq!(authors[1].author_mode(), Some(AuthorMode::Split));
    let enclosure = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("En"))
        .and_then(crate::NodeRef::enclosure)
        .unwrap();
    assert_eq!(enclosure.opening.as_ref(), "<<");
    assert_eq!(enclosure.closing.as_deref(), Some(">>"));
    let enclosure_block = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("En"))
        .unwrap();
    assert_eq!(enclosure_block.children().count(), 2);
    assert_eq!(
        enclosure_block
            .children()
            .nth(1)
            .and_then(|body| body.children().next())
            .and_then(crate::NodeRef::text),
        Some("enclosed")
    );
    assert!(
        nodes
            .iter()
            .any(|node| node.macro_name() == Some("Es") && !node.flags().no_print)
    );
}

#[test]
fn obsolete_enclosure_macros_emit_typed_warnings() {
    let name = SourceName::new("mdoc-obsolete-enclosure.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt OBSOLETE 1\n.Os\n.Sh DESCRIPTION\n.Es << >>\n.En words\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.severity))
            .collect::<Vec<_>>(),
        [
            ("mdoc.obsolete", crate::Severity::Warning),
            ("mdoc.obsolete", crate::Severity::Warning),
        ]
    );
}

#[test]
fn obsolete_debug_macros_keep_their_end_of_line_arguments() {
    let name = SourceName::new("mdoc-obsolete-debug.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt OBSOLETE 1\n.Os\n.Sh DESCRIPTION\n.Db\n.Db on\n.Db foo bar\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.obsolete", "obsolete macro: Db"),
            ("mdoc.obsolete", "obsolete macro: Db"),
            ("mdoc.obsolete", "obsolete macro: Db"),
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.macro_name() == Some("Db"))
            .flat_map(crate::NodeRef::children)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["on", "foo", "bar"]
    );
}

#[test]
fn duplicate_date_prologues_keep_the_last_metadata_value() {
    let name = SourceName::new("mdoc-duplicate-date.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 1, 2014\n.Dt DUPLICATE 1\n.Os\n.Dd August 3, 2014\n.Sh NAME\n.Nm duplicate-date\n.Nd date test\n.Sh DESCRIPTION\ninitial text\n.Dd August 5, 2014\nfinal text\n",
            ))
            .unwrap();
    assert_eq!(
        report.document.metadata().date.as_deref(),
        Some("August 5, 2014")
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.duplicate-prologue", "duplicate prologue macro: Dd"),
            ("mdoc.duplicate-prologue", "duplicate prologue macro: Dd"),
        ]
    );
}

#[test]
fn operating_system_prologues_keep_the_first_legacy_validation_flavour() {
    let name = SourceName::new("mdoc-operating-system-prologues.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".\\\" $OpenBSD: os.in,v 1.0 2026/08/26 00:00:00 maintainer Exp $\n.Dd $Mdocdate: August 26 2026 $\n.Os NetBSD\n.Dt OS 1\n.Os FreeBSD\n.Sh DESCRIPTION\n.Os OpenBSD\n",
            ))
            .unwrap();

    assert_eq!(report.document.metadata().os.as_deref(), Some("OpenBSD"));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.severity))
            .collect::<Vec<_>>(),
        [
            ("mdoc.operating-system-explicit", Severity::Style),
            ("mdoc.mdocdate-found", Severity::Style),
            ("mdoc.prologue-order", Severity::Warning),
            ("mdoc.duplicate-prologue", Severity::Error),
            ("mdoc.operating-system-explicit", Severity::Style),
            ("mdoc.mdocdate-found", Severity::Style),
            ("mdoc.duplicate-prologue", Severity::Error),
            ("mdoc.operating-system-explicit", Severity::Style),
            ("mdoc.rcs-id-missing", Severity::Style),
        ]
    );
}

#[test]
fn operating_system_validation_distinguishes_late_arbitrary_and_missing_prologues() {
    let late_name = SourceName::new("mdoc-late-os.1").unwrap();
    let late = Parser::default()
        .parse(Source::new(
            &late_name,
            b".Dd August 26, 2026\n.Dt LATE-OS 1\n.Sh DESCRIPTION\ntext\n.Os\n",
        ))
        .unwrap();
    assert_eq!(
        late.diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [("mdoc.late-operating-system", "late prologue macro: Os")]
    );

    let arbitrary_name = SourceName::new("mdoc-arbitrary-os.1").unwrap();
    let arbitrary = Parser::default()
            .parse(Source::new(
                &arbitrary_name,
                b".Dd $Mdocdate: August 26 2026 $\n.Dt ARBITRARY-OS 1\n.Os ExampleBSD\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
    assert_eq!(
        arbitrary
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            "mdoc.operating-system-explicit",
            "operating system explicitly specified: Os ExampleBSD (NetBSD)",
        )]
    );

    let missing_name = SourceName::new("mdoc-missing-os.1").unwrap();
    let missing = Parser::default()
        .parse(Source::new(
            &missing_name,
            b".Dd August 26, 2026\n.Dt MISSING-OS 1\n.Sh DESCRIPTION\ntext\n",
        ))
        .unwrap();
    assert_eq!(missing.document.metadata().os.as_deref(), Some(""));
    assert_eq!(
        missing
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            "mdoc.operating-system-missing",
            "missing Os macro, using \"\"",
        )]
    );
}

#[test]
fn duplicate_and_late_title_prologues_keep_the_last_pre_body_title() {
    let name = SourceName::new("mdoc-duplicate-title.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FIRST 2 first_arch\n.Os\n.Dt DUPLICATE 1\n.Sh NAME\n.Nm duplicate-title\n.Nd title test\n.Sh DESCRIPTION\ninitial text\n.Dt LATE 3 late_arch\nfinal text\n",
            ))
            .unwrap();
    assert_eq!(
        report.document.metadata().title.as_deref(),
        Some("DUPLICATE")
    );
    assert_eq!(report.document.metadata().section.as_deref(), Some("1"));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.duplicate-prologue", "duplicate prologue macro: Dt"),
            ("mdoc.late-title", "skipping late title macro: Dt"),
        ]
    );
}

#[test]
fn late_only_title_reports_the_missing_eof_title_after_its_source_error() {
    let name = SourceName::new("mdoc-late-only-title.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Os\n.Sh NAME\n.Nm late-title\n.Nd title test\n.Sh DESCRIPTION\ninitial text\n.Dt LATE 1\nfinal text\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            ("mdoc.late-title", "skipping late title macro: Dt"),
            (
                "mdoc.title-missing",
                "missing manual title, using UNTITLED: EOF"
            ),
        ]
    );
    assert_eq!(
        report.document.metadata().title.as_deref(),
        Some("UNTITLED")
    );
    assert_eq!(report.document.metadata().volume.as_deref(), Some("LOCAL"));
}

#[test]
fn title_discards_and_reports_the_first_fourth_argument() {
    let name = SourceName::new("mdoc-title-four-arguments.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FOUR-ARGUMENTS 1 amd64 bogus ignored\n.Os\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [("mdoc.arguments", "skipping excess arguments: Dt ... bogus")]
    );
    assert_eq!(report.document.metadata().arch.as_deref(), Some("amd64"));
}

#[test]
fn obsolete_es_keeps_only_its_delimiter_pair() {
    let name = SourceName::new("mdoc-obsolete-es-arguments.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt OBSOLETE 1\n.Os\n.Sh DESCRIPTION\n.Es << >> surplus\n",
        ))
        .unwrap();
    let es = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Es"))
        .unwrap();
    assert_eq!(es.children().count(), 2);
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("surplus"))
    );
}

#[test]
fn definition_item_command_tags_cover_pipes_xo_and_an_empty_tg() {
    let name = SourceName::new("mdoc-definition-item-tags.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt TAGS 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm one | \\&two\ntext\n.It Xo\n.Cm three\n.Xc\ntext\n.El\n.Tg\n.Cm four\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let item_tags = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
        .map(|node| (node.tag(), node.flags().deep_link_target))
        .collect::<Vec<_>>();
    assert_eq!(item_tags, [(Some("one"), true), (Some("three"), true)]);

    let xo = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Xo"))
        .unwrap();
    assert_eq!(xo.children().count(), 2);

    let commands = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Cm"))
        .map(|node| {
            (
                node.children().next().and_then(crate::NodeRef::text),
                node.tag(),
                node.flags().deep_link_target,
                node.flags().permalink,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        commands,
        [
            (Some("one"), None, false, true),
            (Some("\\&two"), Some("two"), true, true),
            (Some("three"), None, false, true),
            (Some("four"), None, true, true),
        ]
    );
    assert!(nodes.iter().copied().any(|node| {
        node.kind() == NodeKind::Element && node.macro_name() == Some("Tg") && node.flags().no_print
    }));
}

#[test]
fn enclosed_error_terms_move_their_destination_to_the_definition_head() {
    let name = SourceName::new("mdoc-enclosed-error-term.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt ERROR-TERMS 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Er\n.It Er one\nplain error term\n.It Bq Er ENOENT\nenclosed error term\n.El\n",
            ))
            .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let heads = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("It"))
        .map(|node| (node.tag(), node.flags().deep_link_target))
        .collect::<Vec<_>>();
    assert_eq!(heads, [(None, false), (Some("ENOENT"), true)]);

    let errors = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Er"))
        .map(|node| {
            (
                node.children().next().and_then(crate::NodeRef::text),
                node.flags().deep_link_target,
                node.flags().permalink,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        errors,
        [(Some("one"), false, false), (Some("ENOENT"), false, true)]
    );
    assert!(nodes.iter().copied().any(|node| {
        node.kind() == NodeKind::Block
            && node.macro_name() == Some("Bq")
            && node.children().count() == 2
    }));
}

#[test]
fn empty_definition_item_is_safe_for_xo_tag_postprocessing() {
    let name = SourceName::new("mdoc-empty-definition-item.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt EMPTY 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It\n.El\n",
            ))
            .unwrap();
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("It"))
    );
}

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
