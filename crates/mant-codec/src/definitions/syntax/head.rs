//! Whole-head acceptance for layout-inferred declarations, never body search.
//!
//! Explicit definition tags already supply an author-owned boundary. A plain
//! paragraph needs stronger evidence before an indented successor can become
//! its description: every part must be explainable as declaration syntax.

use mant_ir::Inline;

use super::{commands, forms, named, options};
use crate::definitions::context::DefinitionContext;
use mant_ir::inline_plain_text as plain_text;

pub(in crate::definitions) fn is_inferred_head(
    inlines: &[Inline],
    context: DefinitionContext,
) -> bool {
    let options = super::scan::option_head(inlines, &[]);
    inferred_head_with_options(inlines, context, &options)
}

fn inferred_head_with_options(
    inlines: &[Inline],
    context: DefinitionContext,
    options: &super::scan::OptionHead,
) -> bool {
    if options.inferred_complete {
        return true;
    }
    // Candidate syntax precedes final role: a complete flag declaration does
    // not stop being a candidate under a configuration/value heading.
    let groups = forms::declaration_groups(inlines);
    if options.names.is_empty()
        && !groups.is_empty()
        && groups.iter().enumerate().all(|(index, group)| {
            is_option_head(group)
                || (index > 0 && index + 1 == groups.len() && plain_text(group).trim() == "...")
        })
    {
        return true;
    }
    let text = plain_text(inlines);
    if named::local_configuration_head(
        &text,
        matches!(
            context,
            DefinitionContext::Variables | DefinitionContext::ConfigurationKeys
        ),
    ) {
        return true;
    }
    match context {
        DefinitionContext::EnvironmentVariables => named::environment_occurrences(&text).is_some(),
        DefinitionContext::Commands => forms::declaration_groups(inlines)
            .iter()
            .all(|group| is_command_head(group)),
        DefinitionContext::ConfigurationKeys => {
            named::named_occurrences(&text, named::is_configuration_key).is_some()
                && (commands::leading_styled_command_name(inlines).is_some()
                    || text.contains(['.', '=']))
        }
        DefinitionContext::Variables => {
            named::named_occurrences(&text, named::is_variable_term).is_some()
                && commands::leading_styled_command_name(inlines).is_some()
        }
        DefinitionContext::Generic | DefinitionContext::Parameters | DefinitionContext::Values => {
            false
        }
    }
}

/// Recognize an inferred owner without cloning its original inline tree.
/// This stronger admission is shared with native HP/headless-IP recovery;
/// explicit definition tags continue to use their own author-owned boundary.
#[cfg(test)]
fn recognize_inferred_head(
    inlines: &[Inline],
    context: DefinitionContext,
) -> Option<super::InferredIdentity> {
    recognize_inferred_head_with_operands(inlines, context, &[])
}

pub(in crate::definitions) fn recognize_inferred_head_with_operands(
    inlines: &[Inline],
    context: DefinitionContext,
    operands: &[crate::definitions::NativeOperand],
) -> Option<super::InferredIdentity> {
    let options = super::scan::option_head(inlines, operands);
    if !inferred_head_with_options(inlines, context, &options) {
        return None;
    }
    let text = plain_text(inlines);
    let (kind, case) = if options.inferred_complete {
        (
            mant_ir::EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            mant_ir::NameCase::Sensitive,
        )
    } else {
        super::decision::select_kind(text.trim(), context, None)
    };
    let occurrences = match kind {
        mant_ir::EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        } => options.names,
        mant_ir::EntryKind::EnvironmentVariable => named::environment_occurrences(&text)?,
        mant_ir::EntryKind::ConfigurationKey => {
            named::named_occurrences(&text, named::is_configuration_key)?
        }
        mant_ir::EntryKind::Variable => named::named_occurrences(&text, named::is_variable_term)?,
        mant_ir::EntryKind::Command => {
            let name = commands::inferred_command_name(inlines)
                .or_else(|| commands::command_name_from_authored_form(&text).map(str::to_owned))?;
            let start = text.find(&name)?;
            vec![crate::definitions::RecognizedName::contiguous(&name, start)]
        }
        _ => return None,
    };
    let mut names = Vec::new();
    for found in &occurrences {
        if !names.contains(&found.name) {
            names.push(found.name.clone());
        }
    }
    (!names.is_empty()).then_some(super::InferredIdentity {
        kind,
        case,
        names,
        occurrences: vec![occurrences],
        limit: None,
    })
}

