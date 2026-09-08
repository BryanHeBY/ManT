//! commands recognition; complete forms retain their role-specific grammar.
use crate::inline::plain_text;
use mant_ir::Inline;

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
    let mut name = String::new();
    append_literal_head(term, &mut name);
    let name = styled_command_prefix(name.trim());
    is_command_name(name).then(|| name.to_owned())
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
            Inline::Code { value } => output.push_str(value),
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
fn styled_command_prefix(value: &str) -> &str {
    let mut after_space = false;
    for (offset, character) in value.char_indices() {
        if after_space && matches!(character, '-' | '+' | '/' | '[' | '<' | '{') {
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
            assert_eq!(
                leading_styled_command_name(&term).as_deref(),
                Some(expected)
            );
        }
    }
}
