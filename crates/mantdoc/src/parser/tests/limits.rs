use super::*;

#[test]
fn root_input_limit_is_fatal_before_ast_allocation() {
    let name = SourceName::new("too-large.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_root_source_bytes = 3;
    let error = Parser::new(config)
        .parse(Source::new(&name, b"four"))
        .unwrap_err();
    assert_eq!(error.kind, FatalErrorKind::SourceLimit);
}

#[test]
fn source_line_limit_bounds_document_line_index_allocation() {
    let name = SourceName::new("many-lines.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_source_lines = 2;
    let error = Parser::new(config)
        .parse(Source::new(&name, b"one\ntwo\nthree"))
        .unwrap_err();
    assert_eq!(error.kind, FatalErrorKind::SourceLineLimit);
}

#[test]
fn scanner_emits_control_arguments_and_honors_dynamic_characters() {
    let name = SourceName::new("dynamic.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".cc !\n!ec @\nvisible @(em @\\ tail\n!TH \"two words\" 1\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes[0].macro_name(), Some("cc"));
    assert_eq!(nodes[1].macro_name(), Some("ec"));
    assert_eq!(nodes[2].text(), Some("visible — @ tail"));
    assert_eq!(nodes[3].macro_name(), Some("TH"));
    assert_eq!(
        nodes[3]
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>(),
        ["two words", "1"]
    );
}

#[test]
fn character_control_requests_are_private_and_discard_excess_bytes_in_man_input() {
    let name = SourceName::new("character-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CHARACTER-CONTROL 1 28-Aug-2026\n.SH DESCRIPTION\n.cc :\n:cc ;bogus\ntext\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
            "skipping excess arguments: cc ... bogus",
        )]
    );
    let position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (4, 6));
    assert!(
        report
            .document
            .preorder()
            .all(|node| !matches!(node.macro_name(), Some("cc" | "c2" | "ec")))
    );
}

#[test]
fn roff_font_requests_validate_and_project_the_legacy_ft_shape() {
    let name = SourceName::new("font-request.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd January 1, 2020\n.Dt FONT-REQUEST 1\n.Os\n.Sh NAME\n.Nm font-request\n.Nd font validation\n.Sh DESCRIPTION\n.ft B\n.ft foo\n.ft I bogus\n.ft P\n.ft\n",
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
                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                "skipping excess arguments: ft ... bogus",
            ),
            (
                DiagnosticCode::ROFF_UNKNOWN_FONT,
                "unknown font, skipping request: ft foo",
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
        [Some((10, 7)), Some((9, 2))]
    );
    let fonts = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("ft"))
        .map(|node| {
            node.children()
                .map(crate::NodeRef::text)
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()
        .unwrap();
    assert_eq!(fonts, [vec!["B"], vec!["I"], vec!["P"], vec!["P"]]);
}

#[test]
fn char_requests_are_private_but_expand_declared_character_values() {
    let name = SourceName::new("character-definitions.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CHARACTER-DEFINITIONS 1 28-Aug-2026\n.SH DESCRIPTION\n.char \\[myc] myval\n.char x y\n.char \\[boldX] \\fBX\n\\[boldX] \\[myc]\nfinal text\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "invalid escape sequence: \\[myc]",
            "invalid escape sequence: \\[boldX]",
            "invalid escape sequence: \\[myc]",
            "invalid escape sequence: \\[boldX]",
        ]
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"\\fBX\\fP myval"));
    assert!(text.contains(&"final teyt"));
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("char"))
    );
}

