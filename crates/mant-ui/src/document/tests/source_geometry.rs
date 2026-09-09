//! Source-derived geometry must agree across the two production frontends.
use super::*;
use std::fmt::Write;

fn column(text: &str, token: &str) -> usize {
    text.lines()
        .find_map(|line| line.find(token))
        .unwrap_or_else(|| panic!("missing {token}: {text}"))
}

#[test]
fn relative_definition_geometry_matches_text_and_all_normal_viewports() {
    // Original minimal shape of btrfs-subvolume's nested INDENT/TP regions.
    // mandoc CVS HEAD 1.250 and groff 1.24.1 agree on these relative columns.
    let query = mant_engine::query_roff_bytes(b".TH GEOMETRY 1\n.SH SUBCOMMAND\n.RS 0\n.TP\n.B create\nCREATE_BODY\n.sp\nOPTIONS_BODY\n.RS 7\n.TP\n.B -i QGROUP\nINNER_BODY\n.RE\n.TP\n.B delete\nDELETE_BODY\n.RE\n").unwrap();
    let before = query.clone();
    let text = mant_engine::render_query_text(&query);
    let expected = [
        ("create", 0),
        ("CREATE_BODY", 7),
        ("OPTIONS_BODY", 7),
        ("-i QGROUP", 7),
        ("INNER_BODY", 14),
        ("delete", 0),
        ("DELETE_BODY", 7),
    ];
    for (token, origin) in expected {
        assert_eq!(column(&text, token), origin, "{text}");
    }
    let view = DocumentView::new(&query);
    for width in [40, 80, 120] {
        let rendered = view.render(width);
        let text = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        for (token, origin) in expected {
            assert_eq!(column(&text, token), origin + 3, "width {width}: {text}");
            // Case-insensitive search also sees the CREATE_/DELETE_ body
            // witness, independently of the visible label's column.
            assert_eq!(
                rendered.search(token).len(),
                if matches!(token, "create" | "delete") {
                    2
                } else {
                    1
                }
            );
        }
    }
    assert_eq!(query, before);
}

#[test]
fn markdown_semantic_annotation_does_not_change_translated_content_geometry() {
    for loose in [false, true] {
        let separator = if loose { "\n\n" } else { "\n" };
        let plain = format!(
            "# Demo\n\n## Commands\n\n- `run`: First.{separator}- `stop`: Second.\n\n  ```text\n  CODE\n  ```\n"
        );
        let annotated = plain.replace("- `run`", "<!-- mant:entries role=command -->\n- `run`");
        let plain = mant_engine::query_markdown_text(&plain, None).unwrap();
        let annotated = mant_engine::query_markdown_text(&annotated, None).unwrap();
        assert_eq!(
            mant_engine::render_query_text(&plain),
            mant_engine::render_query_text(&annotated)
        );
        for query in [&plain, &annotated] {
            let blocks = &query.document.as_ref().unwrap().sections[0].blocks;
            let mut baseline: Option<Vec<(usize, String)>> = None;
            for shift in [0, 2, 5] {
                let mut builder = DocumentBuilder::new("translation".into(), None);
                builder.blocks(blocks, shift);
                let rows = builder
                    .lines
                    .iter()
                    .map(|line| {
                        (
                            line.indent,
                            line.spans
                                .iter()
                                .map(|span| span.content.as_ref())
                                .collect::<String>(),
                        )
                    })
                    .collect::<Vec<_>>();
                if let Some(original) = &baseline {
                    let expected: Vec<_> = original
                        .iter()
                        .map(|(indent, text): &(usize, String)| {
                            (
                                if text.is_empty() {
                                    *indent
                                } else {
                                    *indent + usize::try_from(shift).unwrap()
                                },
                                text.clone(),
                            )
                        })
                        .collect();
                    assert_eq!(rows, expected);
                } else {
                    baseline = Some(rows);
                }
            }
        }
    }
}

