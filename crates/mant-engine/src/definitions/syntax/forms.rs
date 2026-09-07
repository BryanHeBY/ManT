//! Keep alias separators and parameter styling distinct until name extraction.
use mant_ir::Inline;

/// Split explicit alias separators without flattening argument spans. A generic
/// strong/code run can contain several names, whereas punctuation inside an
/// emphasized argument is not evidence for another alias.
pub(super) fn alias_groups(term: &[Inline]) -> Vec<Vec<Inline>> {
    let mut groups = vec![Vec::new()];
    for inline in term {
        let parts = match inline {
            Inline::Text { value } => value
                .split([',', '|'])
                .map(|value| {
                    vec![Inline::Text {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Code { value } => value
                .split([',', '|'])
                .map(|value| {
                    vec![Inline::Code {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Strong { children } => alias_groups(children)
                .into_iter()
                .map(|children| vec![Inline::Strong { children }])
                .collect(),
            _ => vec![vec![inline.clone()]],
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

pub(super) fn starts_with_parameter(term: &[Inline]) -> bool {
    first_content_is_parameter(term).unwrap_or(false)
}

/// Locate content, not merely a wrapper: separators may leave empty strong
/// runs and anchors before the argument. Preserve emphasis ancestry instead
/// of flattening text and losing the distinction between a name and a value.
fn first_content_is_parameter(term: &[Inline]) -> Option<bool> {
    term.iter().find_map(|inline| match inline {
        Inline::Anchor { .. } | Inline::LineBreak => None,
        Inline::Text { value } | Inline::Code { value } => {
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
    fn empty_styling_cannot_hide_the_first_parameter_or_alias() {
        for spacer in [
            Inline::Text {
                value: " \t".into(),
            },
            Inline::Code { value: "".into() },
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
            Inline::LineBreak,
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
                    let term = vec![Inline::Strong {
                        children: vec![
                            Inline::Text {
                                value: format!("-n{separator}"),
                            },
                            spacer.clone(),
                            name,
                        ],
                    }];
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
