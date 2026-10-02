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
    let width = longest_backtick_run(value).saturating_add(1).max(1);
    let delimiter = "`".repeat(width);
    let padding = (value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']))
        && !value.chars().all(|character| character == ' ');
    if padding {
        format!("{delimiter} {value} {delimiter}")
    } else {
        format!("{delimiter}{value}{delimiter}")
    }
}

fn longest_backtick_run(value: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in value.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}
