//! Keep declaration separators and parameter styling distinct until extraction.
use super::declaration::DeclarationState;
use mant_ir::Inline;

use mant_ir::inline_plain_text as plain_text;

/// Read visible literal content only until an explicitly styled parameter.
/// Transparent wrappers do not erase that boundary, even without whitespace.
fn append_name_prefix(nodes: &[Inline], output: &mut String) -> bool {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                output.push_str(value);
            }
            Inline::Strong { children } | Inline::Link { children, .. } => {
                if !append_name_prefix(children, output) {
                    return false;
                }
            }
            Inline::Emphasis { children } => {
                if first_content_is_parameter(children).is_some() {
                    return false;
                }
                // Whitespace-only styling is not a parameter, but still
                // occupies bytes before a later recognized name.
                output.push_str(&plain_text(children));
            }
            Inline::Anchor { .. } => {}
            Inline::LineBreak { .. } => output.push('\n'),
        }
    }
    true
}

pub(super) fn literal_prefix(inlines: &[Inline]) -> String {
    let mut prefix = String::new();
    append_name_prefix(inlines, &mut prefix);
    prefix
}

/// An adjacent placeholder is part of the variable name, not the boundary of
/// a shorter exact name. Separated operands and assignment values are different.
pub(super) fn environment_prefix(inlines: &[Inline]) -> Option<String> {
    let mut prefix = String::new();
    let complete = append_name_prefix(inlines, &mut prefix);
    (complete || prefix.ends_with(char::is_whitespace) || prefix.contains('=')).then_some(prefix)
}

/// Split complete declarations without flattening parameter spans. Bracket
/// nesting and local argument phases survive strong/link wrapper boundaries;
/// punctuation inside an argument is not a fresh declaration.
pub(super) fn declaration_groups(term: &[Inline]) -> Vec<Vec<Inline>> {
    split_groups(
        term,
        &[',', '|'],
        &mut None,
        &mut DeclarationState::new(plain_text(term), term),
    )
}

