use super::*;

#[test]
fn m3_environment_requests_expand_text_and_control_arguments() {
    let name = SourceName::new("environment.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds title mantdoc\n.nr count 7\n.TH \\*[title] \\n[count]\ntext \\*[title] \\n[count]\n.as title -rs\n\\*[title]\n.rm title count\n\\*[title] \\n[count]\n",
            ))
            .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(report.document.metadata().title.as_deref(), Some("mantdoc"));
    assert_eq!(report.document.metadata().section.as_deref(), Some("7"));
    assert_eq!(nodes[0].text(), Some("text mantdoc 7"));
    assert_eq!(nodes[1].text(), Some("mantdoc-rs"));
    assert_eq!(nodes[2].text(), Some(" 0"));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|finding| finding.code.as_str() == "roff.undefined-reference")
            .count(),
        1
    );
}

#[test]
fn empty_user_strings_are_silent_in_control_position() {
    let name = SourceName::new("empty-string-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH EMPTY-STRING 1 28-Aug-2026\n.SH DESCRIPTION\n.ds empty \"\n.empty\nvisible\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("empty"))
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("visible"))
    );
}

#[test]
fn user_macro_fill_requests_apply_before_the_next_physical_text_line() {
    let name = SourceName::new("pod-verbatim.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH POD-VERBATIM 1 28-Aug-2026\n.SH DESCRIPTION\n.de Vb\n.nf\n..\n.de Ve\n.fi\n..\n.Vb\n\\&        \n.Ve\n",
        ))
        .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn mdoc_control_arguments_expand_unescaped_string_references() {
    let name = SourceName::new("mdoc-string-argument.1").unwrap();
    let report = Parser::new(ParserConfig {
            operating_system: Some("mantdoc canonical differential".into()),
            ..ParserConfig::default()
        })
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt STRING-ARG 1\n.Os\n.Sh DESCRIPTION\n.ds o \\(Fo\n.Eo \\*o\nbody\n.Ec \\*o\n.Pp\n.Eo \\\\*o\nbody\n.Ec \\\\*o\n",
            ))
            .unwrap();
    let texts = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(texts.iter().filter(|text| **text == "\\(Fo").count() >= 4);
    assert!(!texts.contains(&"\\*o"));
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
}

#[test]
fn m3_string_definitions_retain_the_full_unquoted_value() {
    let name = SourceName::new("string-value.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ds phrase native rust parser\n\\*[phrase]\n.as phrase with bounds\n\\*[phrase]\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        ["native rust parser", "native rust parserwith bounds"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn recursive_string_expansion_drops_only_its_own_input_line() {
    let name = SourceName::new("recursive-string.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH RECURSIVE-STRING 1 28-Aug-2026\n.SH DESCRIPTION\n.ds recur \\\\*[recur]\nbefore recursion\n(and do not \\*[recur] print this)\nafter recursion\n",
            ))
            .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        (
            report.diagnostics[0].code.as_str(),
            report.diagnostics[0].severity,
            report.diagnostics[0].message.as_ref(),
        ),
        (
            DiagnosticCode::LIMIT_EXPANSION_STEPS,
            Severity::Error,
            "input stack limit exceeded, infinite loop?",
        )
    );
    assert_eq!(
        report.diagnostics[0]
            .primary
            .as_ref()
            .and_then(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column)),
        Some((5, 13))
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"before recursion"));
    assert!(text.contains(&"after recursion"));
    assert!(!text.iter().any(|value| value.contains("print this")));
}

#[test]
fn string_definition_names_normalize_literal_escapes_and_reject_other_escapes() {
    let name = SourceName::new("string-escaped-name.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ds std\\\\esc stdval\n\\*[std\\\\esc]\n.ds esc\\eesc ignored\n\\*[esc]\n.ds bl\\ e ignored\n",
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
                DiagnosticCode::ROFF_ESCAPED_NAME,
                "escaped character not allowed in a name: esc\\e",
            ),
            (
                DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                "undefined string, using \"\": esc",
            ),
            (
                DiagnosticCode::ROFF_ESCAPED_NAME,
                "escaped character not allowed in a name: bl\\ ",
            ),
        ]
    );
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"stdval"));
    assert!(!text.contains(&"ignored"));
}

