//! commands recognition; complete forms retain their role-specific grammar.
use super::eligibility::{NameSource, OperandUses};
use crate::definitions::RecognizedName;
use mant_ir::Inline;
use mant_ir::inline_plain_text as plain_text;
use std::ops::Range;

/// A typed manual label can establish a directory command without font
/// styling. Its visible spelling must contain that exact command name;
/// destinations or arbitrary alternative labels are never lexical evidence.
pub(super) fn inferred_command_binding(
    term: &[Inline],
    text: &str,
    offset: usize,
    uses: OperandUses<'_>,
) -> Option<RecognizedName> {
    let name = manual_name(term)
        .or_else(|| styled_command_name(term, offset, uses))
        .or_else(|| command_name_from_authored_form(text).map(str::to_owned))?;
    command_binding(text, &name, offset, uses)
}

pub(super) fn command_binding(
    text: &str,
    name: &str,
    offset: usize,
    uses: OperandUses<'_>,
) -> Option<RecognizedName> {
    let start = text.find(name)?;
    let end = start + name.len();
    let end = start + before_symbol(text, start..end, uses.shifted(offset))?.len();
    // Strong styling can cut the prefix of one joined command template.
    // A separate argument or an explicit syntax delimiter still gives a
    // complete name. Check the rest of the joined token even when an
    // unstyled literal sits between the claimed name and the argument.
    let tail = &text[end..];
    let joined = tail
        .find(|character: char| {
            character.is_whitespace() || matches!(character, '[' | '<' | '{' | '(')
        })
        .unwrap_or(tail.len());
    if joined > 0 && uses.protects(&(offset + end..offset + end + joined)) {
        return None;
    }
    let binding = RecognizedName::contiguous(&text[start..end], offset + start);
    uses.permits(&binding, NameSource::Lexical)
        .then_some(binding)
}

pub(super) fn manual_name(nodes: &[Inline]) -> Option<String> {
    for node in nodes {
        match node {
            Inline::Anchor { .. } => {}
            Inline::Text { value } if value.trim().is_empty() => {}
            Inline::Strong { children } | Inline::Emphasis { children } => {
                return manual_name(children);
            }
            Inline::Link {
                children,
                target:
                    mant_ir::LinkTarget::Manual {
                        name,
                        manual_section,
                    },
                ..
            } => {
                let label = plain_text(children);
                let expected = format!("{name}({})", manual_section.as_deref().unwrap_or_default());
                return (label.trim() == expected && is_command_name(name)).then(|| name.clone());
            }
            _ => return None,
        }
    }
    None
}

/// Extract the command token from an unstyled authored form.
pub(in crate::definitions) fn command_name_from_authored_form(value: &str) -> Option<&str> {
    let value = value.trim();
    let Some((first, suffix)) = value.split_once(char::is_whitespace) else {
        return is_command_name(value).then_some(value);
    };
    let suffix = suffix.trim_start();
    if first == "." && super::named::is_variable_term(suffix) {
        return Some(first);
    }
    (suffix.starts_with(['-', '+', '/', '[', '<', '{']) && is_command_name(first)).then_some(first)
}

/// Read a formatter-emphasized command name without adjacent placeholders.
pub(in crate::definitions) fn leading_styled_command_name(term: &[Inline]) -> Option<String> {
    styled_command_name(term, 0, OperandUses::default())
}

fn styled_command_name(term: &[Inline], offset: usize, uses: OperandUses<'_>) -> Option<String> {
    let mut name = String::new();
    append_literal_head(term, &mut name);
    let start = name.len() - name.trim_start().len();
    let end = start + name.trim().len();
    let prefix = before_symbol(&name, start..end, uses.shifted(offset))?;
    let prefix = styled_command_prefix(prefix);
    is_command_name(prefix).then(|| prefix.to_owned())
}

fn before_symbol<'a>(text: &'a str, span: Range<usize>, uses: OperandUses<'_>) -> Option<&'a str> {
    #[cfg(feature = "roff")]
    if let Some(symbol) = uses.command_symbol_start(span.clone()) {
        let before = &text[span.start..symbol];
        let name = before.trim_end();
        // Separate native scopes can still contribute to one visible word.
        return (before.len() > name.len() || text[symbol..].starts_with(char::is_whitespace))
            .then_some(name);
    }
    #[cfg(not(feature = "roff"))]
    let _ = uses;
    Some(&text[span])
}

// Nm/Cm and adjacent font runs can form one multiword literal head. Preserve
// their actual whitespace and stop at the first argument or unstyled token;
// never scan later literals in the invocation for additional names.
fn append_literal_head(inlines: &[Inline], output: &mut String) -> bool {
    for inline in inlines {
        match inline {
            Inline::Anchor { .. } => {}
            Inline::Text { value } if value.chars().all(char::is_whitespace) => {
                output.push_str(value);
            }
            Inline::Strong { children } => output.push_str(&plain_text(children)),
            Inline::Code { value } | Inline::Equation { value, .. } => output.push_str(value),
            Inline::Link { children, .. } => {
                if !append_literal_head(children, output) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// A style run can include an entire invocation. Keep a multiword literal
/// head, but stop before an explicit option or placeholder token; styling
/// alone must not turn `launch -p [` into an addressable name.
pub(super) fn styled_command_prefix(value: &str) -> &str {
    let mut after_space = false;
    for (offset, character) in value.char_indices() {
        if after_space && matches!(character, '-' | '+' | '/' | '(' | '[' | '<' | '{') {
            return value[..offset].trim_end();
        }
        after_space = character.is_whitespace();
    }
    value
}

pub(in crate::definitions) fn is_command_name(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && !value.starts_with(['-', '+', '/'])
        && (matches!(value, "[" | "{") || !value.contains(['[', ']', '{', '}', '<', '>', '|', ',']))
        && !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_multiword_heads_stop_at_syntax_not_at_every_space() {
        for (form, expected) in [
            ("launch -p [", "launch"),
            ("query session -v", "query session"),
            ("Send Env", "Send Env"),
            ("Send Buffer", "Send Buffer"),
            ("[", "["),
            ("query user <NAME>", "query user"),
        ] {
            let term = [Inline::Strong {
                children: vec![Inline::Text { value: form.into() }],
            }];
            for content in [
                term.to_vec(),
                vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://example.invalid".into(),
                    },
                    title: None,
                    children: term.to_vec(),
                }],
            ] {
                assert_eq!(
                    leading_styled_command_name(&content).as_deref(),
                    Some(expected)
                );
            }
        }
    }
}
