//! Parameters in the shared declaration scanner.
use super::*;

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

#[test]
fn connected_parameter_fragments_keep_their_source_punctuation() {
    let nodes = [
        strong(vec![text("-L")]),
        emphasis(vec![text("<start>")]),
        text(","),
        emphasis(vec![text("<end>")]),
        text(":"),
        emphasis(vec![text("<file>")]),
        text(", "),
        strong(vec![text("-L")]),
        text(":"),
        emphasis(vec![text("<funcname>")]),
        text(":"),
        emphasis(vec![text("<file>")]),
    ];
    assert_names(&nodes, &[], &["-L", "-L"]);
    assert!(option_head(&nodes, &[]).inferred_complete);
    let nodes = [
        strong(vec![text("--map-users ")]),
        emphasis(vec![text("inner")]),
        text(":_outer_:"),
        emphasis(vec![text("count")]),
    ];
    assert!(option_head(&nodes, &[]).inferred_complete);
    for (before, inner, after) in [
        ("--sd-id ", "name", "[@digits]"),
        ("--trailer ", "<token>", "[(=|:)<value>]"),
    ] {
        let nodes = [
            strong(vec![text(before)]),
            emphasis(vec![text(inner)]),
            text(after),
        ];
        assert!(
            option_head(&nodes, &[]).inferred_complete,
            "{before}{inner}{after}"
        );
    }
    let nodes = [
        strong(vec![text("-g ")]),
        emphasis(vec![text("GLOB")]),
        text(", "),
        strong(vec![text("--glob=")]),
        emphasis(vec![text("GLOB")]),
    ];
    assert_names(&nodes, &[], &["-g", "--glob"]);
    assert!(option_head(&nodes, &[]).inferred_complete);
}

#[test]
fn literal_delimiters_complete_a_single_styled_parameter_token() {
    // The exact font-escaped TP and TEXT/RS sources ran pristine first.
    // term_word changes fonts inside the same TEXT; the external comma is
    // authored declaration syntax, rather than proof of another roff operand.
    for separator in [", ", " | "] {
        let nodes = [
            strong(vec![text("-o ")]),
            emphasis(vec![text("file")]),
            text(separator),
            strong(vec![text("--output=")]),
            emphasis(vec![text("file")]),
        ];
        let value = mant_ir::inline_plain_text(&nodes);
        for operands in [
            Vec::new(),
            vec![NativeOperand {
                bytes: 0..value.len(),
                role: NativeOperandRole::Literal,
            }],
        ] {
            assert_names(&nodes, &operands, &["-o", "--output"]);
            assert!(option_head(&nodes, &operands).inferred_complete);
        }
    }
    for parameter in ["file,", "file names", "file/name"] {
        let nodes = [
            strong(vec![text("-o ")]),
            emphasis(vec![text(parameter)]),
            text(", "),
            strong(vec![text("--fake")]),
        ];
        assert!(!option_head(&nodes, &[]).inferred_complete, "{parameter}");
    }
    let nodes = [
        strong(vec![text("-o ")]),
        emphasis(vec![text("file,")]),
        text(" --fake"),
    ];
    let operand = NativeOperand {
        bytes: 3..8,
        role: NativeOperandRole::Argument,
    };
    assert_names(&nodes, &[operand], &["-o"]);
}

#[test]
fn a_temporary_parameter_font_requires_both_token_boundaries() {
    // pre_B supplies one child term_word, and font escapes inside that child
    // never split the native operand. All four exact sources plus the original
    // inword-font-argument control ran pristine before these assertions.
    for boundary in [" ", "="] {
        let nodes = [
            strong(vec![text(&format!("-L{boundary}"))]),
            emphasis(vec![text("first")]),
            strong(vec![text(", --other")]),
        ];
        let value = mant_ir::inline_plain_text(&nodes);
        let operand = NativeOperand {
            bytes: 0..value.len(),
            role: NativeOperandRole::Literal,
        };
        assert_names(&nodes, &[operand], &["-L", "--other"]);
        assert!(option_head(&nodes, &[]).inferred_complete);
    }
    for (before, parameter, after) in [
        ("-L", "first", ", --fake,last"),
        ("-L fi", "rst", ", --fake"),
        ("-L ", "fir", "st, --fake"),
    ] {
        let nodes = [
            strong(vec![text(before)]),
            emphasis(vec![text(parameter)]),
            strong(vec![text(after)]),
        ];
        let value = mant_ir::inline_plain_text(&nodes);
        let operand = NativeOperand {
            bytes: 0..value.len(),
            role: NativeOperandRole::Literal,
        };
        assert_names(&nodes, &[operand], &["-L"]);
        assert!(!option_head(&nodes, &[]).inferred_complete);
    }
}