#[test]
fn mdoc_bracketed_string_name_preserves_its_literal_escape_until_lookup() {
    let name = SourceName::new("mdoc-string-escaped-name.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt ESCAPED-NAME 1\n.Os\n.Sh NAME\n.Nm escaped-name\n.Nd test\n.Sh DESCRIPTION\n.ds std\\\\esc stdval\n.Sq \\*[std\\\\esc] .\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("stdval"))
    );
    assert!(
        !report
            .document
            .preorder()
            .any(|node| node.text() == Some(""))
    );
}

#[test]
fn m3_copy_mode_macro_body_expands_arguments_at_invocation() {
    let name = SourceName::new("macro.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds salutation welcome\n.de greet\nHello, \\$1!\n\\*[salutation]\n..\n.ds salutation later\n.greet mantdoc\n",
            ))
            .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].text(), Some("Hello, mantdoc!"));
    assert_eq!(nodes[1].text(), Some("welcome"));
    assert!(nodes.iter().all(|node| !node.flags().generated));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_generated_controls_relex_expanded_macro_arguments() {
    let name = SourceName::new("macro-expanded-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH MACRO 1 28-Aug-2026\n.SH DESCRIPTION\n.de show\n.BI \\$@\n..\n.show one two three\n",
        ))
        .unwrap();
    let bold_italic = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("BI"))
        .unwrap();
    assert_eq!(
        bold_italic
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["one", "two", "three"]
    );
    assert_eq!(
        bold_italic
            .children()
            .map(|argument| {
                report
                    .document
                    .source_position(argument.location().expect("argument location"))
                    .expect("argument source position")
            })
            .collect::<Vec<_>>(),
        [
            crate::SourcePosition { line: 6, column: 5 },
            crate::SourcePosition {
                line: 6,
                column: 11
            },
            crate::SourcePosition {
                line: 6,
                column: 17
            },
        ]
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn man_attached_name_escape_rebases_the_first_visible_argument() {
    let name = SourceName::new("attached-man-escape.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH ATTACHED 1 28-Aug-2026\n.SH DESCRIPTION\n.IB\\(lqone two\n",
        ))
        .unwrap();
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("IB"))
        .expect("recovered IB macro");
    let first = macro_node.children().next().expect("first argument");
    assert_eq!(first.text(), Some("one"));
    assert_eq!(
        report
            .document
            .source_position(first.location().expect("argument location")),
        Some(crate::SourcePosition { line: 3, column: 8 })
    );
}

