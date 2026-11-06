//! Backtick framing for inline code and preformatted blocks.

pub(in crate::encode) fn fenced_code(value: &str, language: Option<&str>) -> String {
    let value = crate::text_safety::markdown_code_text(value, false);
    accepted_fenced_code(&value, language)
}

pub(in crate::encode) fn accepted_fenced_code(value: &str, language: Option<&str>) -> String {
    let (opening, closing) = code_fences(value, language);
    format!("{opening}{value}{closing}")
}

/// Syntax framing cannot reinterpret an already accepted payload. Callers
/// resolve its content role before constructing byte ownership ranges.
pub(in crate::encode) fn code_fences(value: &str, language: Option<&str>) -> (String, String) {
    let width = longest_backtick_run(value).saturating_add(1).max(3);
    let fence = "`".repeat(width);
    let language = language
        .map(|language| {
            language
                .chars()
                .filter(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '_')
                })
                .collect::<String>()
        })
        .filter(|language| !language.is_empty())
        .unwrap_or_default();
    // The reader removes exactly the framing newline before the closing
    // fence (markdown/layout.rs::trim_code_framing_newline). Give that
    // syntax its own byte; an authored final hard row belongs to the value.
    (format!("{fence}{language}\n"), format!("\n{fence}"))
}

pub(crate) fn code_span(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 4);
    append_code_span(&mut output, value);
    output
}

pub(super) fn append_code_span(output: &mut String, value: &str) {
    let value = crate::text_safety::markdown_code_text(value, true);
    let width = longest_backtick_run(&value).saturating_add(1).max(1);
    let padding = (value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']))
        && !value.bytes().all(|byte| byte == b' ');
    for _ in 0..width {
        output.push('`');
    }
    if padding {
        output.push(' ');
    }
    output.push_str(&value);
    if padding {
        output.push(' ');
    }
    for _ in 0..width {
        output.push('`');
    }
}

fn longest_backtick_run(value: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for byte in value.bytes() {
        if byte == b'`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    #[test]
    fn manual_code_projection_normalizes_line_endings_without_bare_controls() {
        for (value, span, literal) in [
            ("a\rb", "a b", "a\nb"),
            ("a\r\nb", "a b", "a\nb"),
            ("a\nb", "a b", "a\nb"),
            ("a\x0cb\x1bc\u{81}d", "a b c  d", "a b c  d"),
            (
                "a\u{a0}b\u{2028}c\u{2029}d\u{200b}e",
                "a\u{a0}b\u{2028}c\u{2029}d\u{200b}e",
                "a\u{a0}b\u{2028}c\u{2029}d\u{200b}e",
            ),
        ] {
            let code = super::code_span(value);
            let parsed = crate::parse_markdown(&code, None).unwrap().document;
            assert_eq!(parsed.diagnostics, []);
            let [mant_ir::Block::Paragraph { children, .. }] = parsed.blocks.as_slice() else {
                panic!("code paragraph")
            };
            assert!(
                matches!(children.as_slice(), [mant_ir::Inline::Code { value }] if value == span)
            );
            let fenced = super::fenced_code(value, Some("text"));
            let parsed = crate::parse_markdown(&fenced, None).unwrap().document;
            assert_eq!(parsed.diagnostics, []);
            let [mant_ir::Block::Preformatted { children, .. }] = parsed.blocks.as_slice() else {
                panic!("literal")
            };
            assert_eq!(mant_ir::inline_plain_text(children), literal);
        }
    }

    #[test]
    fn direct_code_writes_keep_backticks_padding_and_unicode_after_readback() {
        for (value, expected) in [
            ("a", "`a`"),
            ("中😀", "`中😀`"),
            (" ", "` `"),
            ("  ", "`  `"),
            (" a ", "`  a  `"),
            ("`", "`` ` ``"),
            ("a``中", "```a``中```"),
            ("中`", "`` 中` ``"),
        ] {
            let mut output = String::from("prefix");
            super::append_code_span(&mut output, value);
            assert_eq!(output, format!("prefix{expected}"));
            assert_eq!(super::code_span(value), expected);
            let codes: Vec<_> = pulldown_cmark::Parser::new(expected)
                .filter_map(|event| match event {
                    pulldown_cmark::Event::Code(code) => Some(code.into_string()),
                    _ => None,
                })
                .collect();
            assert_eq!(codes, [value], "{expected}");
        }
    }
}
