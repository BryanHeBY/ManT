//! Lookup for the special-character catalog shipped by pinned libmandoc.

mod generated {
    include!(concat!(env!("OUT_DIR"), "/special_characters.rs"));
}

/// Semantic result of resolving a named roff character.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialCharacter {
    /// A printable Unicode scalar from the pinned catalog.
    Visible(char),
    /// A formatter control that intentionally occupies no visible width.
    ZeroWidth,
}

/// Resolve a roff named special character through the catalog compiled into
/// this exact libmandoc version.
///
/// The input is the name without `\(`, `\[`, or `\C` delimiters. Zero-width
/// Names unknown to the pinned parser return `None`; known zero-width controls
/// remain distinguishable from unknown names.
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
        assert_eq!(special_character(":"), Some(SpecialCharacter::ZeroWidth));
        assert_eq!(special_character("not-a-mandoc-character"), None);
    }
}