#[test]
fn m3_direct_definition_in_a_macro_spans_pending_and_following_input() {
    let name = SourceName::new("nested-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de outer\nouter macro\n.de inner\ninner macro\n..\nouter definition ended\n.outer\nfollowing caller input\n..\ninner definition ended\n.inner\nfinal text\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "outer definition ended",
            "outer macro",
            "inner definition ended",
            "inner macro",
            "following caller input",
            "final text",
        ]
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn roff_input_traps_reparse_the_armed_macro_after_the_matching_text_line() {
    let name = SourceName::new("input-trap.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INPUT-TRAP 1 28-Aug-2026\n.SH DESCRIPTION\n.de first\nfirst trap\n..\n.de second\nsecond trap\n..\n.it 1first\none\n.it 2 second\ntwo\nthree\nfour\n",
            ))
            .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "DESCRIPTION",
            "one",
            "first trap",
            "two",
            "three",
            "second trap",
            "four"
        ]
    );
    assert!(
        report
            .document
            .preorder()
            .all(|node| node.macro_name() != Some("it"))
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn man_builtin_macro_names_take_precedence_over_roff_definitions() {
    let name = SourceName::new("defined-man-macro.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH DEFINED-MAN 1 28-Aug-2026\n.de BI\n.IB \\$1 \\$2 \\$3\n..\n.SH DESCRIPTION\n.BI bold italic bold\n",
            ))
            .unwrap();
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("BI"))
        .expect("the authored BI remains a man element");
    let children = macro_node
        .children()
        .map(|node| node.text().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(children, ["bold", "italic", "bold"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn implemented_mdoc_macro_names_take_precedence_over_roff_definitions() {
    let name = SourceName::new("defined-mdoc-macro.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt DEFINED-MDOC 1\n.Os\n.de At\nBSD\n..\n.Sh DESCRIPTION\n.At\n",
            ))
            .unwrap();
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("At"))
        .expect("the authored At remains an mdoc element");
    let child = macro_node.children().next().expect("At default child");
    assert_eq!(child.text(), Some("AT&T UNIX"));
    assert!(child.flags().generated);
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
}

#[test]
fn at_expands_standard_versions_and_recovers_unknown_selectors() {
    let name = SourceName::new("at-versions.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt AT-VERSIONS 1\n.Os\n.Sh DESCRIPTION\n.At v7\n.At murks \"Sy\" bold\n",
            ))
            .unwrap();
    let at_nodes = report
        .document
        .preorder()
        .filter(|node| node.macro_name() == Some("At"))
        .collect::<Vec<_>>();
    assert_eq!(at_nodes.len(), 2);
    let valid_children = at_nodes[0].children().collect::<Vec<_>>();
    assert_eq!(
        valid_children
            .iter()
            .copied()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["Version\\~7 AT&T UNIX", "v7"]
    );
    assert!(valid_children[0].flags().generated);
    assert!(valid_children[1].flags().no_print);
    let invalid_children = at_nodes[1].children().collect::<Vec<_>>();
    assert_eq!(
        invalid_children
            .iter()
            .copied()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["AT&T UNIX", "murks"]
    );
    assert!(invalid_children[0].flags().generated);
    assert!(report.document.preorder().any(|node| {
        node.macro_name() == Some("Sy")
            && node.children().next().and_then(crate::NodeRef::text) == Some("bold")
    }));
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME,
            "mdoc.unknown-at-version",
        ]
    );
    assert_eq!(
        report.diagnostics[1].message.as_ref(),
        "unknown AT&T UNIX version: At murks"
    );
}

#[test]
fn appended_mdoc_closer_keeps_its_builtin_scope_action() {
    let name = SourceName::new("appended-mdoc-closer.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt APPENDED-CLOSER 1\n.Os\n.Sh DESCRIPTION\n.Bo in brackets\n.Bc end\n.am Bc\n.Pq appended words\n..\n.Bo in brackets\n.Bc end\n",
            ))
            .unwrap();
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::MDOC_FIRST_SECTION_NOT_NAME
    }));
    let bracket_bodies = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        .map(|body| {
            body.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(bracket_bodies, [["in brackets"], ["in brackets"]]);
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.macro_name() == Some("Pq"))
    );
}

#[test]
fn renamed_appended_mdoc_closer_keeps_scope_and_caller_provenance() {
    let name = SourceName::new("renamed-appended-mdoc-closer.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 25, 2026\n.Dt RENAMED-APPENDED-CLOSER 1\n.Os\n.Sh NAME\n.Nm renamed-appended-closer\n.Nd package macro alias\n.Sh DESCRIPTION\n.rn Bc myBc\n.Bo first brackets\n.myBc\n.am myBc\n.Pq appended words\n..\n.Bo second brackets\n.myBc\n",
            ))
            .unwrap();
    assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::INPUT_TRAILING_WHITESPACE
    );
    let diagnostic_position = report
        .document
        .source_position(report.diagnostics[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        (diagnostic_position.line, diagnostic_position.column),
        (15, 4)
    );

    let bracket_bodies = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Body && node.macro_name() == Some("Bo"))
        .map(|body| {
            body.children()
                .filter_map(crate::NodeRef::text)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(bracket_bodies, [["first brackets"], ["second brackets"]]);

    let appended_text = report
        .document
        .preorder()
        .find(|node| node.text() == Some("appended words"))
        .unwrap();
    let appended_position = report
        .document
        .source_position(appended_text.location().unwrap())
        .unwrap();
    assert_eq!((appended_position.line, appended_position.column), (15, 5));
}

#[test]
fn m3_indirect_macro_definitions_expand_names_and_custom_terminators() {
    let name = SourceName::new("indirect-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds target delayed\n.ds end-marker done\n.dei target end-marker\nfirst\n.done trailing words\n.ami target end-marker\nsecond\n.done\n.delayed\n",
            ))
            .unwrap();

    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["first", "second"]);
}