#[test]
fn tq_run_in_uses_only_the_final_label_and_preserves_one_source_owner() {
    // CVS HEAD man_term.c pre_TP/post_TP flush each head independently;
    // groff's TQ enters TP after a break as well. Completed heads do not fit
    // against the last head's body column.
    for labels in [
        vec!["--long-first-label", "-b"],
        vec!["-a", "--long-last-label"],
        vec!["--long-first-label", "--long-last-label"],
        vec!["-a", "-b"],
        vec!["--long-first-label", "--another-long-label", "-c", "-d"],
        vec!["--long-first-label", "日本"],
    ] {
        let mut source = format!(".TH PROBE 1\n.SH OPTIONS\n.TP 7\n.B {}\n", labels[0]);
        for label in &labels[1..] {
            writeln!(source, ".TQ\n.B {label}").unwrap();
        }
        source.push_str("BODY\n.br\nTAIL\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        let items = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::DefinitionList { items, .. } => Some(items),
                _ => None,
            })
            .unwrap();
        assert_eq!(items.len(), 1, "{source}");
        assert_eq!(items[0].terms.len(), labels.len(), "{source}");
        let last = labels.last().unwrap();
        let runs_in = last.width() <= 6;
        assert_eq!(items[0].layout.inline_term, runs_in, "{source}");
        if labels.iter().all(|label| label.starts_with('-')) {
            let facts = items[0].entry.as_ref().unwrap();
            assert_eq!(facts.names, labels, "{source}");
            for name in &labels {
                let excerpt = mant_engine::select_excerpt(
                    &query,
                    &[mant_protocol::ContentSelector::id(facts.id.clone())],
                )
                .unwrap();
                let text = mant_engine::render_excerpt_text(&excerpt);
                let body = text.lines().find(|line| line.contains("BODY")).unwrap();
                assert_eq!(body.contains(last), runs_in, "{source}\n{text}");
                let explained = mant_engine::explain_query(
                    &query,
                    &mant_protocol::ExplanationQuery {
                        entry: (*name).into(),
                        options: mant_protocol::ExplanationOptions::default(),
                    },
                )
                .unwrap();
                assert_eq!(explained.counts.direct_entry.total, 1);
                let text = mant_engine::render_explanation_text(&explained);
                assert!(text.contains("BODY"), "{text}");
                assert!(text.contains("TAIL"), "{text}");
            }
        }
        let before = query.clone();
        for (text, origin) in std::iter::once((mant_engine::render_query_text(&query), 0)).chain(
            [40, 80, 120].map(|width| {
                (
                    DocumentView::new(&query)
                        .render(width)
                        .text
                        .lines
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    3,
                )
            }),
        ) {
            let body = text.lines().find(|line| line.contains("BODY")).unwrap();
            assert_eq!(body.contains(last), runs_in, "{source}\n{text}");
            assert_eq!(
                body.split("BODY").next().unwrap().width(),
                origin + 7,
                "{text}"
            );
            assert_eq!(column(&text, "TAIL"), origin + 7, "{text}");
            for label in &labels[..labels.len() - 1] {
                assert!(text.lines().any(|line| line.trim() == *label), "{text}");
            }
        }
        assert_eq!(query, before);
    }
}

