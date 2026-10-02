//! Source-neutral declaration grammar controls; no formatter gold is inferred.

use super::*;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}
fn strong(nodes: Vec<Inline>) -> Inline {
    Inline::Strong { children: nodes }
}
fn emphasis(nodes: Vec<Inline>) -> Inline {
    Inline::Emphasis { children: nodes }
}

fn assert_names(nodes: &[Inline], operands: &[NativeOperand], expected: &[&str]) {
    let result = option_head(nodes, operands);
    assert_eq!(
        result
            .names
            .iter()
            .map(|name| name.name.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    let visible = mant_ir::inline_plain_text(nodes);
    for name in result.names {
        assert_eq!(name.parts.len(), 1);
        assert_eq!(&visible[name.parts[0].clone()], name.name);
    }
}

#[test]
fn complete_groups_keep_every_explicit_spelling_and_exact_ranges() {
    for value in [
        "-c --stdout --to-stdout",
        "-c or --stdout --to-stdout",
        "(-0, -1, -2, -9)",
        "(-0 -1 -2 -9)",
        "-### --long --other",
    ] {
        let expected: &[&str] = if value.contains("-0") {
            &["-0", "-1", "-2", "-9"]
        } else if value.contains('#') {
            &["--long", "--other"]
        } else {
            &["-c", "--stdout", "--to-stdout"]
        };
        let nodes = [strong(vec![text(value)])];
        assert_names(&nodes, &[], expected);
        assert!(option_head(&nodes, &[]).complete);
    }
    let nodes = [text("-c --stdout --stdout")];
    assert_names(&nodes, &[], &["-c", "--stdout", "--stdout"]);
}

#[test]
fn uppercase_metavariables_require_their_own_explicit_option_receipt() {
    let nodes = [strong(vec![text("-n -NUM")])];
    for role in [NativeOperandRole::Literal, NativeOperandRole::Argument] {
        assert_names(
            &nodes,
            &[
                NativeOperand {
                    bytes: 0..2,
                    role: NativeOperandRole::ExplicitOption,
                },
                NativeOperand { bytes: 3..7, role },
            ],
            &["-n"],
        );
    }
    // The generated dash and its child may have separate word owners. Both
    // are accepted parts of the same actual Fl, never a whole-head hint.
    assert_names(
        &nodes,
        &[
            NativeOperand {
                bytes: 0..1,
                role: NativeOperandRole::ExplicitOption,
            },
            NativeOperand {
                bytes: 1..2,
                role: NativeOperandRole::ExplicitOption,
            },
            NativeOperand {
                bytes: 3..4,
                role: NativeOperandRole::ExplicitOption,
            },
            NativeOperand {
                bytes: 4..7,
                role: NativeOperandRole::ExplicitOption,
            },
        ],
        &["-n", "-NUM"],
    );
}

#[test]
fn explicit_option_receipts_survive_an_inner_italic_font() {
    let nodes = [strong(vec![text("-n -")]), emphasis(vec![text("NUM")])];
    for (role, expected) in [
        (NativeOperandRole::ExplicitOption, &["-n", "-NUM"][..]),
        (NativeOperandRole::Argument, &["-n"][..]),
        (NativeOperandRole::Literal, &["-n"][..]),
    ] {
        assert_names(&nodes, &[NativeOperand { bytes: 3..7, role }], expected);
    }
    let single = [strong(vec![text("-")]), emphasis(vec![text("NUM")])];
    assert_names(
        &single,
        &[NativeOperand {
            bytes: 0..4,
            role: NativeOperandRole::ExplicitOption,
        }],
        &["-NUM"],
    );
}

#[test]
fn argument_receipts_survive_an_inner_bold_font() {
    for value in ["-n --fake", "-n -N"] {
        let nodes = [strong(vec![text(value)])];
        let operands = [
            NativeOperand {
                bytes: 0..2,
                role: NativeOperandRole::ExplicitOption,
            },
            NativeOperand {
                bytes: 3..value.len(),
                role: NativeOperandRole::Argument,
            },
        ];
        assert_names(&nodes, &operands, &["-n"]);
    }
}

#[test]
fn name_limit_is_distinct_from_an_unsupported_weak_head() {
    for (count, expected) in [
        (255, None),
        (256, None),
        (257, Some(DeclarationLimit::Names)),
    ] {
        let value = (0..count)
            .map(|index| format!("--flag{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let nodes = [strong(vec![text(&value)])];
        let result = option_head(&nodes, &[]);
        assert_eq!(result.limit, expected);
        assert_eq!(
            result.names.len(),
            if expected.is_some() { 0 } else { count }
        );
        let item = mant_ir::DefinitionItem {
            terms: vec![nodes.to_vec()],
            description: Vec::new(),
            entry: None,
            layout: mant_ir::DefinitionLayout::default(),
            source: None,
        };
        // A constructed leading role has no accepted native operand proof.
        let inferred = super::super::infer_identity(
            &item,
            crate::definitions::context::DefinitionContext::Generic,
            Some(crate::definitions::NativeHeadRole::Option),
            None,
        );
        assert_eq!(inferred.limit, None);
    }
    let opaque = [strong(vec![text("--pattern [unterminated,--fake")])];
    assert_eq!(option_head(&opaque, &[]).limit, None);
}

#[test]
fn quote_bracket_and_plain_argument_state_survives_font_changes() {
    for value in [
        "--number -10,--fake,20",
        "--pattern \"one, --fake,two\"",
        "--pattern 'one, --fake,two'",
        "--pattern [one,--fake,two]",
        "--pattern first,--fake,last",
    ] {
        let nodes = [Inline::Code {
            value: value.into(),
        }];
        assert_names(&nodes, &[], &[value.split_whitespace().next().unwrap()]);
    }
    let nodes = [strong(vec![
        text("-L"),
        emphasis(vec![text("first")]),
        text(", --fake"),
        emphasis(vec![text(",last")]),
    ])];
    // Without a native new-operand proof, a temporary literal font change
    // inside the already entered argument cannot restart the declaration.
    let argument = NativeOperand {
        bytes: 2.."-Lfirst, --fake,last".len(),
        role: NativeOperandRole::Argument,
    };
    assert_names(
        &nodes,
        &[
            NativeOperand {
                bytes: 0..2,
                role: NativeOperandRole::Literal,
            },
            argument,
        ],
        &["-L"],
    );
}

#[test]
fn complete_native_operands_allow_a_following_declaration_not_an_inner_font_run() {
    let nodes = [
        strong(vec![text("-L")]),
        emphasis(vec![
            text("first"),
            strong(vec![text(", --fake")]),
            text(",last,"),
        ]),
        strong(vec![text("--all ")]),
        emphasis(vec![text("FILE")]),
    ];
    let boundary = "-Lfirst, --fake,last,".len();
    let operands = [
        NativeOperand {
            bytes: 0..2,
            role: NativeOperandRole::Literal,
        },
        NativeOperand {
            bytes: 2..boundary,
            role: NativeOperandRole::Argument,
        },
        NativeOperand {
            bytes: boundary..boundary + 6,
            role: NativeOperandRole::Literal,
        },
        NativeOperand {
            bytes: boundary + 6..boundary + 10,
            role: NativeOperandRole::Argument,
        },
    ];
    assert_names(&nodes, &operands, &["-L", "--all"]);
    assert!(option_head(&nodes, &operands).complete);
}

#[test]
fn effective_combined_style_names_do_not_make_later_parameters_literal() {
    for node in [
        strong(vec![emphasis(vec![text("--all")])]),
        emphasis(vec![strong(vec![text("--all")])]),
    ] {
        assert_names(
            &[node, text(" "), emphasis(vec![text("FILE,--fake")])],
            &[],
            &["--all"],
        );
    }
    assert_names(&[emphasis(vec![text("--operand")])], &[], &[]);
    assert_names(
        &[emphasis(vec![Inline::Code {
            value: "--operand".into(),
        }])],
        &[],
        &[],
    );
}

#[test]
fn exhausted_groups_and_uncertain_argument_nesting_never_restart() {
    let value = std::iter::repeat_n("--all", MAX_NAMES + 1)
        .collect::<Vec<_>>()
        .join(" ");
    assert_names(&[text(&value)], &[], &[]);
    let value = format!(
        "--all {}X{} , --fake",
        "[".repeat(MAX_NESTING + 1),
        "]".repeat(MAX_NESTING + 1)
    );
    assert_names(&[text(&value)], &[], &["--all"]);
    assert!(!option_head(&[text(&value)], &[]).complete);
}

#[test]
fn effective_combined_style_can_begin_or_finish_one_complete_name() {
    for nodes in [
        vec![
            strong(vec![text("--a")]),
            strong(vec![emphasis(vec![text("ll")])]),
        ],
        vec![
            strong(vec![emphasis(vec![text("--a")])]),
            strong(vec![text("ll")]),
        ],
    ] {
        assert_names(&nodes, &[], &["--all"]);
        assert!(option_head(&nodes, &[]).complete);
    }
}

#[test]
fn lexical_escape_parity_is_shared_by_restart_and_complete_argument_validation() {
    for value in ["--pattern \"one\\\"two\"", "--pattern [one\\]two]"] {
        let nodes = [text(value)];
        assert_names(&nodes, &[], &["--pattern"]);
        assert!(option_head(&nodes, &[]).complete);
    }
    for (value, expected) in [
        ("--pattern foo\\, --other", &["--pattern"][..]),
        ("--pattern foo\\\\, --other", &["--pattern", "--other"][..]),
        ("--pattern foo\\| --other", &["--pattern"][..]),
    ] {
        let start = "--pattern ".len();
        let boundary = value.find("--other").unwrap();
        let operands = [
            NativeOperand {
                bytes: 0..start,
                role: NativeOperandRole::Literal,
            },
            NativeOperand {
                bytes: start..boundary,
                role: NativeOperandRole::Argument,
            },
            NativeOperand {
                bytes: boundary..value.len(),
                role: NativeOperandRole::Literal,
            },
        ];
        assert_names(&[text(value)], &operands, expected);
    }
}

#[test]
fn leading_authored_whitespace_does_not_hide_a_proved_literal_operand() {
    for prefix in [" ", "\u{a0}"] {
        let value = format!("-Ldir, {prefix}--output=FILE");
        let argument_end = "-Ldir, ".len();
        let operands = [
            NativeOperand {
                bytes: 0..2,
                role: NativeOperandRole::Literal,
            },
            NativeOperand {
                bytes: 2..argument_end,
                role: NativeOperandRole::Argument,
            },
            NativeOperand {
                bytes: argument_end..value.len() - 4,
                role: NativeOperandRole::Literal,
            },
            NativeOperand {
                bytes: value.len() - 4..value.len(),
                role: NativeOperandRole::Argument,
            },
        ];
        assert_names(&[text(&value)], &operands, &["-L", "--output"]);
    }
}

#[test]
fn only_proved_parameter_boundaries_can_promote_a_weak_paragraph_owner() {
    let nodes = [
        strong(vec![text("--opt ")]),
        emphasis(vec![text("arg,")]),
        strong(vec![text("--other FILE")]),
    ];
    let result = option_head(&nodes, &[]);
    assert_names(&nodes, &[], &["--opt", "--other"]);
    assert!(result.complete);
    assert!(!result.inferred_complete);
    let value = "--output=FILE, --other";
    let nodes = [text(value)];
    assert_names(&nodes, &[], &["--output", "--other"]);
    assert!(option_head(&nodes, &[]).inferred_complete);
}

#[test]
fn one_long_argument_whitespace_run_is_consumed_without_suffix_restarts() {
    let value = format!("--opt VALUE{}ordinary", " ".repeat(65_536));
    let view = HeadView::new(&[text(&value)], &[]);
    let mut scanner = Scanner::new(&view);
    scanner.cursor = "--opt VALUE".len();
    scanner.argument_start = Some("--opt ".len());
    scanner.consume_argument(' ');
    assert_eq!(scanner.cursor, value.find("ordinary").unwrap());
}

#[test]
fn whitespace_only_literal_operand_cannot_prove_the_following_unowned_name() {
    let nodes = [text("-Larg, --other")];
    let operands = [
        NativeOperand {
            bytes: 0..2,
            role: NativeOperandRole::Literal,
        },
        NativeOperand {
            bytes: 2..6,
            role: NativeOperandRole::Argument,
        },
        NativeOperand {
            bytes: 6..7,
            role: NativeOperandRole::Literal,
        },
    ];
    assert_names(&nodes, &operands, &["-L"]);
    assert!(!HeadView::new(&nodes, &operands).literal_operand_starts(7));
}

#[test]
fn uppercase_metavariables_after_an_option_are_arguments_even_when_bold() {
    // Native pre_B selects a font, not a role for each space-separated token.
    // The exact .B "-n -NUM" pristine input ran before this regression;
    // its shared ManT lexical contract excludes -NUM while retaining the form.
    for value in ["-n -NUM", "-n -COUNT", "-n -FILE_NAME", "-n -10,--fake,20"] {
        assert_names(&[strong(vec![text(value)])], &[], &["-n"]);
    }
    for (value, expected) in [
        ("-n -N", &["-n", "-N"][..]),
        ("-n --NUM", &["-n", "--NUM"][..]),
        ("-NUM", &["-NUM"][..]),
    ] {
        assert_names(&[strong(vec![text(value)])], &[], expected);
    }
}
