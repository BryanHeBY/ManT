use super::*;

#[test]
fn m3_resolved_includes_preserve_order_source_maps_and_session_state() {
    let root = SourceName::new("root.roff").unwrap();
    let mut bundle = SourceBundle::default();
    bundle
        .insert(
            "part.roff",
            b"inside \\*[word]\n.ds word changed\n".to_vec(),
        )
        .unwrap();
    let report = Parser::default()
        .parse_with_resolver(
            Source::new(
                &root,
                b".ds word welcome\n.so part.roff\noutside \\*[word]\n",
            ),
            &mut bundle,
        )
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].text(), Some("inside welcome"));
    assert_eq!(nodes[1].text(), Some("outside changed"));
    assert_eq!(report.document.source_count(), 2);
    let child_span = nodes[0].location().unwrap();
    assert_eq!(
        report
            .document
            .source_name(child_span.source)
            .map(SourceName::as_str),
        Some("part.roff")
    );
    assert_eq!(report.statistics.source_files, 2);
    assert_eq!(
        report.statistics.source_bytes,
        b".ds word welcome\n.so part.roff\noutside \\*[word]\n".len()
            + b"inside \\*[word]\n.ds word changed\n".len()
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_include_cycles_and_missing_targets_are_recoverable() {
    let root = SourceName::new("root.roff").unwrap();
    let mut bundle = SourceBundle::default();
    bundle.insert("root.roff", b"ignored".to_vec()).unwrap();
    bundle
        .insert("part.roff", b".so root.roff\n".to_vec())
        .unwrap();
    let cyclic = Parser::default()
        .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
        .unwrap();
    assert_eq!(cyclic.document.source_count(), 2);
    assert!(cyclic.statistics.truncated);
    assert!(
        cyclic
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "roff.include-cycle")
    );

    let missing = Parser::default()
        .parse(Source::new(&root, b".so missing.roff\n"))
        .unwrap();
    assert_eq!(missing.document.node_count(), 1);
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "roff.include-unavailable")
    );
}

#[test]
fn m3_include_graph_limits_stop_before_source_map_mutation() {
    let root = SourceName::new("root.roff").unwrap();
    let limits = Limits {
        max_sources: 1,
        ..Limits::default()
    };
    let mut bundle = SourceBundle::new(limits.clone());
    bundle.insert("part.roff", b"child\n".to_vec()).unwrap();
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
    .unwrap();
    assert_eq!(report.document.source_count(), 1);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.sources")
    );
}