#[test]
fn char_requests_report_invalid_left_operands_at_their_precise_source_spans() {
    let name = SourceName::new("character-invalid.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CHARACTER-INVALID 1 28-Aug-2026\n.SH DESCRIPTION\n.char\n.char \\fR myval\n.char \\[myc]x myval\n.char xy myval\nmyc: <\\[myc]> x\n",
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
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char ",
            ),
            (
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char \\fR myval",
            ),
            (
                DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                "invalid escape sequence: \\[myc]",
            ),
            (
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char \\[myc]x myval",
            ),
            (
                DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
                "argument is not a character: char xy myval",
            ),
            (
                DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                "invalid escape sequence: \\[myc]",
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
    assert_eq!(positions, [(3, 6), (4, 7), (5, 7), (5, 7), (6, 7), (7, 7)]);
}

#[test]
fn scanner_limits_return_a_bounded_prefix_and_typed_findings() {
    let name = SourceName::new("bounded.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_nodes = 2;
    config.limits.max_diagnostics = 4;
    let report = Parser::new(config)
        .parse(Source::new(&name, b"one\ntwo\nthree\n"))
        .unwrap();
    assert_eq!(report.document.node_count(), 2);
    assert!(report.statistics.truncated);
    assert_eq!(report.diagnostics[0].code.as_str(), "limits.nodes");
}

#[test]
fn default_tree_limit_matches_the_legacy_finite_prefix_boundary() {
    let name = SourceName::new("deep-man.1").unwrap();
    let mut source = String::from(".TH DEEP 1 28-Aug-2026\n.SH BODY\n");
    for _ in 0..300 {
        source.push_str(".RS\n");
    }
    source.push_str("retained prefix\n");
    for _ in 0..300 {
        source.push_str(".RE\n");
    }

    let report = Parser::default()
        .parse(Source::new(&name, source.as_bytes()))
        .unwrap();
    assert!(report.statistics.truncated);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::LEGACY_SYNTAX_TREE_DEPTH_LIMIT
    }));
    assert_eq!(
        report.document.node_count(),
        report.document.preorder().count()
    );
    assert_eq!(maximum_document_depth(&report.document), 256);
    assert_eq!(
        report.statistics.emitted_nodes,
        report.document.node_count()
    );
}

#[test]
fn caller_selected_tree_limit_uses_the_native_limit_code() {
    let name = SourceName::new("narrow-tree.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_tree_depth = 4;
    let mut source = String::from(".TH NARROW 1 28-Aug-2026\n.SH BODY\n");
    for _ in 0..10 {
        source.push_str(".RS\n");
    }
    source.push_str("text\n");
    let report = Parser::new(config)
        .parse(Source::new(&name, source.as_bytes()))
        .unwrap();

    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::LIMIT_TREE_DEPTH)
    );
    assert_eq!(maximum_document_depth(&report.document), 4);
}

#[test]
fn m3_semantic_staging_respects_the_node_budget_before_adding_man_parts() {
    let name = SourceName::new("bounded-man.1").unwrap();
    let mut config = ParserConfig {
        syntax: Syntax::Man,
        ..ParserConfig::default()
    };
    config.limits.max_nodes = 4;
    let report = Parser::new(config)
        .parse(Source::new(&name, b".SH BOUNDED\n"))
        .unwrap();

    // The scanner emitted root, SH, and its argument.  Forming the
    // staging Block/Head/Body shape needs two more nodes, so the original
    // event remains reachable rather than exceeding max_nodes.
    assert_eq!(report.document.node_count(), 3);
    let section = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(section.kind(), NodeKind::Element);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.nodes")
    );
}

#[test]
fn m5_semantic_staging_respects_the_node_budget_before_adding_mdoc_parts() {
    let name = SourceName::new("bounded-mdoc.1").unwrap();
    let mut config = ParserConfig {
        syntax: Syntax::Mdoc,
        ..ParserConfig::default()
    };
    config.limits.max_nodes = 4;
    let report = Parser::new(config)
        .parse(Source::new(&name, b".Sh BOUNDED\n"))
        .unwrap();

    // The scanner emitted root, Sh, and its argument. Forming the
    // staging Block/Head/Body shape needs two more nodes, so the original
    // event remains reachable rather than exceeding max_nodes.
    assert_eq!(report.document.node_count(), 3);
    let section = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(section.kind(), NodeKind::Element);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.nodes")
    );
}