/// One style-preserving splitter for alias punctuation. A bounded pass counts
/// visible bytes even in opaque arguments. Links preserve their wrapper while
/// exposing their visible children; their destination is never name evidence.
fn split_groups(
    term: &[Inline],
    separators: &[char],
    remaining: &mut Option<usize>,
    state: &mut DeclarationState,
) -> Vec<Vec<Inline>> {
    let mut groups = vec![Vec::new()];
    for inline in term {
        let parts = match inline {
            Inline::Text { value } => value
                .split(|character| {
                    state.separator(character, take_separator(character, separators, remaining))
                })
                .map(|value| {
                    vec![Inline::Text {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Code { value } | Inline::Equation { value, .. } => value
                .split(|character| {
                    state.separator(character, take_separator(character, separators, remaining))
                })
                .map(|value| {
                    vec![Inline::Code {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Strong { children } => split_groups(children, separators, remaining, state)
                .into_iter()
                .map(|children| vec![Inline::Strong { children }])
                .collect(),
            Inline::Link {
                target,
                title,
                children,
            } => split_groups(children, separators, remaining, state)
                .into_iter()
                .map(|children| {
                    vec![Inline::Link {
                        target: target.clone(),
                        title: title.clone(),
                        children,
                    }]
                })
                .collect(),
            _ => {
                state.opaque(&plain_text(std::slice::from_ref(inline)));
                if let Some(bytes) = remaining {
                    *bytes = bytes.saturating_sub(plain_text(std::slice::from_ref(inline)).len());
                }
                vec![vec![inline.clone()]]
            }
        };
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                groups.push(Vec::new());
            }
            groups.last_mut().expect("at least one group").extend(part);
        }
    }
    groups
}

fn take_separator(character: char, separators: &[char], remaining: &mut Option<usize>) -> bool {
    let eligible = remaining.is_none_or(|bytes| bytes >= character.len_utf8());
    if let Some(bytes) = remaining {
        *bytes = bytes.saturating_sub(character.len_utf8());
    }
    eligible && separators.contains(&character)
}

fn first_content_is_parameter(term: &[Inline]) -> Option<bool> {
    term.iter().find_map(|inline| match inline {
        Inline::Anchor { .. } | Inline::LineBreak { .. } => None,
        Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
            (!value.trim().is_empty()).then_some(false)
        }
        Inline::Strong { children } | Inline::Link { children, .. } => {
            first_content_is_parameter(children)
        }
        Inline::Emphasis { children } => first_content_is_parameter(children).map(|_| true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaration_state_crosses_wrappers_and_bounds_uncertain_nesting() {
        let text = |value: &str| Inline::Text {
            value: value.into(),
        };
        let link = |children| Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.invalid/".into(),
            },
            title: None,
            children,
        };
        for parameter in ["[a|b]", "{+|-}", "<日本,名前>", "[a|b", "a] | b"] {
            let term = vec![
                Inline::Strong {
                    children: vec![text("set ")],
                },
                link(vec![text(parameter)]),
            ];
            assert_eq!(declaration_groups(&term), vec![term], "{parameter}");
        }
        let deeply_nested = format!("set {}x{} | phantom", "[".repeat(65), "]".repeat(65));
        let term = vec![text(&deeply_nested)];
        assert_eq!(declaration_groups(&term), vec![term]);

        let term = vec![
            text("--界"),
            Inline::Emphasis {
                children: vec![link(vec![text("値,--FAKE")])],
            },
            text(", --other"),
        ];
        assert_eq!(
            declaration_groups(&term)
                .iter()
                .map(|group| literal_prefix(group))
                .collect::<Vec<_>>(),
            ["--界", " --other"]
        );

        let term = vec![text("--mode=[a|b], --other")];
        assert_eq!(declaration_groups(&term).len(), 2);
    }

    #[test]
    fn linked_alias_groups_preserve_native_separators_and_the_source() {
        let source = vec![Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.invalid".into(),
            },
            title: None,
            children: vec![Inline::Strong {
                children: vec![Inline::Text {
                    value: "-n/-NUM".into(),
                }],
            }],
        }];
        let original = source.clone();
        assert_eq!(
            super::super::option_names_from_terms(std::slice::from_ref(&source)),
            ["-n", "-NUM"]
        );
        assert_eq!(source, original);
    }

    #[test]
    fn transparent_links_preserve_separators_and_parameter_ancestry() {
        let text = |value: &str| Inline::Text {
            value: value.into(),
        };
        let link = |children| Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.invalid/--not-a-name".into(),
            },
            title: None,
            children,
        };
        // Pure italic is an argument. Strong(Emphasis) is the effective
        // combined font, whose literal names are tested separately.
        let source = vec![link(vec![
            Inline::Strong {
                children: vec![text("-L")],
            },
            Inline::Emphasis {
                children: vec![text("dir,--FAKE")],
            },
            Inline::Strong {
                children: vec![text(", --library")],
            },
        ])];
        let original = source.clone();
        assert_eq!(
            super::super::option_names_from_terms(std::slice::from_ref(&source)),
            ["-L", "--library"]
        );
        assert_eq!(source, original);
        let argument = vec![Inline::Emphasis {
            children: vec![link(vec![text("-n,--FAKE")])],
        }];
        assert_eq!(super::super::option_names_from_terms(&[argument]).len(), 0);
        let slash = vec![link(vec![text("-n/-NUM")])];
        assert_eq!(
            super::super::option_names_from_terms(&[slash]),
            ["-n", "-NUM"]
        );
    }

    #[test]
    fn slash_grouping_keeps_styles_and_excludes_later_argument_paths() {
        let code = |value: &str| Inline::Code {
            value: value.into(),
        };
        let argument = |value: &str| Inline::Emphasis {
            children: vec![code(value)],
        };
        for spacer in [
            code(""),
            Inline::anchor("invisible"),
            Inline::Strong { children: vec![] },
        ] {
            for name in [
                argument("-NUM"),
                Inline::Strong {
                    children: vec![argument("-NUM")],
                },
            ] {
                let term = vec![code("-n/"), spacer.clone(), name];
                let expected = if matches!(term.last(), Some(Inline::Strong { .. })) {
                    vec!["-n", "-NUM"]
                } else {
                    vec!["-n"]
                };
                assert_eq!(super::super::option_names_from_terms(&[term]), expected);
            }
        }
        for (term, names) in [
            (vec![code("-n"), argument("/-NUM")], vec!["-n"]),
            (vec![code("-n/--number /tmp/-NUM")], vec!["-n", "--number"]),
            (
                vec![code("[-n/--number] /tmp/-NUM")],
                vec!["-n", "--number"],
            ),
            (
                vec![code("-n/--number"), argument(" /日本/"), code("-NUM")],
                vec!["-n", "--number"],
            ),
            (vec![code("--output=dir/-NUM")], vec!["--output"]),
            (vec![code("-n/-NUM")], vec!["-n", "-NUM"]),
        ] {
            assert_eq!(
                super::super::option_names_from_terms(std::slice::from_ref(&term)),
                names,
                "{term:?}"
            );
            // Option-only slash rules must not leak into command grouping.
            assert_eq!(declaration_groups(&term), vec![term]);
        }
    }

    #[test]
    fn empty_styling_cannot_hide_the_first_parameter_or_alias() {
        for spacer in [
            Inline::Text {
                value: " \t".into(),
            },
            Inline::Code {
                value: String::new(),
            },
            Inline::Code {
                value: " \t".into(),
            },
            Inline::Strong { children: vec![] },
            Inline::Strong {
                children: vec![Inline::Text { value: " ".into() }],
            },
            Inline::Strong {
                children: vec![
                    Inline::Code { value: " ".into() },
                    Inline::anchor("invisible"),
                ],
            },
            Inline::Emphasis {
                children: vec![Inline::Text { value: " ".into() }],
            },
            Inline::anchor("invisible"),
            Inline::line_break(),
        ] {
            for separator in [",", "|"] {
                for parameter in [false, true] {
                    let text = Inline::Text {
                        value: "-NUM".into(),
                    };
                    let name = if parameter {
                        Inline::Emphasis {
                            children: vec![text],
                        }
                    } else {
                        Inline::Strong {
                            children: vec![text],
                        }
                    };
                    let term = vec![
                        Inline::Strong {
                            children: vec![Inline::Text {
                                value: format!("-n{separator}"),
                            }],
                        },
                        spacer.clone(),
                        name,
                    ];
                    assert_eq!(
                        super::super::option_names_from_terms(&[term]),
                        if parameter {
                            vec!["-n"]
                        } else {
                            vec!["-n", "-NUM"]
                        },
                        "{spacer:?}: {separator}: parameter={parameter}"
                    );
                }
            }
        }
    }
}
