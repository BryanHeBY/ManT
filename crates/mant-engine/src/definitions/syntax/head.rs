//! Whole-head acceptance for layout-inferred declarations, never body search.
//!
//! Explicit definition tags already supply an author-owned boundary. A plain
//! paragraph needs stronger evidence before an indented successor can become
//! its description: every part must be explainable as declaration syntax.

use mant_ir::Inline;

use super::{forms, named, options};
use crate::{definitions::context::DefinitionContext, inline::plain_text};

pub(in crate::definitions) fn is_inferred_head(
    inlines: &[Inline],
    context: DefinitionContext,
) -> bool {
    match context {
        DefinitionContext::EnvironmentVariables => {
            named::environment_occurrences(&plain_text(inlines)).is_some()
        }
        DefinitionContext::Generic | DefinitionContext::Parameters => {
            forms::declaration_groups(inlines)
                .iter()
                .all(|group| is_option_head(group))
        }
        _ => false,
    }
}

fn is_option_head(inlines: &[Inline]) -> bool {
    let mut literal = String::new();
    append_syntax(inlines, &mut literal);
    let mut tokens = literal.split_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };
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
    if !attached.is_empty() && !attached.starts_with(['=', '[', '{', '<', '(']) {
        return false;
    }
    let mut closers = Vec::new();
    let mut bare_arguments = 0;
    for token in (!attached.is_empty() && !attached.starts_with('='))
        .then_some(attached)
        .into_iter()
        .chain(tokens)
    {
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
        if token == "\u{0}" {
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
        if token
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
                if !plain_text(children).trim().is_empty() {
                    output.push_str(" \0 ");
                }
            }
            Inline::LineBreak => output.push('\n'),
            Inline::Anchor { .. } => {}
        }
    }
}