#[test]
fn nested_literal_display_origins_and_targets_survive_tui_lowering() {
    // CVS HEAD mdoc_term.c saves/restores the display offset around children;
    // D1/Dl add six cells even when the parent already runs a literal stream.
    for (inner, origin) in [
        (".Bd -literal -offset 3n\nBETA\n.Ed", 5),
        (".Bd -unfilled -offset 3n\nBETA\n.Ed", 5),
        (".Bd -literal -compact -offset 3n\nBETA\n.Ed", 5),
        (".D1 BETA", 8),
        (".Dl BETA", 8),
    ] {
        let source = format!(
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\nBASE\n.Tg outer-target\n.Bd -literal -offset 2n\nALPHA\n.Tg inner-target\n{inner}\nGAMMA\n.Ed\nAFTER\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let before = query.clone();
        let blocks = &query.document.as_ref().unwrap().sections[0].blocks;
        let mut builder = DocumentBuilder::new("probe".into(), None);
        builder.blocks(blocks, 0);
        for (target, witness) in [("outer-target", "ALPHA"), ("inner-target", "BETA")] {
            let row = *builder
                .anchors
                .get(target)
                .unwrap_or_else(|| panic!("missing {target}: {source}\n{:?}", builder.anchors));
            let line = builder.lines[row]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            assert!(
                line.contains(witness),
                "{target} points at {line:?}: {source}"
            );
        }
        for width in [40, 80, 120] {
            let rendered = DocumentView::new(&query).render(width);
            let text = rendered
                .text
                .lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            let base = column(&text, "BASE");
            for (token, delta) in [("ALPHA", 2), ("BETA", origin), ("GAMMA", 2), ("AFTER", 0)] {
                assert_eq!(column(&text, token), base + delta, "{source}\n{text}");
                assert_eq!(rendered.search(token).len(), 1);
            }
        }
        assert_eq!(query, before);
    }
}

fn literal_source(mode: &str, body: &str) -> String {
    let (header, open, close) = match mode {
        "nf" => (".TH PROBE 1\n.SH TEST", ".nf", ".fi"),
        "EX" => (".TH PROBE 1\n.SH TEST", ".EX", ".EE"),
        "literal" => (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST",
            ".Bd -literal -compact",
            ".Ed",
        ),
        "unfilled" => (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST",
            ".Bd -unfilled -compact",
            ".Ed",
        ),
        _ => unreachable!(),
    };
    format!("{header}\nBEFORE\n{open}\n{body}{close}\nAFTER\n")
}

fn tui_blank_rows(source: &str, first: &str, second: &str) -> usize {
    let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
    let rendered = DocumentView::new(&query).render(80);
    let rows: Vec<_> = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect();
    let first = rows.iter().position(|row| row.contains(first)).unwrap();
    let second = rows.iter().position(|row| row.contains(second)).unwrap();
    assert!(
        rows[first + 1..second]
            .iter()
            .all(|row| row.trim().is_empty()),
        "{source}\n{rows:?}"
    );
    second - first - 1
}

#[test]
fn all_blank_literal_rows_are_not_confused_with_zero_width_targets() {
    for mode in ["nf", "EX", "literal", "unfilled"] {
        for rows in 1..=3 {
            let source = literal_source(mode, &"\n".repeat(rows));
            assert_eq!(tui_blank_rows(&source, "BEFORE", "AFTER"), rows, "{source}");
        }
    }
    for (nodes, expected_rows) in [
        (Vec::new(), 0),
        (vec![Inline::anchor("target")], 0),
        (
            vec![Inline::Strong {
                children: vec![Inline::anchor("target")],
            }],
            0,
        ),
        (
            vec![Inline::Text {
                value: String::new(),
            }],
            1,
        ),
        (
            vec![Inline::Code {
                value: String::new(),
            }],
            1,
        ),
        (
            vec![Inline::Strong {
                children: vec![Inline::Text {
                    value: String::new(),
                }],
            }],
            1,
        ),
    ] {
        let mut builder = DocumentBuilder::new("literal".into(), None);
        builder.inline_lines_with_surface(&nodes, 0, Style::default(), LineSurface::Code);
        assert_eq!(builder.lines.len(), expected_rows, "{nodes:?}");
        builder.inline_lines(
            &[Inline::Text {
                value: "AFTER".into(),
            }],
            0,
            Style::default(),
        );
        if let Some(row) = builder.anchors.get("target") {
            assert_eq!(*row, expected_rows, "{nodes:?}");
        }
    }
}

#[test]
fn tui_literal_gap_requests_use_executed_events_without_phantom_rows() {
    for mode in ["nf", "EX", "literal", "unfilled"] {
        for (requests, expected_rows) in [
            (".sp", 1),
            (".sp 0", 0),
            (".sp 1", 1),
            (".sp 2", 2),
            (".sp 1\n.sp 2", 3),
            (".if 1 .sp 2", 2),
            (".if 0 \\{\\\n.sp 2\n.\\}", 0),
            (".de UNUSED\n.sp 2\n..", 0),
            (".ig END\n.sp 2\n.END", 0),
        ] {
            let source = literal_source(mode, &format!("ALPHA\n{requests}\nBETA\n"));
            assert_eq!(
                tui_blank_rows(&source, "ALPHA", "BETA"),
                expected_rows,
                "{source}"
            );
        }
    }
}
