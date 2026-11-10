use super::*;

#[test]
fn compound_names_quotes_uris_and_negative_numbers_keep_their_source_appearance() {
    let original = content(vec![
        text("Use "),
        emphasis("machine"),
        strong("-gcc-"),
        emphasis("version"),
        text(" with "),
        strong("--visible"),
        text("; keep "),
        strong("'--quoted'"),
        text(" "),
        strong("https://host/--url?q=[-uri]"),
        text(" "),
        strong("user@host.test"),
        text(" "),
        strong("-1 -20 x-y"),
        text(" unchanged."),
    ]);
    let rendered = DocumentView::new(&original).render(240);
    assert_eq!(style_at(&rendered, "--visible").fg, Some(theme::GREEN));
    for unchanged in [
        "-gcc-",
        "--quoted",
        "--url",
        "-uri",
        "user@host.test",
        "-1",
        "-20",
        "x-y",
    ] {
        assert_eq!(
            style_at(&rendered, unchanged).fg,
            Some(theme::STRONG),
            "{unchanged}"
        );
    }
    assert_eq!(style_at(&rendered, "machine").fg, Some(theme::TEXT));
    assert!(
        style_at(&rendered, "machine")
            .add_modifier
            .contains(Modifier::ITALIC)
    );
}

#[test]
fn inline_code_comments_cannot_capture_later_prose_option_references() {
    let original = content(vec![
        code("gcc -O2 # unmatched '"),
        text("; see "),
        strong("--visible"),
        text("."),
    ]);
    let rendered = DocumentView::new(&original).render(240);
    assert_eq!(style_at(&rendered, "gcc").fg, Some(theme::PEACH));
    assert_eq!(style_at(&rendered, "-O2").fg, Some(theme::GREEN));
    assert_eq!(style_at(&rendered, "# unmatched").fg, Some(theme::SUBTEXT));
    assert_eq!(style_at(&rendered, "--visible").fg, Some(theme::GREEN));
}

#[test]
fn optional_option_spelling_and_fixed_choices_are_not_argument_templates() {
    let original = content(vec![
        strong("-V["),
        emphasis("ersion"),
        strong("] -h["),
        emphasis("uman-readable"),
        strong("] --method=[GET|POST] --kind=[CUSTOM_KIND|OTHER_KIND]"),
    ]);
    let rendered = DocumentView::new(&original).render(240);
    for option in ["-V", "-h", "--method", "--kind"] {
        assert_eq!(
            style_at(&rendered, option).fg,
            Some(theme::GREEN),
            "{option}"
        );
    }
    for suffix in ["ersion", "uman-readable"] {
        let style = style_at(&rendered, suffix);
        assert_eq!(style.fg, Some(theme::TEXT));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }
    for literal in ["GET", "POST", "CUSTOM_KIND", "OTHER_KIND"] {
        assert_eq!(
            style_at(&rendered, literal).fg,
            Some(theme::STRONG),
            "{literal}"
        );
    }
}

#[test]
fn equation_text_never_supplies_option_or_command_evidence() {
    for value in [
        "math [-f FILE]".to_owned(),
        "'".to_owned(),
        "\"".to_owned(),
        format!("{}x{}", "(".repeat(65), ")".repeat(65)),
    ] {
        let expression = serde_json::from_value(serde_json::json!({
            "kind": "text", "font": "none", "operator": "none", "text": value, "children": [],
        }))
        .unwrap();
        let original = content(vec![
            text("The equation "),
            Inline::Strong {
                children: vec![Inline::Equation {
                    value: value.clone(),
                    expression,
                }],
            },
            text(" is followed by "),
            emphasis("--visible"),
            text("."),
        ]);
        assert_eq!(
            mant_ir::validate_document(original.document.as_ref().unwrap()),
            []
        );
        let rendered = DocumentView::new(&original).render(240);
        assert_eq!(style_at(&rendered, "--visible").fg, Some(theme::GREEN));
        for token in if value == "math [-f FILE]" {
            vec!["math", "-f", "FILE"]
        } else {
            vec![value.as_str()]
        } {
            assert_eq!(
                style_at(&rendered, token).fg,
                Some(theme::SUBTEXT_BRIGHT),
                "{token}"
            );
        }
    }
}

#[test]
fn equation_punctuation_never_completes_neighboring_prose_tokens() {
    for (left, math, right) in [(".", ".", ". and "), ("", "://", " and ")] {
        let expression = serde_json::from_value(serde_json::json!({
            "kind": "text", "font": "none", "operator": "none", "text": math, "children": [],
        }))
        .unwrap();
        let original = content(vec![
            strong("--alpha"),
            text(left),
            Inline::Equation {
                value: math.into(),
                expression,
            },
            text(right),
            strong("--beta"),
        ]);
        let rendered = DocumentView::new(&original).render(240);
        for option in ["--alpha", "--beta"] {
            assert_eq!(style_at(&rendered, option).fg, Some(theme::GREEN));
        }
        if math == "." {
            for occurrence in [0, 2] {
                assert_eq!(
                    style_at_occurrence(&rendered, ".", occurrence).fg,
                    Some(theme::TEXT),
                    "an equation cannot complete a prose ellipsis"
                );
            }
        }
    }
}

#[test]
fn exact_bound_names_remain_stronger_than_argument_template_guesses() {
    use mant_ir::{
        EntryContentSlice, EntryForm, EntryInlineRoot, EntryNameBinding, EntryNameEvidence,
        ParameterKind,
    };
    let mut entry = facts(EntryKind::Parameter {
        parameter_kind: ParameterKind::Option,
    });
    entry.names = vec!["--name".into()];
    entry.name_bindings = vec![EntryNameBinding {
        name: 0,
        evidence: EntryNameEvidence::Declared,
        occurrences: vec![EntryForm {
            parts: vec![
                EntryContentSlice {
                    root: EntryInlineRoot::Term { index: 0 },
                    path: vec![0],
                    bytes: None,
                },
                EntryContentSlice {
                    root: EntryInlineRoot::Term { index: 0 },
                    path: vec![1],
                    bytes: None,
                },
            ],
        }],
    }];
    let mut original = content(vec![]);
    original.document.as_mut().unwrap().sections[0].blocks = vec![definition(
        vec![
            strong("--"),
            emphasis("name"),
            strong("="),
            emphasis("value"),
        ],
        vec![],
        entry,
    )];
    assert_eq!(
        mant_ir::validate_document(original.document.as_ref().unwrap()),
        []
    );
    let rendered = DocumentView::new(&original).render(120);
    assert_eq!(style_at(&rendered, "--name").fg, Some(theme::GREEN));
    assert_eq!(style_at(&rendered, "name").fg, Some(theme::GREEN));
    assert_eq!(style_at(&rendered, "value").fg, Some(theme::SAPPHIRE));
}

#[test]
fn unaccented_source_markup_keeps_ordinary_inline_font_precedence() {
    let original = content(vec![
        Inline::Strong {
            children: vec![code("explanatory prose")],
        },
        text("; consult "),
        emphasis("--help"),
        text("."),
    ]);
    let rendered = DocumentView::new(&original).render(120);
    let style = style_at(&rendered, "explanatory prose");
    assert_eq!(style.fg, Some(theme::SUBTEXT_BRIGHT));
    assert_eq!(style.bg, Some(theme::SURFACE));
    assert!(style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(style_at(&rendered, "--help").fg, Some(theme::GREEN));
}
