//! Splits the argument text of roff requests flattened into source strings.

pub(in crate::mandoc) fn roff_macro_arguments(source: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut escaped = false;
    for character in source.chars() {
        if escaped {
            current.push('\\');
            current.push(character);
            escaped = false;
            started = true;
        } else if character == '\\' {
            escaped = true;
            started = true;
        } else if character == '"' {
            quoted = !quoted;
            started = true;
        } else if character.is_whitespace() && !quoted {
            if started {
                arguments.push(std::mem::take(&mut current));
                started = false;
            }
        } else {
            current.push(character);
            started = true;
        }
    }
    if escaped {
        current.push('\\');
    }
    if started {
        arguments.push(current);
    }
    arguments
}

#[cfg(test)]
mod tests {
    use super::roff_macro_arguments;

    #[test]
    fn preserves_quoted_groups_and_escaped_boundaries() {
        assert_eq!(
            roff_macro_arguments(r#""two words" plain\ value final"#),
            ["two words", r"plain\ value", "final"]
        );
    }

    #[test]
    fn preserves_a_trailing_escape_as_visible_input() {
        assert_eq!(roff_macro_arguments("value\\"), [r"value\"]);
    }
}
