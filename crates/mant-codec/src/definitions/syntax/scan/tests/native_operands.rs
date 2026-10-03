//! Native operands in the shared declaration scanner.
use super::*;

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
fn alias_or_cannot_skip_a_real_parameter_operand() {
    // Exact pinned mdoc Ar/No siblings ran first: Ar selects parameter
    // styling and owns its word independently of the following No text.
    let nodes = [
        strong(vec![text("-a ")]),
        emphasis(vec![text("or")]),
        text(" --ascii"),
    ];
    assert_names(&nodes, &[], &["-a"]);
    let value = "-a or --ascii";
    let operands = [NativeOperand {
        bytes: 3..5,
        role: NativeOperandRole::Argument,
    }];
    assert_names(&[strong(vec![text(value)])], &operands, &["-a"]);
    // Ns joins an ordinary `o` to a distinct Ar `r`; qualification belongs
    // to both accepted scalars of the alias token, not just its first one.
    let nodes = [
        strong(vec![text("-a ")]),
        text("o"),
        emphasis(vec![text("r")]),
        text(" --ascii"),
    ];
    assert_names(&nodes, &[], &["-a"]);
    let operands = [NativeOperand {
        bytes: 4..5,
        role: NativeOperandRole::Argument,
    }];
    assert_names(&[strong(vec![text(value)])], &operands, &["-a"]);
    for value in ["-L:<funcname>:<file>", "-L@<file>"] {
        assert!(
            option_head(&[text(value)], &[]).inferred_complete,
            "{value}"
        );
    }
    for value in ["-L :<file>", "-L:<file>::<other>", "-L:<file>:"] {
        assert!(
            !option_head(&[text(value)], &[]).inferred_complete,
            "{value}"
        );
    }
}
