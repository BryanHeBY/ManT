use super::*;

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
fn an_inline_xc_closes_an_extended_item_head_before_its_body() {
    let name = SourceName::new("mdoc-item-xo-inline-close.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".Dd August 27, 2026\n.Dt ITEM-XO 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It Li outer Xo\n.Oo heading Oc Xc\nbody text\n.El\n",
        ))
        .unwrap();
    let body_text = report
        .document
        .preorder()
        .find(|node| node.text() == Some("body text"))
        .expect("body text is retained");
    assert_eq!(
        body_text.parent().and_then(crate::NodeRef::macro_name),
        Some("It")
    );
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
