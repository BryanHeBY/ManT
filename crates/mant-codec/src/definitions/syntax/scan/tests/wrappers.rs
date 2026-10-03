//! Wrappers in the shared declaration scanner.
use super::*;

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
fn paired_outer_wrappers_are_declarations_but_parameter_quotes_stay_opaque() {
    // The exact visible quote/bracket carriers ran pinned pre_B and
    // pre_alternate first. Those handlers preserve spelling and join BR
    // operands; this source-neutral grammar never gives parameter quotes
    // or escaped quotes a fresh declaration boundary.
    for value in [
        "\"--foo\"",
        "'--foo'",
        "“--foo”",
        "‘--foo’",
        "({--foo})",
        "[“({--foo})”]",
        "“--foo FILE”",
        "(--foo \"one --fake\")",
    ] {
        let nodes = [strong(vec![text(value)])];
        assert_names(&nodes, &[], &["--foo"]);
        assert!(option_head(&nodes, &[]).inferred_complete, "{value}");
    }
    let split = [strong(vec![text("“--")]), text("foo”")];
    assert_names(&split, &[], &["--foo"]);
    for value in ["“--foo", "“--foo’", "“ordinary --fake”", "\\\"--fake\\\""] {
        assert_names(&[strong(vec![text(value)])], &[], &[]);
        assert!(!option_head(&[text(value)], &[]).inferred_complete);
    }
    for value in ["--set=\"--fake\"", "--set \"--fake\"", "--set=[{--fake}]"] {
        assert_names(&[strong(vec![text(value)])], &[], &["--set"]);
        assert!(option_head(&[text(value)], &[]).inferred_complete);
    }
    let value = "“--fake”";
    assert_names(
        &[strong(vec![text(value)])],
        &[NativeOperand {
            bytes: 0..value.len(),
            role: NativeOperandRole::Argument,
        }],
        &[],
    );
    let uncertain = format!("{}--fake{}", "[".repeat(65), "]".repeat(65));
    assert_names(&[text(&uncertain)], &[], &[]);
}

#[test]
fn inherited_wrapper_fonts_require_explicit_inner_options() {
    // Exact Dq/Bq under ft I or Bf -emphasis ran pristine first. quote_pre
    // retains that font; Fl supplies a distinct generated dash and child.
    for (open, close) in [("“", "”"), ("[", "]")] {
        let nodes = [
            emphasis(vec![text(open)]),
            strong(vec![text("-foo")]),
            emphasis(vec![text(close)]),
        ];
        let end = open.len() + "-foo".len();
        let mut operands = [
            NativeOperand {
                bytes: 0..open.len(),
                role: NativeOperandRole::Literal,
            },
            NativeOperand {
                bytes: open.len()..open.len() + 1,
                role: NativeOperandRole::ExplicitOption,
            },
            NativeOperand {
                bytes: open.len() + 1..end,
                role: NativeOperandRole::ExplicitOption,
            },
            NativeOperand {
                bytes: end..end + close.len(),
                role: NativeOperandRole::Literal,
            },
        ];
        assert_names(&nodes, &operands, &["-foo"]);
        assert_names(&nodes, &[], &[]);
        operands[0].role = NativeOperandRole::Argument;
        assert_names(&nodes, &operands, &[]);
        operands[0].role = NativeOperandRole::Literal;
        operands[1].role = NativeOperandRole::Literal;
        operands[2].role = NativeOperandRole::Literal;
        assert_names(&nodes, &operands, &[]);
    }
}