#[test]
fn repetition_suffixes_complete_parameters_without_accepting_literal_prose() {
    // The exact TEXT/RS option... source ran pristine first. The italic
    // parameter and its literal repetition suffix remain separate IR leaves;
    // validation must not invent whitespace or include dots in a name.
    for suffix in ["...", "", ",..."] {
        let nodes = [
            strong(vec![text("-O, --test-opts ")]),
            emphasis(vec![text("option")]),
            text(suffix),
        ];
        assert_names(&nodes, &[], &["-O", "--test-opts"]);
        assert!(option_head(&nodes, &[]).inferred_complete, "{suffix}");
    }
    for suffix in ["..", "....", "...more", ".ordinary"] {
        let nodes = [
            strong(vec![text("--test-opts ")]),
            emphasis(vec![text("option")]),
            text(suffix),
        ];
        assert!(!option_head(&nodes, &[]).inferred_complete, "{suffix}");
    }
    assert!(!option_head(&[text("--test-opts option...")], &[]).inferred_complete);
}

#[test]
fn parameter_owned_openers_do_not_wrap_new_declarations() {
    // Pinned Ar/No/Ar executes three distinct term_word calls; Ar selects
    // UNDER before its opening quote. A generated Dq wrapper around Fl
    // instead supplies ordinary delimiters around an actual option.
    let nodes = [
        emphasis(vec![text("“")]),
        text(" --fake "),
        emphasis(vec![text("”")]),
    ];
    assert_names(&nodes, &[], &[]);
    let value = "“ --fake ”";
    let operands = [NativeOperand {
        bytes: 0.."“".len(),
        role: NativeOperandRole::Argument,
    }];
    assert_names(&[strong(vec![text(value)])], &operands, &[]);
    assert_names(
        &[text("“"), strong(vec![text("-foo")]), text("”")],
        &[],
        &["-foo"],
    );
}

#[test]
fn bare_parameter_completion_requires_a_top_level_delimiter_and_independent_name() {
    // Exact TP/HP sources for these spellings and fonts ran pristine first.
    // term_word() retains each scalar through in-word font switches; fonts
    // neither restrict parameter spelling nor create a new native operand.
    for parameter in [
        "script",
        "Script",
        "sCript",
        "SCRIPT",
        "ScriptFile",
        "Script-file",
        "_script",
        "_Script",
        "SCRIPT_FILE",
        "脚本",
        "s_cript",
        "1A",
        "_SCRIPT",
    ] {
        for (font, separator) in [text, strong_text, emphasis_text]
            .into_iter()
            .flat_map(|font| [", ", ",", " | "].map(|separator| (font, separator)))
        {
            let nodes = [
                strong(vec![text("-e")]),
                text(" "),
                font(parameter),
                text(separator),
                strong(vec![text("--expression=")]),
                emphasis(vec![text("script")]),
            ];
            assert_names(&nodes, &[], &["-e", "--expression"]);
            assert!(option_head(&nodes, &[]).inferred_complete);
            // Transparent inline fragmentation cannot change the grammar or bytes.
            let fragmented: Vec<_> = nodes
                .iter()
                .flat_map(|node| match node {
                    Inline::Strong { children } => mant_ir::inline_plain_text(children)
                        .chars()
                        .map(|c| strong(vec![text(&c.to_string())]))
                        .collect(),
                    Inline::Emphasis { children } => mant_ir::inline_plain_text(children)
                        .chars()
                        .map(|c| emphasis(vec![text(&c.to_string())]))
                        .collect(),
                    _ => vec![node.clone()],
                })
                .collect();
            assert_names(&fragmented, &[], &["-e", "--expression"]);
        }
    }
    for argument in [
        "first,--fake,last",
        "first, --fake,last",
        "-10,--fake,20",
        "first second, --fake",
        "script,, --fake",
        "script --fake",
        "[first, --fake]",
        "\"first, --fake\"",
        "script, otherwise continue",
    ] {
        let nodes = [
            strong(vec![text("-e")]),
            text(" "),
            strong(vec![text(argument)]),
        ];
        assert_names(&nodes, &[], &["-e"]);
    }
    // These exact TP/HP inputs ran pristine in Roman and bold first. Word
    // validity does not provide uppercase restart evidence without its initial
    // ASCII uppercase qualifier; the comma remains inside an opaque value.
    for argument in ["1A,--fake,last", "_SCRIPT,--fake,last"] {
        for font in [text, strong_text] {
            let nodes = [strong(vec![text("-e")]), text(" "), font(argument)];
            assert_names(&nodes, &[], &["-e"]);
            assert!(!option_head(&nodes, &[]).inferred_complete);
        }
    }
    let nodes = [
        strong(vec![text("-e ")]),
        emphasis(vec![text("first, --fake")]),
    ];
    let operand = NativeOperand {
        bytes: 3..16,
        role: NativeOperandRole::Argument,
    };
    assert_names(&nodes, &[operand], &["-e"]);
}

fn strong_text(value: &str) -> Inline {
    strong(vec![text(value)])
}

fn emphasis_text(value: &str) -> Inline {
    emphasis(vec![text(value)])
}
