//! Original mdoc probes for display scope, not portable-source endorsements.
//! mandoc CVS HEAD `termp_bd_pre` applies offsets at each body; its node walker
//! restores the parent offset at exit. Nested Bd is supported with a native
//! portability warning, independently of its source geometry.

use mant_engine::{query_roff_bytes, render_query_text};
use mant_ir::{
    Block, Inline,
    visit::{self, Visit},
};

fn query(body: &str) -> mant_engine::ResolvedContent {
    let source = format!(
        ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd exercise display scopes\n.Sh DESCRIPTION\nBASE\n{body}\nAFTER\n"
    );
    query_roff_bytes(source.as_bytes()).unwrap()
}

fn assert_column(text: &str, token: &str, column: usize) {
    let line = text
        .lines()
        .find(|line| line.trim() == token)
        .unwrap_or_else(|| {
            panic!("missing {token}: {text}");
        });
    assert_eq!(line, format!("{}{token}", " ".repeat(column)), "{text}");
}

fn visible(children: &[Inline]) -> String {
    struct Text(String);
    impl<'ir> Visit<'ir> for Text {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Text { value } | Inline::Code { value } => self.0.push_str(value),
                Inline::LineBreak => self.0.push('\n'),
                _ => visit::walk_inline(self, inline),
            }
        }
    }
    let mut text = Text(String::new());
    for inline in children {
        text.visit_inline(inline);
    }
    text.0
}

#[test]
fn nested_display_offsets_compose_and_restore_for_each_mode() {
    for outer in ["filled", "literal", "unfilled"] {
        for inner in ["filled", "literal", "unfilled"] {
            for (outer_offset, inner_offset) in [(0, 0), (2, 0), (0, 3), (2, 3)] {
                let content = query(&format!(
                    ".Bd -{outer} -offset {outer_offset}n\nALPHA\n.Bd -{inner} -offset {inner_offset}n\nBETA\n.Ed\nGAMMA\n.Ed"
                ));
                let text = render_query_text(&content);
                for (token, column) in [
                    ("BASE", 0),
                    ("ALPHA", outer_offset),
                    ("BETA", outer_offset + inner_offset),
                    ("GAMMA", outer_offset),
                    ("AFTER", 0),
                ] {
                    assert_column(&text, token, column);
                }
                let document = content.document.as_ref().unwrap();
                assert!(mant_ir::validate_document(document).is_empty());
                assert!(document.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains("nested displays are not portable")
                }));
                let blocks = &document.sections[1].blocks;
                let beta = blocks
                    .iter()
                    .find(|block| match block {
                        Block::Paragraph { children, .. }
                        | Block::Preformatted { children, .. } => visible(children) == "BETA",
                        _ => false,
                    })
                    .unwrap();
                let (Block::Paragraph { layout, source, .. }
                | Block::Preformatted { layout, source, .. }) = beta
                else {
                    unreachable!()
                };
                assert_eq!(
                    layout.indent_columns,
                    i32::try_from(outer_offset + inner_offset).unwrap()
                );
                assert!(source.is_some());
                assert_eq!(matches!(beta, Block::Paragraph { .. }), inner == "filled");
            }
        }
    }
}

#[test]
fn nested_single_line_displays_keep_their_own_six_column_offset() {
    for outer in ["filled", "literal", "unfilled"] {
        for inner in ["D1", "Dl"] {
            let content = query(&format!(
                ".Bd -{outer} -offset 2n\nALPHA\n.{inner} BETA\nGAMMA\n.Ed"
            ));
            let text = render_query_text(&content);
            for (token, column) in [("ALPHA", 2), ("BETA", 8), ("GAMMA", 2), ("AFTER", 0)] {
                assert_column(&text, token, column);
            }
        }
    }
}

#[test]
fn compactness_controls_only_the_nested_display_leading_gap() {
    for outer in ["literal", "unfilled"] {
        for compact in [false, true] {
            let flag = if compact { " -compact" } else { "" };
            let content = query(&format!(
                ".Bd -{outer} -offset 2n\nALPHA\n.Bd -literal -offset 3n{flag}\nBETA\n.Ed\nGAMMA\n.Ed"
            ));
            let text = render_query_text(&content);
            let gap = if compact { "\n" } else { "\n\n" };
            assert!(
                text.contains(&format!("ALPHA{gap}     BETA\n  GAMMA")),
                "{text}"
            );
        }
    }
}

