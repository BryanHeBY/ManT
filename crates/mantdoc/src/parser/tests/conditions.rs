use super::*;

#[test]
fn same_line_conditionals_reparse_nested_requests() {
    let name = SourceName::new("nested-inline-condition.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 .if 1 nested-true\n.if 1 .if 0 hidden\n.nr count 0\n.if 1 .if 1 .nr count 1\n.if \\n[count] register-updated\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .filter_map(|node| node.text().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(text, ["nested-true", "register-updated"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn nested_inline_conditional_in_a_scope_keeps_its_body_location() {
    let name = SourceName::new("nested-scope-location.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH NESTED 1 28-Aug-2026\n.SH DESCRIPTION\n.if n \\{outer\n.if n inner\n.\\}\n",
        ))
        .unwrap();
    let node = report
        .document
        .preorder()
        .find(|node| node.text() == Some("inner"))
        .unwrap();
    let position = report
        .document
        .source_position(node.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (4, 7));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn inline_conditional_user_macro_keeps_the_physical_request_column() {
    let name = SourceName::new("conditional-user-macro-location.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CONDITIONAL 1 28-Aug-2026\n.SH DESCRIPTION\n.de visible\nmacro body\n..\n.if n .visible\n",
        ))
        .unwrap();
    let node = report
        .document
        .preorder()
        .find(|node| node.text() == Some("macro body"))
        .unwrap();
    let position = report
        .document
        .source_position(node.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (6, 1));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn selected_conditionals_reenter_definition_and_else_dispatch() {
    let name = SourceName::new("conditional-rerun.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 .de emit\ndefined through selected request\n..\n.emit\n.if 1 .ie 0 hidden\n.el selected else\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["defined through selected request", "selected else"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn selected_conditionals_reenter_ignore_copy_mode() {
    let name = SourceName::new("conditional-ignore-rerun.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr zY 1\n.if \\n(zY=1 .ig zY\nhidden copy-mode input\n.zY\nvisible\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["visible"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn nop_and_selected_nop_reparse_their_remainder() {
    let name = SourceName::new("nop-rerun.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nop direct text\n.if 1 .nop selected text\n.nop .if 1 nested control\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["direct text", "selected text", "nested control"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn single_line_while_reparses_nested_controls_and_loop_flow() {
    let name = SourceName::new("while-rerun.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 2\n.while \\n[count] .if 1 .nr count -1\n.if !\\n[count] nested-register-body\n.while 1 .break\nafter-break\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["nested-register-body", "after-break"]);
    assert!(!report.statistics.truncated);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn selected_controls_share_register_string_translation_and_character_state() {
    let name = SourceName::new("conditional-environment-rerun.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 .ds selected string-state\n.if 1 .nr selected-register 1\n.if \\n[selected-register] \\*[selected]\n.if 1 .tr xy\nx\n.if 1 .cc !\n!if 1 !ds dynamic control-state\n!if 1 !cc .\n\\*[dynamic]\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["string-state", "y", "control-state"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn collected_scopes_reparse_nested_single_line_programs() {
    let name = SourceName::new("scope-inline-rerun.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.if 1 .if 1 nested-condition\n.nr count 2\n.while \\n[count] .if 1 .nr count -1\n.if !\\n[count] nested-while\n.\\}\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["nested-condition", "nested-while"]);
    assert!(!report.statistics.truncated);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn collected_scopes_let_selected_definitions_consume_following_lines() {
    let name = SourceName::new("scope-inline-definition.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.if 1 .de emit\ndefined in collected scope\n..\n.emit\n.\\}\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["defined in collected scope"]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn macro_frames_reparse_single_line_while_programs() {
    let name = SourceName::new("macro-inline-while.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de program\n.nr count 2\n.while \\n[count] .if 1 .nr count -1\n.if !\\n[count] macro-while\n..\n.program\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(text, ["macro-while"]);
    assert!(!report.statistics.truncated);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn m3_string_and_macro_defined_conditionals_accept_the_two_token_form() {
    let name = SourceName::new("defined-condition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie d phrase unexpected\n.el absent\n.ds phrase value\n.ie d phrase string\n.el unexpected\n.if !d phrase unexpected\n.de macro\nbody\n..\n.ie d macro macro\n.el unexpected\n.ie d PP builtin\n.el unexpected\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["absent", "string", "macro", "builtin"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn device_and_character_conditions_match_the_terminal_mandoc_profile() {
    let name = SourceName::new("device-character-condition.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if o old-device\n.if e hidden-even-device\n.if v hidden-v-device\n.if c A literal-character\n.if c \\[em] special-character\n.if c \\[u2717] unicode-character\n.if !c \\[not-a-character] unavailable-character\n.if 1 .ec @\n.if c @[em] custom-escape-character\n.if !c @[not-a-character] custom-escape-unavailable\n",
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
            "old-device",
            "literal-character",
            "special-character",
            "unicode-character",
            "unavailable-character",
            "custom-escape-character",
            "custom-escape-unavailable",
        ]
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn m3_delimited_string_conditions_handle_match_mismatch_and_malformed_input() {
    let name = SourceName::new("string-compare.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie \"\"\" empty\n.el unexpected\n.ie xabcxabcx equal\n.el unexpected\n.ie xabcxabdx unexpected\n.el mismatch\n.ie xabc unexpected\n.el malformed\n.ie !xabcxabcx unexpected\n.el negated\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["empty", "equal", "mismatch", "malformed", "negated"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_numeric_conditions_compare_physical_units_and_boolean_operators() {
    let name = SourceName::new("numeric-condition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie 42 positive\n.el unexpected\n.ie !42 unexpected\n.el negated\n.ie -42 unexpected\n.el negative\n.ie !-42 negated-negative\n.el unexpected\n.ie 42=bad unexpected\n.el incomplete\n.ie 1&1 both\n.el unexpected\n.ie 1&0 unexpected\n.el and-false\n.ie 0:1 either\n.el unexpected\n.ie 1i>2c physical\n.el unexpected\n.ie 1i-6P unexpected\n.el zero\n.ie ( unexpected\n.el bare-open\n.ie !( unexpected\n.el negated-bare-open\n.ie (1 open\n.el unexpected\n.ie !(0 negated-open\n.el unexpected\n",
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
        [
            "positive",
            "negated",
            "negative",
            "negated-negative",
            "incomplete",
            "both",
            "and-false",
            "either",
            "physical",
            "zero",
            "bare-open",
            "negated-bare-open",
            "open",
            "negated-open",
        ]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_multiline_conditional_scopes_use_the_explicit_execution_stack() {
    let name = SourceName::new("conditional-scope.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if n \\{\\\nouter\n.if t \\{\\\nhidden\n.\\}\n.if n \\{\\\ninner\n.\\}\n.\\}\n.if t \\{\\\nskipped\n.\\}\n.ie n \\{\\\ntrue-branch\n.\\}\n.el \\{\\\nwrong-branch\n.\\}\n.ie t \\{\\\nwrong-branch\n.\\}\n.el \\{\\\nelse-branch\n.\\}\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["outer", "inner", "true-branch", "else-branch"]);
    let outer = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    let position = report
        .document
        .source_position(outer.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (2, 9));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_multiline_while_scope_preserves_its_opener_column() {
    let name = SourceName::new("while-scope.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 1\n.while \\n[count] \\{\\\nbody\n.nr count 0\n.\\}\n",
        ))
        .unwrap();
    let node = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .next()
        .unwrap();
    assert_eq!(node.text(), Some("body"));
    let position = report
        .document
        .source_position(node.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (3, 20));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_continue_skips_to_the_nearest_explicit_loop_frame() {
    let name = SourceName::new("continue.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr remaining 3\n.while \\n[remaining] \\{\\\n.nr remaining -1\n.if \\n[remaining]=1 \\{\\\n.continue\n.\\}\nkept \\n[remaining]\n.\\}\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["kept 2", "kept 0"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_empty_ie_predicates_consume_their_next_line_before_selecting_else() {
    let name = SourceName::new("empty-ie.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ie\ntext-after-empty\n.el empty-else\n.ie !\ntext-after-negated-empty\n.el negated-empty-else\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["empty-else", "negated-empty-else"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_bare_ie_leaves_an_immediate_else_as_its_paired_branch() {
    let name = SourceName::new("bare-ie-else.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(&name, b".ie 0\n.el selected\n"))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["selected"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_conditional_text_preserves_literal_escape_before_a_brace() {
    let name = SourceName::new("ie-literal-brace.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ie n If \\&.el\\e{ works, nothing follows here:\n.el\\{dummy\nBOOHOO\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["If .el\\{ works, nothing follows here:"]);
}

#[test]
fn m3_conditional_scope_closes_after_a_control_request() {
    let name = SourceName::new("ie-control-closer.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".ie n \\{\\\nactive branch\n.br\\}\n.el \\{\\\ninactive branch\n.br\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["active branch"]);
}

#[cfg(feature = "render")]
#[test]
fn conditional_scope_closer_suffix_keeps_terminal_inline_provenance() {
    let name = SourceName::new("conditional-scope-suffix.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1 28-Aug-2026\n.SH DESCRIPTION\npreceding words\n.if n \\{text line block end\n\\} with additional words\nfollowing words\n",
            ))
            .unwrap();
    let suffix = report
        .document
        .preorder()
        .find(|node| {
            node.text()
                .is_some_and(|text| text.contains("additional words"))
        })
        .expect("scope-closer suffix must remain visible");
    assert!(suffix.terminal_inline_conditional());
}

#[test]
fn m3_control_scope_closer_discards_following_text() {
    let name = SourceName::new("control-scope-closer-suffix.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1 28-Aug-2026\n.SH DESCRIPTION\n.if n \\{\\\nfirst line\n.\\}suffix must not print\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["DESCRIPTION", "first line"]);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_nested_text_closers_remain_in_the_active_inner_scope() {
    let name = SourceName::new("nested-text-closers.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1 28-Aug-2026\n.SH DESCRIPTION\n.if n \\{outer\n.if n \\{inner\non\\} the\\} same\nafter\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        [
            "DESCRIPTION",
            "outer",
            "inner",
            "on\\& the\\& same",
            "after"
        ]
    );
}

#[test]
fn m3_attached_font_scope_closers_keep_font_arguments_and_diagnostic() {
    let name = SourceName::new("attached-font-closers.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL 1 28-Aug-2026\n.SH DESCRIPTION\n.if n \\{outer\n.if n \\{inner\n.BR\\}on\\}the same\nafter\n",
            ))
            .unwrap();
    assert_eq!(
        report
            .diagnostics
            .first()
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_ref())),
        Some((
            Severity::Error,
            "escaped character not allowed in a name: BR\\&"
        ))
    );
    let macro_node = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("BR"))
        .unwrap();
    assert_eq!(
        macro_node
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["on\\&the", "same"]
    );
}

#[test]
fn m3_unterminated_conditional_scope_reports_its_opener_and_executes_prefix() {
    let name = SourceName::new("unterminated-condition.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH CONDITIONAL 1 28-Aug-2026\n.SH DESCRIPTION\n.if n \\{\nstill open\n",
        ))
        .unwrap();
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::ROFF_UNTERMINATED_SCOPE)
        .unwrap();
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.message.as_ref(),
        "appending missing end of block: if"
    );
    assert!(
        report
            .document
            .preorder()
            .any(|node| node.text() == Some("still open"))
    );
}

#[test]
fn m3_nonstandard_brace_scopes_retain_the_same_line_body_at_every_depth() {
    let name = SourceName::new("nonstandard-brace-scopes.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\nouter\n.if 1 \\{inner\n\\}\n.\\}\n.nr count 1\n.while \\n[count] \\{first\n.nr count -1\n\\}\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["outer", "inner", "first"]);
}

#[test]
fn m3_nested_scope_closers_share_a_control_line_without_leaking_frames() {
    let name = SourceName::new("nested-control-closers.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{outer\n.if 1 \\{inner\n.\\}middle\\}end\nafter\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["outer", "inner", "after"]);
}

#[test]
fn m3_nested_ie_else_scopes_keep_the_eligible_branch_in_the_same_frame() {
    let name = SourceName::new("nested-ie-else-scopes.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.ie 0 \\{\\\ninactive\n.\\}\n.el \\{\\\nactive\n.\\}\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["active"]);
}

#[test]
fn m3_collected_scopes_define_direct_and_indirect_copy_mode_macros() {
    let name = SourceName::new("scope-copy-mode-definition.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".ds indirect appended\n.de direct\nfirst\n..\n.if 1 \\{\\\n.am direct\nsecond\n..\n.dei indirect\nthird\n..\n.de custom finish\ncustom marker\n.finish\n.\\}\n.direct\n.appended\n.custom\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["first", "second", "third", "custom marker"]);
}

#[test]
fn m3_conditional_macro_definitions_discard_terminator_tails_and_inactive_definitions() {
    let name = SourceName::new("conditional-definition-tails.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CONDITIONAL-DEFINITION 1 28-Aug-2026\n.SH DESCRIPTION\n.if n \\{.de first\nfirst content\n.. \\}\n.if n \\{.de second\nsecond content\n.. \\}ignored\n.if t \\{.de suppressed\nnot visible\n.. \\}ignored\ninitial text\n.first\n.second\n.suppressed\nfinal text\n",
            ))
            .unwrap();
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        [
            "DESCRIPTION",
            "initial text",
            "first content",
            "second content",
            "final text"
        ]
    );
    assert_eq!(report.diagnostics.len(), 2, "{:#?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::ROFF_ALL_ARGUMENTS
    );
    assert_eq!(
        report.diagnostics[1].code.as_str(),
        DiagnosticCode::ROFF_UNKNOWN_MACRO
    );
}

#[test]
fn m3_collected_scope_definitions_preserve_nested_ie_else_copy_mode() {
    let name = SourceName::new("scope-copy-mode-nested-ie.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".if 1 \\{\\\n.de emit\n.ie 0 \\{\\\nskipped\n.\\}\n.el \\{\\\nselected\n.\\}\n..\n.\\}\n.emit\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["selected"]);
}

#[test]
fn m3_inline_ie_else_inside_a_loop_scope_selects_only_the_eligible_body() {
    let name = SourceName::new("inline-ie-else-in-loop.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 1\n.while \\n[count] \\{\\\n.ie 0 skipped\n.el kept\n.nr count -1\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["kept"]);
}

#[test]
fn m3_inline_if_inside_a_loop_scope_dispatches_a_macro_body() {
    let name = SourceName::new("inline-if-macro-in-loop.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de emit\nfrom macro\n..\n.nr count 1\n.while \\n[count] \\{\\\n.if 1 .emit\n.nr count -1\n.\\}\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["from macro"]);
}

#[test]
fn m3_top_level_inline_if_dispatches_a_macro_body() {
    let name = SourceName::new("inline-if-macro.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".de emit\nfrom macro: \\$1\n..\n.if n .emit argument\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["from macro: argument"]);
}

#[test]
fn m3_inline_if_inside_a_loop_scope_dispatches_translation_requests() {
    let name = SourceName::new("inline-if-translation-in-loop.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 1\n.while \\n[count] \\{\\\n.if 1 .tr xy\nx\n.nr count -1\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["y"]);
}

#[test]
fn m3_collected_scopes_reclassify_requests_after_a_dynamic_control_change() {
    let name = SourceName::new("scope-dynamic-control.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.cc !\n!ds word dynamic\n!cc .\n\\*[word]\n.\\}\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["dynamic"]);
}

#[test]
fn m3_inactive_collected_scopes_do_not_leak_dynamic_control_changes() {
    let name = SourceName::new("inactive-scope-dynamic-control.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 0 \\{\\\n.cc !\n!ds word hidden\n.\\}\n.ds word outside\n\\*[word]\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["outside"]);
}

#[test]
fn m3_collected_scopes_close_with_a_delayed_escape_character() {
    let name = SourceName::new("scope-dynamic-escape.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 \\{\\\n.ec @\n@}\n.ds word after\n@*[word]\n",
        ))
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["after"]);
}

#[test]
fn m3_scope_macros_execute_their_own_while_brace_frames() {
    let name = SourceName::new("scope-macro-while.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de emit\n.nr count 1\n.while \\n[count] \\{\\\ninside\n.nr count -1\n.\\}\n..\n.if 1 \\{\\\n.emit\n.\\}\n",
            ))
            .unwrap();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["inside"]);
}

#[test]
fn m3_scope_macro_while_frames_share_the_session_loop_budget() {
    let name = SourceName::new("scope-macro-while-limit.roff").unwrap();
    let report = Parser::new(ParserConfig {
            limits: Limits {
                max_loop_iterations: 2,
                max_total_loop_iterations: 2,
                ..Limits::default()
            },
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".de emit\n.nr count 3\n.while \\n[count] \\{\\\ninside\n.nr count -1\n.\\}\n..\n.if 1 \\{\\\n.emit\n.\\}\n",
        ))
        .unwrap();
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.loop-iterations")
    );
    let visible = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert_eq!(visible, ["inside", "inside"]);
}