#[test]
fn aggregate_escape_work_limit_stops_before_unbounded_scanner_output() {
    let name = SourceName::new("escapes.1").unwrap();
    let mut config = ParserConfig::default();
    config.limits.max_expansion_steps = 1;
    let report = Parser::new(config)
        .parse(Source::new(&name, b"\\&\\&\nnext\n"))
        .unwrap();
    assert_eq!(report.document.node_count(), 1);
    assert!(report.statistics.truncated);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        "limits.expansion-steps"
    );
}

#[test]
fn scanner_is_total_and_source_bounded_for_every_two_byte_prefix() {
    let name = SourceName::new("all-byte-prefixes.roff").unwrap();
    let parser = Parser::default();
    for first in u8::MIN..=u8::MAX {
        for second in u8::MIN..=u8::MAX {
            let bytes = [first, second];
            let report = parser.parse(Source::new(&name, &bytes)).unwrap();
            assert_eq!(
                report.document.node_count(),
                report.statistics.emitted_nodes
            );
            for node in report.document.preorder() {
                if let Some(span) = node.location() {
                    assert!(span.start <= span.end);
                    assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                    assert!(report.document.source_position(span).is_some());
                }
            }
            for finding in &report.diagnostics {
                for span in finding
                    .primary
                    .iter()
                    .chain(finding.related.iter().map(|related| &related.span))
                {
                    assert!(span.start <= span.end);
                    assert!(usize::try_from(span.end).unwrap() <= bytes.len());
                    assert!(report.document.source_position(span).is_some());
                }
            }
        }
    }
}

#[test]
fn dynamic_character_requests_keep_public_spans_inside_source_bytes() {
    let name = SourceName::new("fuzz-dynamic-control.roff").unwrap();
    let bytes = b".cc !\x8c";
    let report = Parser::default().parse(Source::new(&name, bytes)).unwrap();
    for node in report.document.preorder() {
        if let Some(span) = node.location() {
            assert!(span.start <= span.end);
            assert!(usize::try_from(span.end).unwrap() <= bytes.len());
            assert!(report.document.source_position(span).is_some());
        }
    }
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_EXCESS_ARGUMENTS
    );
    for finding in &report.diagnostics {
        if let Some(span) = &finding.primary {
            assert!(span.start <= span.end);
            assert!(usize::try_from(span.end).unwrap() <= bytes.len());
            assert!(report.document.source_position(span).is_some());
        }
    }
}

#[test]
fn man_title_validation_keeps_malformed_input_diagnostic_spans_in_bounds() {
    let name = SourceName::new("fuzz-man-title.roff").unwrap();
    let bytes = b".TH A\xc7n";
    let report = Parser::default().parse(Source::new(&name, bytes)).unwrap();
    let finding = report
        .diagnostics
        .iter()
        .find(|finding| finding.code.as_str() == DiagnosticCode::MAN_TITLE_NOT_UPPERCASE)
        .expect("malformed title still contains an ASCII lower-case character");
    let span = finding.primary.as_ref().expect("title finding has a span");
    assert_eq!((span.start, span.end), (6, 7));
    assert!(usize::try_from(span.end).unwrap() <= bytes.len());
    assert!(report.document.source_position(span).is_some());
}

#[test]
fn pinned_head_c_escape_shape_is_visible_before_roff_scope_execution() {
    let name = SourceName::new("c_man.in").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".B\none\\c\nword\n"))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes[0].macro_name(), Some("B"));
    assert_eq!(nodes[1].text(), Some("one"));
    assert!(nodes[1].flags().line_continuation);
    assert_eq!(nodes[2].text(), Some("word"));
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
}

#[test]
fn package_ast_retains_a_final_no_space_escape() {
    let name = SourceName::new("package-c.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH PACKAGE-C 1 28-Aug-2026\n.SH DESCRIPTION\none\\c\nword\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .find(|node| node.text() == Some("one\\c"))
        .expect("the package AST retains the authored escape");
    assert!(text.flags().line_continuation);
    assert!(report.diagnostics.is_empty());
}
