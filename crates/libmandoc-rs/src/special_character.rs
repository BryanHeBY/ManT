//! Lookup for the special-character catalog shipped by pinned libmandoc.

mod generated {
    include!(concat!(env!("OUT_DIR"), "/special_characters.rs"));
}

/// Semantic result of resolving a named roff character.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialCharacter {
    /// A Unicode scalar from the pinned catalog, including formatting
    /// scalars such as U+200B. Terminal cell behavior is a separate contract.
    Visible(char),
    /// A formatter control that intentionally occupies no visible width.
    ZeroWidth,
}

/// Resolve a roff named special character through the catalog compiled into
/// this exact libmandoc version.
///
/// The input is the catalog name without `\(`, `\[`, or `\C` delimiters.
/// Names unknown to the catalog return `None`; known zero-codepoint controls
/// remain distinguishable from unknown names. This lookup neither validates
/// source delimiters nor selects a terminal device's buffer-cell semantics.
#[must_use]
pub fn special_character(name: &str) -> Option<SpecialCharacter> {
    match generated::lookup(name)? {
        0 => Some(SpecialCharacter::ZeroWidth),
        codepoint => char::from_u32(codepoint).map(SpecialCharacter::Visible),
    }
}

#[cfg(test)]
mod tests {
    use super::{SpecialCharacter, special_character};

    #[test]
    fn exposes_the_complete_pinned_mandoc_catalog() {
        for (name, expected) in [
            ("at", '@'),
            ("ga", '`'),
            ("oq", '‘'),
            ("->", '→'),
            ("<-", '←'),
            ("mu", '×'),
            ("lB", '['),
            ("rB", ']'),
            ("a\"", '˝'),
            ("'", '´'),
        ] {
            assert_eq!(
                special_character(name),
                Some(SpecialCharacter::Visible(expected)),
                "name={name}"
            );
        }
        // New pinned chars.c maps ':' to U+200B for HTML; pristine UTF-8
        // term_word() instead buffers ASCII_BREAK for the direct escape.
        assert_eq!(
            special_character(":"),
            Some(SpecialCharacter::Visible('\u{200b}'))
        );
        assert_eq!(special_character("not-a-mandoc-character"), None);
    }
}
