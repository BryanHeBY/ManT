//! Whole-head acceptance for layout-inferred declarations, never body search.
//!
//! Explicit definition tags already supply an author-owned boundary. A plain
//! paragraph needs stronger evidence before an indented successor can become
//! its description: every part must be explainable as declaration syntax.
#![allow(clippy::similar_names)] // ContentContext and DefinitionContext are distinct inputs.

use mant_ir::{ContentContext, Inline, InlineView};

use super::{forms, named, options};
use crate::definitions::context::DefinitionContext;
use mant_ir::inline_plain_text as plain_text;

pub(in crate::definitions) fn is_inferred_head(
    content: ContentContext<'_>,
    inlines: &[Inline],
    context: DefinitionContext,
) -> bool {
    // Candidate syntax precedes final role: a complete flag declaration does
    // not stop being a candidate under a configuration/value heading.
    let groups = forms::declaration_groups(content, inlines);
    if !groups.is_empty()
        && groups.iter().enumerate().all(|(index, group)| {
            is_option_head(content, group)
                || (index > 0
                    && index + 1 == groups.len()
                    && plain_text(content, group).trim() == "...")
        })
    {
        return true;
    }
    let text = plain_text(content, inlines);
    if named::local_configuration_head(&text, context) {
        return true;
    }
    match context {
        DefinitionContext::EnvironmentVariables => named::environment_occurrences(&text)
            .is_some_and(|names| {
                let complete_assignment = text
                    .split_once('=')
                    .is_some_and(|(_, value)| !value.trim().is_empty());
                !names.is_empty() && (names.len() > 1 || text.contains('<') || complete_assignment)
            }),
        DefinitionContext::Commands => is_command_head(&text),
        DefinitionContext::Variables => {
            mant_ir::variable_assignment_declaration_range(&text).is_some()
        }
        // The complete dotted key or nonempty assignment above supplies
        // local syntax evidence. Heading + bold alone is not a declaration;
        // neither is a bare variable name under a VARIABLES title. CVS
        // man_term.c::pre_PP/pre_RS only establishes a layout continuation.
        DefinitionContext::ConfigurationKeys
        | DefinitionContext::Generic
        | DefinitionContext::RootConfigurationKeys
        | DefinitionContext::Parameters
        | DefinitionContext::Values => false,
    }
}

fn is_command_head(text: &str) -> bool {
    // man_term.c::pre_PP/pre_RS establish only layout and the continuation.
    // A bold word alone is not a command declaration: the complete visible
    // head also needs a manual-call, parameter, or real key-binding syntax.
    mant_ir::command_declaration_name_range(text).is_some()
}

/// Executed italic argument runs supply argument boundaries for a native
/// definition HEAD. They are not command names. Feed one opaque placeholder
/// per run to the shared complete-call grammar, then bind the returned prefix
/// only if its bytes are unchanged in the actual visible HEAD.
pub(super) fn is_styled_command_head(content: ContentContext<'_>, inlines: &[Inline]) -> bool {
    let mut syntax = String::new();
    append_syntax(content, inlines, &mut syntax);
    if !syntax.contains('\0') {
        return false;
    }
    let syntax = syntax.replace('\0', "<arg>");
    let Some(range) = mant_ir::command_declaration_name_range(&syntax) else {
        return false;
    };
    let visible = plain_text(content, inlines);
    visible.get(range.clone()) == syntax.get(range)
}

fn is_option_head(content: ContentContext<'_>, inlines: &[Inline]) -> bool {
    let mut literal = String::new();
    append_syntax(content, inlines, &mut literal);
    if let Some((_, range)) =
        mant_ir::literal_option_aliases(&forms::literal_prefix(content, inlines))
            .and_then(|names| names.into_iter().last())
    {
        return literal
            .get(range.end..)
            .is_some_and(|tail| arguments(tail.split_whitespace()));
    }
    let mut tokens = literal.split_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };
    if matches!(first, "\0" | "-\0")
        && plain_text(content, inlines)
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
fn append_syntax(content: ContentContext<'_>, inlines: &[Inline], output: &mut String) {
    for inline in inlines {
        match content.inline(inline).expect("definition content resolves") {
            InlineView::Text(value) | InlineView::Code(value) => output.push_str(value),
            InlineView::Strong(children) => {
                append_syntax(content, children, output);
            }
            InlineView::Link(link) => append_syntax(content, link.children(), output),
            InlineView::Emphasis(children) => {
                let value = plain_text(content, children);
                if value.trim().is_empty() {
                    output.push_str(&value);
                } else {
                    output.push('\0');
                }
            }
            InlineView::LineBreak => output.push('\n'),
            InlineView::Anchor(_) => {}
            _ => unreachable!("all inline views are handled"),
        }
    }
}