fn is_command_head(inlines: &[Inline]) -> bool {
    let manual_name = commands::manual_name(inlines);
    let matched_manual = manual_name.is_some();
    let Some(name) = manual_name.or_else(|| commands::leading_styled_command_name(inlines)) else {
        return false;
    };
    let mut literal = String::new();
    append_syntax(inlines, &mut literal);
    let Some(tail) = literal.trim_start().strip_prefix(&name) else {
        return false;
    };
    // Bold alone does not establish a command owner in an ordinary paragraph.
    // A real syntax tail or a typed manual reference supplies independent
    // evidence; explicit It/IP/TP tags need no such inferred-owner proof.
    (!tail.trim().is_empty() || matched_manual || compound_command_name(&name))
        && arguments(tail.split_whitespace().filter(|token| {
            // A command's literal invocation may contain an actual option
            // token. Option-declaration arguments use the stricter grammar
            // below: a dash-shaped parameter there is never another alias.
            options::option_prefix(token) != Some(*token)
        }))
}

fn compound_command_name(name: &str) -> bool {
    name.contains('-')
        && name
            .split('-')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

fn is_option_head(inlines: &[Inline]) -> bool {
    let mut literal = String::new();
    append_syntax(inlines, &mut literal);
    let mut tokens = literal.split_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };
    if matches!(first, "\0" | "-\0")
        && plain_text(inlines)
            .trim()
            .strip_prefix("-<")
            .and_then(|value| value.strip_suffix('>'))
            .is_some_and(named::is_variable_term)
    {
        return true;
    }
    if first.starts_with("-<") && first.ends_with('>') {
        return arguments(std::iter::once(&first[1..]).chain(tokens));
    }
    let Some(name) = options::option_prefix(first).or_else(|| {
        // Complete punctuation flags can establish a head even when another
        // spelling in the group supplies its currently recognized name.
        (first.len() == 2
            && first.starts_with('-')
            && first.as_bytes()[1].is_ascii_punctuation()
            && !first.ends_with(['=', '[', ']', '{', '}', '(', ')', '<', '>', ',', '|']))
        .then_some(first)
    }) else {
        return false;
    };
    let attached = &first[name.len()..];
    let supported_attachment = attached.is_empty()
        || attached.starts_with(['=', '[', '{', '<', '(', '\0'])
        || (attached.starts_with(':') && attached.contains(['\0', '<']));
    if !supported_attachment {
        return false;
    }
    arguments(
        (!attached.is_empty() && !attached.starts_with('='))
            .then_some(attached)
            .into_iter()
            .chain(tokens),
    )
}