#[test]
fn nested_display_targets_remain_on_the_inner_body_and_fonts_survive_boundaries() {
    struct Evidence {
        emphasis: bool,
        words: Vec<(String, bool)>,
        targets: Vec<(String, bool)>,
        in_beta: bool,
    }
    impl<'ir> Visit<'ir> for Evidence {
        fn visit_block(&mut self, block: &'ir Block) {
            let previous = self.in_beta;
            self.in_beta = match block {
                Block::Preformatted { children, .. } | Block::Paragraph { children, .. } => {
                    visible(children) == "BETA"
                }
                _ => false,
            };
            visit::walk_block(self, block);
            self.in_beta = previous;
        }
        fn visit_inline(&mut self, inline: &'ir Inline) {
            let previous = self.emphasis;
            match inline {
                Inline::Emphasis { .. } => self.emphasis = true,
                Inline::Text { value } => self.words.push((value.clone(), self.emphasis)),
                Inline::Anchor {
                    fragment_aliases,
                    owner_source,
                    ..
                } => {
                    for alias in fragment_aliases {
                        if alias.as_str() == "Inner.Target" {
                            assert!(owner_source.is_some());
                            self.targets.push((alias.to_string(), self.in_beta));
                        }
                    }
                }
                _ => {}
            }
            visit::walk_inline(self, inline);
            self.emphasis = previous;
        }
    }
    let content = query(
        ".Bd -literal -offset 2n\n.Bf -emphasis\nALPHA\n.Tg Inner.Target\n.Bd -literal -offset 3n\nBETA\n.Ed\nGAMMA\n.Ef\n.Ed",
    );
    let mut evidence = Evidence {
        emphasis: false,
        words: Vec::new(),
        targets: Vec::new(),
        in_beta: false,
    };
    evidence.visit_document(content.document.as_ref().unwrap());
    assert_eq!(evidence.targets, vec![("Inner.Target".into(), true)]);
    for token in ["ALPHA", "BETA", "GAMMA"] {
        assert!(
            evidence
                .words
                .iter()
                .any(|(word, emphasis)| word == token && *emphasis),
            "{token}: {:?}",
            evidence.words
        );
    }
    assert!(
        evidence
            .words
            .iter()
            .any(|(word, emphasis)| word == "AFTER" && !emphasis)
    );
}

#[test]
fn nested_display_does_not_absorb_adjacent_list_or_table_geometry() {
    for payload in [
        ".Bl -bullet -compact\n.It\nLISTWORD\n.El",
        ".TS\nl l.\nTABLEWORD\tCELLWORD\n.TE",
    ] {
        for before in [false, true] {
            let display = ".Bd -literal -offset 3n -compact\nBETA\n.Ed";
            let inner = if before {
                format!("{payload}\n{display}")
            } else {
                format!("{display}\n{payload}")
            };
            let content = query(&format!(
                ".Bd -literal -offset 2n\nALPHA\n{inner}\nGAMMA\n.Ed"
            ));
            let text = render_query_text(&content);
            for (token, column) in [("ALPHA", 2), ("BETA", 5), ("GAMMA", 2), ("AFTER", 0)] {
                assert_column(&text, token, column);
            }
            let blocks = &content.document.as_ref().unwrap().sections[1].blocks;
            if payload.starts_with(".Bl") {
                assert!(blocks.iter().any(|block| matches!(block, Block::List { layout, .. } if layout.indent_columns == 2)));
                assert!(text.contains("LISTWORD"));
            } else {
                assert!(blocks.iter().any(|block| matches!(block, Block::Table { layout, .. } if layout.indent_columns == 2)));
                assert!(text.contains("TABLEWORD") && text.contains("CELLWORD"));
            }
        }
    }
}

