//! Lightweight, language-neutral highlighting for manual-page displays.
//!
//! Most roff displays do not carry a language name, so a full syntax grammar
//! cannot be selected reliably. This tokenizer mirrors the established TUI's
//! useful visual cues while preserving every source character and inline
//! modifier. Language-aware highlighting can be layered on top later.

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use crate::theme;

const KEYWORDS: &[&str] = &[
    "break", "case", "char", "const", "continue", "do", "double", "else", "enum", "extern",
    "false", "float", "for", "if", "inline", "int", "long", "null", "NULL", "restrict", "return",
    "short", "signed", "sizeof", "static", "struct", "switch", "true", "typedef", "union",
    "unsigned", "void", "volatile", "while",
];

/// Highlight display spans without changing their text or existing emphasis.
pub fn highlight(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .flat_map(|span| highlight_text(span.content.as_ref(), span.style))
        .collect()
}

fn highlight_text(value: &str, base: Style) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < value.len() {
        let rest = &value[offset..];
        let (length, style) = next_token(rest, base);
        result.push(Span::styled(rest[..length].to_owned(), style));
        offset += length;
    }
    result
}

fn next_token(value: &str, base: Style) -> (usize, Style) {
    if value.starts_with("//") || value.starts_with("/*") {
        return (
            value.len(),
            base.fg(theme::SUBTEXT).add_modifier(Modifier::ITALIC),
        );
    }
    let first = value
        .chars()
        .next()
        .expect("called only for non-empty text");
    if first.is_whitespace() {
        return (take_while(value, char::is_whitespace), base);
    }
    if matches!(first, '"' | '\'') {
        return (quoted_length(value, first), base.fg(theme::BLUE));
    }
    if first == '-'
        && value
            .chars()
            .nth(1)
            .is_some_and(|character| character.is_alphabetic() || character == '-')
    {
        return (
            take_while(value, |character| !character.is_whitespace()),
            base.fg(theme::HEADING),
        );
    }
    if first.is_ascii_digit() {
        return (
            take_while(value, |character| {
                character.is_ascii_digit() || character == '.'
            }),
            base.fg(theme::YELLOW),
        );
    }
    if first.is_alphabetic() || first == '_' {
        let length = take_while(value, |character| {
            character.is_alphanumeric() || character == '_'
        });
        let token = &value[..length];
        let style = if KEYWORDS.contains(&token) {
            base.fg(theme::MAUVE).add_modifier(Modifier::BOLD)
        } else {
            base
        };
        return (length, style);
    }
    (first.len_utf8(), base)
}

fn take_while(value: &str, predicate: impl Fn(char) -> bool) -> usize {
    value
        .char_indices()
        .find_map(|(index, character)| (!predicate(character)).then_some(index))
        .unwrap_or(value.len())
}

fn quoted_length(value: &str, quote: char) -> usize {
    let mut escaped = false;
    for (index, character) in value.char_indices().skip(1) {
        if character == quote && !escaped {
            return index + character.len_utf8();
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    value.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_common_manual_display_tokens_without_changing_text() {
        let source = "gcc --output file.c && return 12; // done";
        let spans = highlight(vec![Span::raw(source.to_owned())]);

        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            source
        );
        assert!(
            spans.iter().any(|span| {
                span.content == "--output" && span.style.fg == Some(theme::HEADING)
            })
        );
        assert!(
            spans
                .iter()
                .any(|span| { span.content == "return" && span.style.fg == Some(theme::MAUVE) })
        );
        assert!(
            spans
                .iter()
                .any(|span| { span.content == "12" && span.style.fg == Some(theme::YELLOW) })
        );
    }
}
