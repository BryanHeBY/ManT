//! Shared native-manual selector semantics.

/// Return whether a value is a conventional native manual section.
///
/// Numeric sections may carry an ASCII-alphanumeric extension such as `1p`
/// or `3type`; the historical single-letter `l` and `n` sections are also
/// accepted. The length bound keeps selectors finite and matches the public
/// request boundary.
#[must_use]
pub fn is_manual_section(value: &str) -> bool {
    if value.is_empty() || value.len() > 16 {
        return false;
    }
    let mut characters = value.chars();
    match characters.next() {
        Some(first) if first.is_ascii_digit() => {
            characters.all(|character| character.is_ascii_alphanumeric())
        }
        Some('l' | 'n') => characters.next().is_none(),
        _ => false,
    }
}

/// Return whether a manual section belongs to a command-page family.
///
/// Sections `1` and `8`, including conventional extensions such as `1p`, are
/// eligible for a tldr command quick reference. Other manual categories
/// describe APIs, formats, devices, games, or miscellaneous concepts.
#[must_use]
pub fn is_command_manual_section(value: &str) -> bool {
    if !is_manual_section(value) || !matches!(value.as_bytes().first(), Some(b'1' | b'8')) {
        return false;
    }
    let suffix = &value[1..];
    suffix.is_empty()
        || suffix
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
}

/// Split the `name(section)` spelling accepted by manual readers.
#[must_use]
pub fn parenthesized_manual_reference(selector: &str) -> Option<(&str, &str)> {
    if selector.contains(['/', '\\']) || !selector.ends_with(')') {
        return None;
    }
    let opening = selector.rfind('(')?;
    let name = &selector[..opening];
    let section = &selector[opening + 1..selector.len() - 1];
    let valid_name = !name.is_empty()
        && name.chars().all(|character| {
            !character.is_whitespace() && !character.is_control() && !matches!(character, '(' | ')')
        });
    (valid_name && is_manual_section(section)).then_some((name, section))
}

#[cfg(test)]
mod tests {
    use super::{is_command_manual_section, is_manual_section, parenthesized_manual_reference};

    #[test]
    fn recognizes_conventional_sections_and_command_families() {
        for section in ["0", "1", "1p", "3type", "8x", "l", "n"] {
            assert!(is_manual_section(section), "{section}");
        }
        for section in ["1", "1p", "8", "8x"] {
            assert!(is_command_manual_section(section), "{section}");
        }
        for section in ["", "qgroup", "17!", "3-type", "ll"] {
            assert!(!is_manual_section(section), "{section}");
        }
        for section in ["0", "10", "17", "3", "5", "l", "n"] {
            assert!(!is_command_manual_section(section), "{section}");
        }
    }

    #[test]
    fn splits_parenthesized_manual_references_without_paths() {
        assert_eq!(
            parenthesized_manual_reference("systemd.slice(5)"),
            Some(("systemd.slice", "5"))
        );
        assert_eq!(parenthesized_manual_reference("manual/1/git"), None);
        assert_eq!(parenthesized_manual_reference("two words(1)"), None);
        assert_eq!(parenthesized_manual_reference("function(arg)(3)"), None);
    }
}
