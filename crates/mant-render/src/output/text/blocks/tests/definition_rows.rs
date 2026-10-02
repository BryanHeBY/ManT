use super::*;

#[test]
fn distinct_term_roots_and_hard_lines_do_not_acquire_commas() {
    let blocks = [Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            terms: ["-a", "--all"]
                .map(|value| {
                    vec![Inline::Text {
                        value: value.into(),
                    }]
                })
                .into(),
            description: vec![paragraph("FIRST\nCONTINUATION", 0)],
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                head_body_relation: mant_ir::HeadBodyRelation::from(true),
                ..Default::default()
            },
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    assert_eq!(
        super::super::super::plain_renderer().render_blocks(&blocks, 0),
        "-a\n--all FIRST\n    CONTINUATION"
    );
}

#[test]
fn zero_width_terms_and_clipped_markers_do_not_move_body_text() {
    let renderer = super::super::super::plain_renderer();
    let mut block = plain_list(vec![paragraph("BODY", 4)], -5);
    let Block::List { kind, .. } = &mut block else {
        unreachable!()
    };
    *kind = ListKind::Bullet;
    // Pinned mdoc_term.c::termp_it_pre uses a bullet glyph for Bl -bullet;
    // its UTF-8 spelling was checked with the one-item list probe.
    assert_eq!(renderer.render_blocks(&[block], 0), "•\n BODY");
    let block = Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            source: None,
            entry: None,
            terms: vec![
                vec![Inline::anchor_at("target", None)],
                vec![Inline::Text {
                    value: "TERM".into(),
                }],
            ],
            description: vec![paragraph("BODY", 0)],
            layout: mant_ir::DefinitionLayout {
                head_body_relation: mant_ir::HeadBodyRelation::from(true),
                ..Default::default()
            },
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(renderer.render_blocks(&[block], 0), "TERM BODY");
}

#[test]
fn empty_head_prefix_rows_preserve_the_recorded_run_in_body_origin() {
    // Exact .Bl -hang -width 12n / .No \\z / .sp 1 or 2 / .An -split /
    // .An Bob / .No BODY ran pristine before this assertion: the empty
    // endline is real, but did not print or wrap the label (term.c:489-497).
    // Its accepted "ob" therefore retains BODY column 14, not only gap 1.
    for (prefix, prefix_rows, expected_gap) in
        [("", 1, 12), ("", 2, 12), ("HEAD", 1, 1), (" ", 1, 1)]
    {
        let mut head = vec![Inline::anchor("empty-head-origin")];
        head.push(Inline::Strong {
            children: vec![Inline::Text {
                value: prefix.into(),
            }],
        });
        head.extend((0..prefix_rows).map(|_| Inline::line_break()));
        head.push(Inline::Text { value: "ob".into() });
        let block = Block::DefinitionList {
            declaration_groups: vec![],
            compact: true,
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                terms: vec![head],
                description: vec![paragraph("BODY", 0)],
                layout: mant_ir::DefinitionLayout {
                    head_body_relation: mant_ir::HeadBodyRelation::separated(
                        if prefix.is_empty() {
                            mant_ir::DefinitionBodyAlignment::Indented
                        } else {
                            mant_ir::DefinitionBodyAlignment::AfterTerm
                        },
                    ),
                    body_indent_columns: 14,
                    min_term_gap_columns: 1,
                    spacing_before_lines: None,
                },
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        for decorated in [false, true] {
            let paint = |_: TextPresentation, value: &str| {
                if decorated {
                    format!("\x1b[1m{value}\x1b[0m")
                } else {
                    value.into()
                }
            };
            let renderer = BlockRenderer {
                names: None,
                locations: None,
                decorate: &paint,
            };
            let output = renderer
                .render_blocks(std::slice::from_ref(&block), 0)
                .replace("\x1b[1m", "")
                .replace("\x1b[0m", "");
            assert_eq!(
                output,
                format!(
                    "{prefix}{}ob{}BODY",
                    "\n".repeat(prefix_rows),
                    " ".repeat(expected_gap)
                )
            );
        }
    }
}