#[test]
fn m3_copy_mode_reparses_delayed_register_adjustments_on_invocation() {
    let name = SourceName::new("copy-register.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 2 1\n.de decrement\n\\\\n-[count]\n..\n.decrement\ncount \\n[count]\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["1", "count 1"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_while_brace_scope_reexecutes_controls_and_closes_inline_text() {
    let name = SourceName::new("while-scope.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 3\n.while \\n[count] \\{\\\n.nr count -1\n\\n[count]\\},\nafter\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2,", "1,", "0,", "after"]);
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_break_in_a_scoped_conditional_stops_only_the_current_while() {
    let name = SourceName::new("while-break.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 3 1\n.while n \\{\\\n\\n-[count]\n.if !\\n[count] .break\nnext\n.\\}\nafter\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2", "next", "1", "next", "0", "after"]);
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_nested_while_scopes_execute_on_an_explicit_frame_stack() {
    let name = SourceName::new("nested-while.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr outer 2\n.while \\n[outer] \\{\\\n.nr inner 2\n.while \\n[inner] \\{\\\n\\n[outer]:\\n[inner]\n.nr inner -1\n.\\}\n.nr outer -1\n.\\}\nafter\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2:2", "2:1", "after"]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::ROFF_WHILE_NESTED,
            DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
        ]
    );
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_macro_body_can_close_the_active_while_scope() {
    let name = SourceName::new("while-macro-close.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2\n.de close\n.nr count -1\n.\\}\n..\n.while \\n[count] \\{\\\n\\n[count]\n.close\ninside-never\n.\\}\nafter\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["2", "inside-never", "after"]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.severity))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_WHILE_INNER_SCOPE,
                Severity::Unsupported
            ),
            (
                DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                Severity::Unsupported
            ),
        ]
    );
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_copy_mode_does_not_apply_control_changes_before_macro_invocation() {
    let name = SourceName::new("copy-control.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".de delayed\n.cc !\n..\noutside\n"))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("outside"));
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "roff.unterminated-definition")
    );
}

