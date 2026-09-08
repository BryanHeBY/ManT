//! Whole-head acceptance for layout-inferred declarations, never body search.
//!
//! Explicit definition tags already supply an author-owned boundary. A plain
//! paragraph needs stronger evidence before an indented successor can become
//! its description: every part must be explainable as declaration syntax.

use mant_ir::Inline;

use super::{commands, forms, named, options};
use crate::{definitions::context::DefinitionContext, inline::plain_text};

pub(in crate::definitions) fn is_inferred_head(
    inlines: &[Inline],
    context: DefinitionContext,
) -> bool {
    // Candidate syntax precedes final role: a complete flag declaration does
    // not stop being a candidate under a configuration/value heading.
    let groups = forms::declaration_groups(inlines);
    if !groups.is_empty()
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

fn is_command_head(inlines: &[Inline]) -> bool {
    let Some(name) = commands::leading_styled_command_name(inlines) else {
        return false;
    };
    let mut literal = String::new();
    append_syntax(inlines, &mut literal);
    let Some(tail) = literal.trim_start().strip_prefix(&name) else {
        return false;
    };
    arguments(tail.split_whitespace())
}

fn is_option_head(inlines: &[Inline]) -> bool {
    let mut literal = String::new();
    append_syntax(inlines, &mut literal);
    if let Some([_, (name, start)]) = forms::paired_option_tokens(inlines) {
        return literal
            .get(start + name.len()..)
            .is_some_and(|tail| arguments(tail.split_whitespace()));
    }
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
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
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
            Inline::LineBreak => output.push('\n'),
            Inline::Anchor { .. } => {}
        }
    }
}