#[test]
fn first_child_displays_inherit_only_real_predecessors_across_parent_scopes() {
    // mandoc's print_bvspace walks through first-child Bd/Bl wrappers. A
    // preceding BASE makes both default Bd gaps observable; directly after
    // Sh neither scope creates a gap. groff agrees for these combinations.
    for outer in ["filled", "literal", "unfilled"] {
        for inner in ["filled", "literal", "unfilled"] {
            for predecessor in [false, true] {
                for compact in [false, true] {
                    let before = if predecessor { "BASE\n" } else { "" };
                    let flag = if compact { " -compact" } else { "" };
                    let source = format!(
                        ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n{before}.Bd -{outer}{flag}\n.Bd -{inner}\nINNER\n.Ed\n.Ed\nAFTER\n"
                    );
                    let content = query_roff_bytes(source.as_bytes()).unwrap();
                    let text = render_query_text(&content);
                    let expected_gap = u16::from(predecessor) * (2 - u16::from(compact));
                    if predecessor {
                        assert!(
                            text.contains(&format!(
                                "BASE{}INNER\nAFTER",
                                "\n".repeat(usize::from(expected_gap) + 1)
                            )),
                            "{source}\n{text}"
                        );
                    }
                    let block = content.document.as_ref().unwrap().sections[0]
                        .blocks
                        .iter()
                        .find(|block| match block {
                            Block::Paragraph { children, .. }
                            | Block::Preformatted { children, .. } => visible(children) == "INNER",
                            _ => false,
                        })
                        .unwrap();
                    let spacing = match block {
                        Block::Paragraph { layout, .. } | Block::Preformatted { layout, .. } => {
                            layout.spacing_before_lines
                        }
                        _ => unreachable!(),
                    };
                    assert_eq!(spacing, expected_gap, "{source}\n{text}");
                }
            }
        }
    }
}

#[test]
fn empty_displays_preserve_their_independent_requests_without_visible_leaves() {
    for outer in ["filled", "literal"] {
        for empty in ["filled", "literal", "unfilled"] {
            for predecessor in [false, true] {
                for compact in [false, true] {
                    let before = if predecessor { "BASE\n" } else { "" };
                    let flag = if compact { " -compact" } else { "" };
                    let source = format!(
                        ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n{before}.Bd -{outer}\n.Bd -{empty}{flag}\n.Ed\n.Bd -literal\nINNER\n.Ed\n.Ed\nAFTER\n"
                    );
                    let content = query_roff_bytes(source.as_bytes()).unwrap();
                    let text = render_query_text(&content);
                    // The second inner Bd has an earlier sibling even when
                    // that empty sibling generated no printable content.
                    let gaps = if predecessor {
                        3 - usize::from(compact)
                    } else {
                        1
                    };
                    let start = if predecessor { "BASE" } else { "TEST" };
                    assert!(
                        text.contains(&format!("{start}{}INNER\nAFTER", "\n".repeat(gaps + 1))),
                        "{source}\n{text}"
                    );
                }
            }
        }
    }
}

#[test]
fn first_item_display_boundaries_follow_the_native_list_kind() {
    struct Gap(Option<u16>);
    impl<'a> Visit<'a> for Gap {
        fn visit_block(&mut self, block: &'a Block) {
            if let Block::Preformatted {
                children, layout, ..
            } = block
                && visible(children) == "INNER"
            {
                self.0 = Some(layout.spacing_before_lines);
            }
            visit::walk_block(self, block);
        }
    }
    for list in [
        "-item",
        "-bullet",
        "-enum",
        "-tag -width 8n",
        "-hang -width 8n",
        "-diag",
        "-inset",
        "-ohang",
        "-column X",
    ] {
        for predecessor in [false, true] {
            let before = if predecessor { "BASE\n" } else { "" };
            let head = if ["-tag", "-hang", "-diag", "-inset", "-ohang"]
                .iter()
                .any(|style| list.starts_with(style))
            {
                " TERM"
            } else {
                ""
            };
            let source = format!(
                ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n{before}.Bl {list} -compact\n.It{head}\n.Bd -literal\nINNER\n.Ed\n.El\nAFTER\n"
            );
            let content = query_roff_bytes(source.as_bytes()).unwrap();
            let mut gap = Gap(None);
            gap.visit_document(content.document.as_ref().unwrap());
            assert_eq!(
                gap.0,
                Some(u16::from(list != "-item" || predecessor)),
                "{source}\n{}",
                render_query_text(&content)
            );
        }
    }
    // LIST_item inherits outer predecessors through the detached list body.
    let content = query(".Bd -literal\n.Bl -item\n.It\n.Bd -literal\nINNER\n.Ed\n.El\n.Ed");
    assert!(render_query_text(&content).contains("BASE\n\n\n\nINNER\nAFTER"));
    let content = query_roff_bytes(b".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -item -compact\n.It\nFIRST\n.It\n.Bd -literal\nINNER\n.Ed\n.El\nAFTER\n").unwrap();
    assert!(render_query_text(&content).contains("FIRST\n\nINNER\nAFTER"));
}