#[test]
fn m3_macro_control_changes_activate_only_when_the_macro_runs() {
    let name = SourceName::new("copy-control-run.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de delayed\n.cc !\n!B generated\n..\n.delayed\n!TH title 1\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].macro_name(), Some("B"));
    assert!(!nodes[0].flags().generated);
    assert_eq!(
        nodes[0].children().next().unwrap().text(),
        Some("generated")
    );
    assert_eq!(nodes[1].macro_name(), Some("TH"));
    assert_eq!(
        nodes[1]
            .children()
            .map(|node| node.text().unwrap().to_owned())
            .collect::<Vec<_>>(),
        ["title", "1"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macro_body_control_requests_become_generated_events() {
    let name = SourceName::new("macro-controls.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de show\n.ds prefix welcome\n.B \\$1\n..\n.show mantdoc\n\\*[prefix]\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].macro_name(), Some("B"));
    assert!(!nodes[0].flags().generated);
    assert_eq!(nodes[0].children().next().unwrap().text(), Some("mantdoc"));
    assert_eq!(nodes[1].text(), Some("welcome"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macro_generated_man_controls_use_the_invocation_control_column() {
    let name = SourceName::new("generated-man-control.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH GENERATED 1 28-Aug-2026\n.de list\n.TP 6n\ntag\n..\n.list\ntext\n",
        ))
        .unwrap();
    let term = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
        .expect("generated TP block");
    let position = report
        .document
        .source_position(term.location().expect("TP location"))
        .expect("TP source position");
    assert_eq!((position.line, position.column), (6, 2));
    let head = term.children().next().expect("TP head");
    let width = head.children().next().expect("TP width argument");
    let width_position = report
        .document
        .source_position(width.location().expect("width location"))
        .expect("width source position");
    assert_eq!((width_position.line, width_position.column), (6, 5));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_macros_can_invoke_nested_macros_with_their_own_arguments() {
    let name = SourceName::new("nested-macros.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de inner\ninner: \\$1\n..\n.de outer\n.inner \\$1\n..\n.outer mantdoc\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("inner: mantdoc"));
    assert!(!nodes[0].flags().generated);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_recursive_macros_reparse_delayed_register_and_argument_escapes() {
    let name = SourceName::new("recursive-macro.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de count\n. ie \\\\$1>0 \\{\\\n.  No \\\\$1\n.  nr next \\\\$1-1\n.  count \\\\n[next]\n. \\}\n..\n.count 3\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["3", "2", "1"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn m3_macro_shift_return_and_argument_count_are_frame_local() {
    let name = SourceName::new("macro-control-flow.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de inner\ninner \\$1 \\n[.$]\n.return\ninner-never\n..\n.de outer\nouter-before \\$1 \\$2\n.shift\n.inner \\$1\nouter-after \\$1\n.return\nouter-never\n..\n.outer one two\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        ["outer-before one two", "inner two 1", "outer-after two"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn shift_recovers_outside_calls_and_invalid_macro_selectors() {
    let name = SourceName::new("shift-recovery.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH SHIFT-RECOVERY 1 \"August 26, 2026\"\n.SH NAME\nshift-recovery - shift validation\n.SH DESCRIPTION\n.shift\n.de mym\nselector: \"\\\\$x\"\n.shift bad\nafter invalid: \"\\\\$1\"\n.shift 2\nafter excessive: \"\\\\$1\"\n..\n.mym one two\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (Severity::Error, "ignoring request outside macro: shift"),
            (Severity::Error, "argument number is not numeric: \\$x"),
            (
                Severity::Error,
                "argument is not numeric, using 1: shift bad"
            ),
            (Severity::Error, "excessive shift: 2, but max is 1"),
        ]
    );
    let text = report
        .document
        .preorder()
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"after invalid: \"two\""), "{text:#?}");
    assert!(text.contains(&"after excessive: \"\""), "{text:#?}");
    assert!(!text.iter().any(|value| value.contains("$x")), "{text:#?}");
}

#[test]
fn empty_while_scope_keeps_validator_order_and_logical_blank_location() {
    let name = SourceName::new("while-empty-scope.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd August 26, 2026\n.Dt WHILE-EMPTY 1\n.Os\n.Sh NAME\n.Nm while-empty\n.Nd test\n.Sh DESCRIPTION\nbefore\n.nr cnt 2 1\n.while \\n-[cnt]\n\\n[cnt]\n.Pp\nfinal text\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        [
            "conditional request controls empty scope: while",
            "blank line in fill mode, using .sp",
            "conditional request controls empty scope: while",
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(10, 2), (10, 9), (10, 2)]
    );
}

#[test]
fn roff_return_and_argument_escapes_outside_macros_are_errors() {
    let name = SourceName::new("return-outside.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".return\noutside \\$1\n.return\n"))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                "ignoring request outside macro: return",
            ),
            (
                DiagnosticCode::ROFF_MACRO_ARGUMENT_OUTSIDE,
                "using macro argument outside macro: \\$1",
            ),
            (
                DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                "ignoring request outside macro: return",
            ),
        ]
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.primary.as_ref())
            .filter_map(|span| report.document.source_position(span))
            .map(|position| (position.line, position.column))
            .collect::<Vec<_>>(),
        [(1, 2), (2, 9), (3, 2)]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["outside "]
    );
}

#[test]
fn m3_macro_depth_limit_returns_a_coherent_prefix() {
    let name = SourceName::new("macro-depth.roff").unwrap();
    let limits = Limits {
        max_macro_depth: 1,
        ..Limits::default()
    };
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse(Source::new(
        &name,
        b".de second\nsecond\n..\n.de first\nfirst-text\n.second\n..\n.first\n",
    ))
    .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("first-text"));
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.macro-depth")
    );
}
