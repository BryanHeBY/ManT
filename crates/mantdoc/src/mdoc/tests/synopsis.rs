use super::*;

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
fn see_also_cross_references_report_reversed_section_order() {
    let name = SourceName::new("mdoc-see-also-order.3").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 28, 2026\n.Dt SEE-ALSO 3\n.Os\n.Sh NAME\n.Nm see-also\n.Nd test\n.Sh SEE ALSO\n.Xr first 5 ,\n.Xr second 3\n",
        ))
        .unwrap();
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MDOC_REFERENCE_ORDER)
        .unwrap();
    assert_eq!(
        diagnostic.message.as_ref(),
        "unusual Xr order: second(3) after first(5)"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (9, 2));
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
fn paragraph_resets_function_tag_priority_without_erasing_an_equal_target() {
    let name = SourceName::new("mdoc-function-tag-priority.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt FO 1\n.Os\n.Sh DESCRIPTION\n.Fn call\n.Pp\n.Fo call\n.Fc\n",
        ))
        .unwrap();
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let function = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .unwrap();
    assert!(function.flags().deep_link_target);
    assert!(function.flags().permalink);
    let paragraph = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Pp"))
        .unwrap();
    assert!(paragraph.flags().deep_link_target);
    assert_eq!(paragraph.tag(), Some("call"));
    let head = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("Fo"))
        .unwrap();
    assert!(!head.flags().deep_link_target);
    assert!(head.flags().permalink);
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
fn every_description_function_contributes_an_automatic_target_candidate() {
    let name = SourceName::new("mdoc-fn-standalone-targets.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 25, 2026\n.Dt FN 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Fn grouped\nand\n.Fn second\n.Ss Independent declarations\nThe functions\n.Fn third\nand\n.Fn fourth\n",
        ))
        .unwrap();
    let functions = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("Fn"))
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 4);
    assert!(!functions[0].flags().deep_link_target);
    assert!(functions[0].flags().permalink);
    assert!(functions[1].flags().deep_link_target);
    assert!(functions[1].flags().permalink);
    assert!(functions[2].flags().deep_link_target);
    assert!(functions[2].flags().permalink);
    assert!(functions[3].flags().deep_link_target);
    assert!(functions[3].flags().permalink);
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
