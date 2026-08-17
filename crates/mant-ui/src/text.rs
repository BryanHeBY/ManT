//! Terminal-safe dynamic text shared by the interactive surfaces.

use std::borrow::Cow;

pub(crate) fn sanitize_terminal_text(value: &str) -> Cow<'_, str> {
    if !value.chars().any(char::is_control) {
        return Cow::Borrowed(value);
    }
    Cow::Owned(
        value
            .chars()
            .map(|character| {
                if character.is_control() {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_text;

    #[test]
    fn replaces_terminal_controls_without_changing_unicode_text() {
        assert_eq!(sanitize_terminal_text("safe → text"), "safe → text");
        assert_eq!(
            sanitize_terminal_text("bad\u{1b}[31m\nname"),
            "bad�[31m�name"
        );
    }
}