fn arguments<'a>(tokens: impl Iterator<Item = &'a str>) -> bool {
    let mut closers = Vec::new();
    let mut bare_arguments = 0;
    for token in tokens {
        let inside = !closers.is_empty();
        for character in token.chars() {
            if let Some(closer) = match character {
                '[' => Some(']'),
                '{' => Some('}'),
                '<' => Some('>'),
                '(' => Some(')'),
                _ => None,
            } {
                if closers.len() >= 64 {
                    return false;
                }
                closers.push(closer);
            } else if matches!(character, ']' | '}' | '>' | ')') && closers.pop() != Some(character)
            {
                return false;
            }
        }
        if inside || token.starts_with(['[', '{', '<', '(']) {
            continue;
        }
        if token == "\u{0}" || token == "..." {
            continue;
        }
        if token.contains('\0')
            && token
                .chars()
                .all(|ch| ch == '\0' || ch.is_ascii_punctuation())
        {
            continue;
        }
        if token.chars().any(char::is_uppercase)
            && token
                .chars()
                .all(|c| c.is_uppercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
        {
            continue;
        }
        // An unstyled lower-case metavariable is common in generated man
        // pages. It must be a single whole token, not a prose suffix.
        if token.starts_with('/')
            || token
                .split_once('=')
                .is_some_and(|(name, value)| named::is_variable_term(name) && !value.is_empty())
            || (token.contains(':')
                && token
                    .split(':')
                    .all(|part| part == "\0" || named::is_variable_term(part)))
            || token
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
                && !token.starts_with('-')
        {
            bare_arguments += 1;
        } else {
            return false;
        }
    }
    closers.is_empty() && bare_arguments <= 1
}

/// Keep literal text exact while marking explicitly styled arguments as
/// opaque grammar tokens. This temporary acceptance string never enters IR,
/// forms, source bindings or rendered text.
fn append_syntax(inlines: &[Inline], output: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                output.push_str(value);
            }
            Inline::Strong { children } | Inline::Link { children, .. } => {
                append_syntax(children, output);
            }
            Inline::Emphasis { children } => {
                let value = plain_text(children);
                if value.trim().is_empty() {
                    output.push_str(&value);
                } else {
                    output.push('\0');
                }
            }
            Inline::LineBreak { .. } => output.push('\n'),
            Inline::Anchor { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manual(children: Vec<Inline>) -> Inline {
        Inline::Link {
            children,
            target: mant_ir::LinkTarget::Manual {
                name: "tool".into(),
                manual_section: Some("1".into()),
            },
            title: None,
        }
    }

    #[test]
    fn literal_command_invocations_need_syntax_but_bare_prose_does_not() {
        // Exact .B "launch -p [FILE]" / .RS ran pristine before this
        // assertion. pre_B selects a font; it does not define a command.
        // The complete source-neutral invocation supplies the extra proof.
        for (value, expected) in [
            ("launch -p [FILE]", true),
            ("launch --verbose FILE", true),
            ("launch -p [FILE", false),
            ("show-environment", true),
            ("list-units", true),
            ("list--units", false),
            ("list-units ordinary prose", false),
            ("Note", false),
            ("Note ordinary prose sentence", false),
        ] {
            let nodes = [Inline::Strong {
                children: vec![Inline::Text {
                    value: value.into(),
                }],
            }];
            assert_eq!(
                is_inferred_head(&nodes, DefinitionContext::Commands),
                expected,
                "{value}"
            );
            assert_eq!(
                recognize_inferred_head(&nodes, DefinitionContext::Commands).is_some(),
                expected,
                "{value}"
            );
        }
    }

    #[test]
    fn weak_manual_labels_need_matching_name_or_independent_invocation_syntax() {
        // Authored IR labels are independent of their destinations. A typed
        // target cannot turn an arbitrary styled label into a bare command.
        // This is a source-neutral naming rule, not native formatter geometry.
        for value in ["Note", "tool(2)", "tool(1)"] {
            for label in [
                Inline::Code {
                    value: value.into(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: value.into(),
                    }],
                },
            ] {
                let nodes = [manual(vec![label])];
                let expected = value == "tool(1)";
                assert_eq!(
                    is_inferred_head(&nodes, DefinitionContext::Commands),
                    expected,
                    "{value}"
                );
                assert_eq!(
                    recognize_inferred_head(&nodes, DefinitionContext::Commands).is_some(),
                    expected,
                    "{value}"
                );
            }
        }
        for name in [
            Inline::Code {
                value: "launch".into(),
            },
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "launch".into(),
                }],
            },
        ] {
            let nodes = [manual(vec![
                name,
                Inline::Text { value: " ".into() },
                Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "FILE".into(),
                    }],
                },
            ])];
            assert!(is_inferred_head(&nodes, DefinitionContext::Commands));
            assert!(recognize_inferred_head(&nodes, DefinitionContext::Commands).is_some());
        }
        let matching = manual(vec![Inline::Text {
            value: "tool(1)".into(),
        }]);
        let prose = [
            matching.clone(),
            Inline::Text {
                value: " ordinary prose sentence".into(),
            },
        ];
        assert!(!is_inferred_head(&prose, DefinitionContext::Commands));
        assert!(recognize_inferred_head(&prose, DefinitionContext::Commands).is_none());
        let invocation = [
            matching,
            Inline::Text { value: " ".into() },
            Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "FILE".into(),
                }],
            },
        ];
        assert!(is_inferred_head(&invocation, DefinitionContext::Commands));
        assert!(is_inferred_head(
            &[manual(vec![Inline::Text {
                value: "tool(1)".into()
            }])],
            DefinitionContext::Commands
        ));
    }
}