#[test]
fn m3_include_depth_and_child_source_bounds_are_diagnostic_not_fatal() {
    let root = SourceName::new("root.roff").unwrap();
    let mut bundle = SourceBundle::default();
    bundle
        .insert("first.roff", b".so second.roff\n".to_vec())
        .unwrap();
    bundle.insert("second.roff", b"second\n".to_vec()).unwrap();
    let depth_limited = Parser::new(ParserConfig {
        limits: Limits {
            max_include_depth: 1,
            ..Limits::default()
        },
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so first.roff\n"), &mut bundle)
    .unwrap();
    assert_eq!(depth_limited.document.source_count(), 2);
    assert!(
        depth_limited
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.include-depth")
    );

    let mut bytes_bundle = SourceBundle::default();
    bytes_bundle
        .insert("large.roff", b"this child is too large\n".to_vec())
        .unwrap();
    let byte_limited = Parser::new(ParserConfig {
        limits: Limits {
            max_root_source_bytes: 16,
            max_total_source_bytes: 64,
            ..Limits::default()
        },
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so large.roff\n"), &mut bytes_bundle)
    .unwrap();
    assert_eq!(byte_limited.document.source_count(), 1);
    assert!(
        byte_limited
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.source-bytes")
    );
}

#[test]
fn m3_include_diagnostics_share_the_session_budget() {
    let root = SourceName::new("root.roff").unwrap();
    let limits = Limits {
        max_diagnostics: 1,
        ..Limits::default()
    };
    let mut bundle = SourceBundle::new(limits.clone());
    bundle
        .insert("part.roff", b".so missing-a\n.so missing-b\n".to_vec())
        .unwrap();
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse_with_resolver(Source::new(&root, b".so part.roff\n"), &mut bundle)
    .unwrap();
    assert_eq!(report.document.source_count(), 2);
    assert_eq!(report.diagnostics.len(), 1);
    assert!(report.statistics.truncated);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        "roff.include-unavailable"
    );
}

#[test]
fn m3_while_rechecks_register_conditions_and_updates_session_state() {
    let name = SourceName::new("while.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr count 0\n.while \\n[count]<3 .nr count +1\ncount \\n[count]\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("count 3"));
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_while_executes_a_copy_mode_macro_body_on_each_iteration() {
    let name = SourceName::new("while-macro.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 2 1\n.de decrement\n\\\\n-[count]\n..\n.while \\n[count] .decrement\ncount \\n[count]\n",
            ))
            .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["1", "0", "count 0"]);
    assert!(report.diagnostics.is_empty());
    assert!(!report.statistics.truncated);
}

#[test]
fn m3_active_inline_conditionals_execute_environment_requests() {
    let name = SourceName::new("conditional-request.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".if 1 .ds selected yes\n.if 0 .ds selected no\n\\*[selected]\n",
        ))
        .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text(), Some("yes"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn m3_while_aggregate_limit_stops_environment_updates() {
    let name = SourceName::new("while-limit.roff").unwrap();
    let limits = Limits {
        max_loop_iterations: 2,
        max_total_loop_iterations: 3,
        ..Limits::default()
    };
    let report = Parser::new(ParserConfig {
            limits,
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".nr first 0\n.while \\n[first]<2 .nr first +1\n.nr second 0\n.while \\n[second]<2 .nr second +1\n",
        ))
        .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(text.is_empty());
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.total-loop-iterations")
    );
}

#[test]
fn m3_while_per_loop_limit_returns_the_generated_prefix() {
    let name = SourceName::new("while-per-loop-limit.roff").unwrap();
    let limits = Limits {
        max_loop_iterations: 2,
        max_total_loop_iterations: 3,
        ..Limits::default()
    };
    let report = Parser::new(ParserConfig {
        limits,
        ..ParserConfig::default()
    })
    .parse(Source::new(&name, b".while 1 repeated\n"))
    .unwrap();
    let text = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(text, ["repeated", "repeated"]);
    assert!(report.statistics.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_str() == "limits.loop-iterations")
    );
}

#[test]
fn m3_numeric_and_nroff_conditionals_choose_only_the_active_inline_branch() {
    let name = SourceName::new("conditionals.roff").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".nr count 7\n.if 1 visible\n.if 0 hidden\n.if !0 inverted\n.if n nroff\n.if t troff\n.if \\n[count]>=7 registered\n.if \\n[count]!=7 wrong\n.ie 0 first\n.el second\n",
            ))
            .unwrap();
    let nodes = report
        .document
        .node(report.document.root())
        .unwrap()
        .children()
        .map(|node| node.text().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        nodes,
        ["visible", "inverted", "nroff", "registered", "second"]
    );
    assert!(report.diagnostics.is_empty());
}

#[test]
fn number_registers_accept_whitespace_inside_parenthesized_values() {
    let name = SourceName::new("register-parenthesized-space.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr value 18\n.nr value ( 25 - 6 )\n\\n[value]\n",
        ))
        .unwrap();
    let text = report
        .document
        .preorder()
        .find_map(|node| node.text().map(str::to_owned));
    assert_eq!(text.as_deref(), Some("19"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn number_register_division_by_zero_recovers_to_zero_and_reports_the_request() {
    let name = SourceName::new("division-by-zero.roff").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".nr quotient 1/0\n.nr remainder 1%0\n\\n[quotient] \\n[remainder]\n",
        ))
        .unwrap();
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.severity,
                    diagnostic.message.as_ref(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                Severity::Error,
                "divide by zero: 1/0",
            ),
            (
                DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                Severity::Error,
                "divide by zero: 1%0",
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
    assert_eq!(positions, [(1, 4), (2, 4)]);
    assert_eq!(
        report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>(),
        ["0 0"]
    );
    assert!(!report.statistics.truncated);
}
