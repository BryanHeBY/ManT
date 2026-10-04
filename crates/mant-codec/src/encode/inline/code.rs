//! Backtick framing for inline code and preformatted blocks.

pub(in crate::encode) fn fenced_code(value: &str, language: Option<&str>) -> String {
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
    format!("{fence}{language}\n{value}\n{fence}")
}

pub(crate) fn code_span(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 4);
    append_code_span(&mut output, value);
    output
}

pub(super) fn append_code_span(output: &mut String, value: &str) {
    let width = longest_backtick_run(value).saturating_add(1).max(1);
    let padding = (value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']))
        && !value.bytes().all(|byte| byte == b' ');
    for _ in 0..width {
        output.push('`');
    }
    if padding {
        output.push(' ');
    }
    output.push_str(value);
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
