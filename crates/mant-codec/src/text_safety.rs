//! Shared masking for renderer-visible, terminal-unsafe control characters.

use std::borrow::Cow;

/// Preserve layout controls used by source formats and mask every other
/// Unicode control character with spaces of the same UTF-8 byte length.
pub(crate) fn mask_terminal_controls(source: &str) -> (Option<String>, usize) {
    if source.chars().all(is_safe_source_character) {
        return (None, 0);
    }
    let mut masked = String::with_capacity(source.len());
    let mut controls = 0;
    for character in source.chars() {
        if is_safe_source_character(character) {
            masked.push(character);
        } else {
            controls += 1;
            masked.extend(std::iter::repeat_n(' ', character.len_utf8()));
        }
    }
    (Some(masked), controls)
}

#[cfg(feature = "roff")]
pub(crate) fn push_terminal_safe(target: &mut String, character: char) {
    if is_safe_source_character(character) {
        target.push(character);
    } else {
        target.extend(std::iter::repeat_n(' ', character.len_utf8()));
    }
}

#[cfg(any(feature = "roff", test))]
pub(crate) fn mask_terminal_control_bytes(source: &[u8]) -> (Cow<'_, [u8]>, usize) {
    let controls = source
        .iter()
        .filter(|byte| byte.is_ascii_control() && !matches!(byte, b'\t' | b'\n' | b'\r'))
        .count();
    if controls == 0 {
        return (Cow::Borrowed(source), 0);
    }
    let mut masked = source.to_vec();
    for byte in &mut masked {
        if byte.is_ascii_control() && !matches!(*byte, b'\t' | b'\n' | b'\r') {
            *byte = b' ';
        }
    }
    (Cow::Owned(masked), controls)
}

pub(crate) fn is_safe_source_character(character: char) -> bool {
    !character.is_control() || matches!(character, '\t' | '\n' | '\r')
}

/// Checked Markdown projection of code line endings. Code spans interpret
/// each CR, LF or CRLF as one space; fenced blocks interpret CR/CRLF as LF.
/// This never mutates the original hand-built IR or pretends the projection
/// can retain code scalars that `CommonMark` itself normalizes.
pub(crate) fn markdown_code_text(source: &str, inline: bool) -> Cow<'_, str> {
    if source.chars().all(is_safe_source_character)
        && !source.contains('\r')
        && (!inline || !source.contains('\n'))
    {
        return Cow::Borrowed(source);
    }
    let mut normalized = String::with_capacity(source.len());
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            normalized.push(if inline { ' ' } else { '\n' });
        } else if inline && character == '\n' {
            normalized.push(' ');
        } else if is_safe_source_character(character) {
            normalized.push(character);
        } else {
            normalized.extend(std::iter::repeat_n(' ', character.len_utf8()));
        }
    }
    Cow::Owned(normalized)
}

#[cfg(test)]
mod tests {
    use super::{mask_terminal_control_bytes, mask_terminal_controls};

    #[test]
    fn masks_controls_without_changing_source_offsets() {
        let source = "a\u{1b}[2J\u{85}b\n\t";
        let (masked, count) = mask_terminal_controls(source);
        let masked = masked.expect("unsafe source");
        assert_eq!(count, 2);
        assert_eq!(masked.len(), source.len());
        assert_eq!(masked, "a [2J  b\n\t");
    }

    #[test]
    fn masks_control_bytes_before_native_parsing() {
        let source = b"roff\x1b[2J\n\t";
        let (masked, count) = mask_terminal_control_bytes(source);
        assert_eq!(count, 1);
        assert_eq!(masked.as_ref(), b"roff [2J\n\t");
    }
}
