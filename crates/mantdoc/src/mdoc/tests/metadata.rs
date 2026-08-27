use super::*;

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
fn derives_mdoc_section_four_volume_from_the_upstream_section_table() {
    let name = SourceName::new("section-four.4").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 28, 2026\n.Dt SECTION-FOUR 4\n.Os\n.Sh NAME\n.Nm section-four\n.Nd test\n",
        ))
        .unwrap();

    assert_eq!(
        report.document.metadata().volume.as_deref(),
        Some("Device Drivers Manual")
    );
}

#[test]
fn author_names_are_one_phrase_before_a_callable_inline_macro() {
    let name = SourceName::new("author-phrase.4").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 28, 2026\n.Dt AUTHOR-PHRASE 4\n.Os\n.Sh AUTHORS\n.An Bill Paul Aq Mt author@example.com .\n",
        ))
        .unwrap();

    let author = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("An"))
        .unwrap();
    assert_eq!(
        author
            .children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["Bill Paul"]
    );
    let address = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("Aq"))
        .unwrap();
    assert_eq!(address.children().count(), 2);
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.text() == Some("."))
            .count(),
        1
    );
}

#[test]
fn openbsd_rcs_id_requires_mdocdate_in_the_first_date_prologue() {
    let name = SourceName::new("openbsd-mdocdate.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".\\\" $OpenBSD: openbsd-mdocdate.1,v 1.0 2026/08/28 00:00:00 maintainer Exp $\n.Dd bad date\n.Dt DATE 1\n.Os\n.Sh NAME\n.Nm date\n.Nd test\n",
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
                "mdoc.date-unparseable",
                "cannot parse date, using it verbatim: Dd bad date",
            ),
            (
                "mdoc.mdocdate-missing",
                "Mdocdate missing: Dd bad date (OpenBSD)",
            ),
        ]
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
fn section_headings_parse_callable_vt_as_an_inline_element() {
    let name = SourceName::new("mdoc-section-vt.3").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 28, 2026\n.Dt SECTION-VT 3\n.Os\n.Sh NAME\n.Nm section-vt\n.Nd test\n.Sh DESCRIPTION\n.Ss Copying to and from Vt struct stat\n",
        ))
        .unwrap();
    let heading = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Ss"))
        .unwrap();
    let children = heading.children().collect::<Vec<_>>();
    assert_eq!(children[0].text(), Some("Copying to and from"));
    assert_eq!(children[1].macro_name(), Some("Vt"));
    assert_eq!(
        children[1]
            .children()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>(),
        ["struct", "stat"]
    );
    assert_eq!(heading.tag(), Some("Copying_to_and_from_struct_stat"));
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
