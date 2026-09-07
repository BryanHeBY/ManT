//! Keep alias separators and parameter styling distinct until name extraction.
use mant_ir::Inline;

use crate::inline::plain_text;

/// Borrowed authored syntax. Candidate generation never rewrites its complete
/// form; only the temporary selector candidates are split.
pub(super) struct AuthoredForm<'a> {
    inlines: &'a [Inline],
}

impl<'a> AuthoredForm<'a> {
    pub(super) const fn new(inlines: &'a [Inline]) -> Self {
        Self { inlines }
    }

    pub(super) fn option_candidates(&self) -> impl Iterator<Item = FormCandidate> {
        option_alias_groups(self.inlines)
            .into_iter()
            .scan(0, |offset, inlines| {
                let start = *offset;
                // All removed separators are one ASCII byte. Opaque argument
                // runs were not split, and remain part of this visible length.
                *offset += plain_text(&inlines).len() + 1;
                Some(FormCandidate { inlines, start })
            })
    }
}

/// Own the styled candidate until the parameter decision is complete. Callers
/// receive a lexical token only through that decision, not a flattenable tree.
pub(super) struct FormCandidate {
    inlines: Vec<Inline>,
    start: usize,
}

impl FormCandidate {
    pub(super) fn invocation_token(&self) -> Option<(String, usize)> {
        if starts_with_parameter(&self.inlines) {
            return None;
        }
        let mut prefix = String::new();
        append_name_prefix(&self.inlines, &mut prefix);
        let token = invocation_token(&prefix);
        if token.is_empty() {
            return None;
        }
        // `token` is the exact subslice selected by the grammar, including
        // removal of whitespace and authored enclosing punctuation.
        let offset = token.as_ptr() as usize - prefix.as_ptr() as usize;
        Some((token.to_owned(), self.start + offset))
    }
}

/// Read visible literal content only until an explicitly styled parameter.
/// Transparent wrappers do not erase that boundary, even without whitespace.
fn append_name_prefix(nodes: &[Inline], output: &mut String) -> bool {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
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
            Inline::LineBreak => output.push('\n'),
        }
    }
    true
}

/// Split explicit alias separators without flattening argument spans. A generic
/// strong/code run can contain several names, whereas punctuation inside an
/// emphasized argument is not evidence for another alias.
pub(super) fn alias_groups(term: &[Inline]) -> Vec<Vec<Inline>> {
    split_groups(term, &[',', '|'], &mut None)
}

/// Slashes only separate the invocation token after its option grammar has
/// been validated. Keep candidate trees intact so each candidate still passes
/// the parameter check; never split argument paths later in the form.
fn option_alias_groups(term: &[Inline]) -> Vec<Vec<Inline>> {
    alias_groups(term)
        .into_iter()
        .flat_map(|group| {
            let text = plain_text(&group);
            let token = invocation_token(&text);
            if super::slash_option_forms(token).is_none() {
                return vec![group];
            }
            // The token is a substring of text; the prefix can include blank
            // styling or opening brackets excluded by invocation_token.
            let Some(start) = text.find(token) else {
                return vec![group];
            };
            split_groups(&group, &['/'], &mut Some(start + token.len()))
        })
        .collect()
}

fn invocation_token(text: &str) -> &str {
    text.split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| {
            matches!(
                character,
                '[' | ']' | '(' | ')' | '{' | '}' | '“' | '”' | '‘' | '’'
            )
        })
}

/// One style-preserving splitter for alias punctuation. A bounded pass counts
/// visible bytes even in opaque arguments. Links preserve their wrapper while
/// exposing their visible children; their destination is never name evidence.
fn split_groups(
    term: &[Inline],
    separators: &[char],
    remaining: &mut Option<usize>,
) -> Vec<Vec<Inline>> {
    let mut groups = vec![Vec::new()];
    for inline in term {
        let parts = match inline {
            Inline::Text { value } => value
                .split(|character| take_separator(character, separators, remaining))
                .map(|value| {
                    vec![Inline::Text {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Code { value } => value
                .split(|character| take_separator(character, separators, remaining))
                .map(|value| {
                    vec![Inline::Code {
                        value: value.into(),
                    }]
                })
                .collect(),
            Inline::Strong { children } => split_groups(children, separators, remaining)
                .into_iter()
                .map(|children| vec![Inline::Strong { children }])
                .collect(),
            Inline::Link {
                target,
                title,
                children,
            } => split_groups(children, separators, remaining)
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

fn starts_with_parameter(term: &[Inline]) -> bool {
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
        let source = vec![link(vec![Inline::Strong {
            children: vec![
                text("-L"),
                Inline::Emphasis {
                    children: vec![text("dir,--FAKE")],
                },
                text(", --library"),
            ],
        }])];
        let original = source.clone();
        assert_eq!(
            super::super::option_names_from_terms(std::slice::from_ref(&source)),
            ["-L", "--library"]
        );
        assert_eq!(source, original);
        let argument = vec![Inline::Emphasis {
            children: vec![link(vec![text("-n,--FAKE")])],
        }];
        assert!(super::super::option_names_from_terms(&[argument]).is_empty());
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
                assert_eq!(super::super::option_names_from_terms(&[term]), ["-n"]);
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
            assert_eq!(alias_groups(&term), vec![term]);
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
