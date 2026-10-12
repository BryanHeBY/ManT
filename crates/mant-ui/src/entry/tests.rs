use super::*;

fn role(text: &str, accents: &[Accent], byte: usize) -> Option<StyleRole> {
    let scalar = text[..byte].chars().count();
    accents
        .iter()
        .find(|accent| accent.range.contains(&scalar))
        .map(|accent| accent.role)
}

#[test]
fn signatures_distinguish_delimiters_assignment_and_choices() {
    let text = "--output[=<文件>] --mode=[fast|slow] call(arg){value} a|b [a||b]";
    let accents = accents(text, std::slice::from_ref(&(0..8)));
    for (byte, character) in text.char_indices() {
        let expected = match character {
            '[' | ']' | '<' | '>' | '(' | ')' | '{' | '}' => Some(StyleRole::EntryDelimiter),
            '=' => Some(StyleRole::EntryOperator),
            '|' if byte == text.find("fast|slow").unwrap() + 4 => Some(StyleRole::EntrySeparator),
            _ => None,
        };
        assert_eq!(
            role(text, &accents, byte),
            expected,
            "{character} at {byte}"
        );
    }
}

#[test]
fn names_quotes_escapes_urls_and_incomplete_pairs_stay_protected() {
    let text = "operator[]= '<literal>' \"[literal]=x\" \\[x\\] https://host/?q=[x]=y ssh://host/[x]=z file:///tmp/[x]=z --value=<未完 [mismatch>";
    let accents = accents(text, std::slice::from_ref(&(0..11)));
    assert_eq!(
        accents,
        vec![Accent {
            range: text[..text.find("--value=").unwrap()].chars().count() + 7
                ..text[..text.find("--value=").unwrap()].chars().count() + 8,
            role: StyleRole::EntryOperator
        }]
    );
}

#[test]
fn bounds_and_depth_fallback_are_atomic() {
    assert_eq!(
        accents(&format!("--x=<{}>", "x".repeat(MAX_FORM_BYTES)), &[]),
        []
    );
    assert_eq!(
        accents(
            &format!(
                "--x={}v{}",
                "[".repeat(MAX_DEPTH + 1),
                "]".repeat(MAX_DEPTH + 1)
            ),
            &[]
        ),
        []
    );
    assert_ne!(
        accents(
            &format!("--x={}v{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH)),
            &[]
        ),
        []
    );
}

#[test]
fn cached_roles_survive_inline_boundaries_and_custom_theme_values() {
    let text = "--x[=<多语言>]";
    let accents = accents(text, std::slice::from_ref(&(0..3)));
    let mut theme = crate::theme::DEFAULT_THEME.clone();
    theme.set_style(
        StyleRole::EntryDelimiter,
        Style::new().fg(ratatui::style::Color::Red),
    );
    let base = Style::new()
        .add_modifier(ratatui::style::Modifier::ITALIC | ratatui::style::Modifier::UNDERLINED);
    for split in text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
    {
        let mut cursor = AccentCursor::new(&accents);
        let mut spans = cursor.spans(&text[..split], base, &theme);
        spans.extend(cursor.spans(&text[split..], base, &theme));
        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            text
        );
        assert!(
            spans
                .iter()
                .all(|span| span.style.add_modifier == base.add_modifier)
        );
        assert!(
            spans
                .iter()
                .filter(|span| span.content.contains('[') || span.content.contains('<'))
                .all(|span| span.style.fg == Some(ratatui::style::Color::Red))
        );
    }
    let label = LabelStyles::new(text, accents);
    assert_eq!(
        label.role_at(text.find('>').unwrap()),
        Some(StyleRole::EntryDelimiter)
    );
}
