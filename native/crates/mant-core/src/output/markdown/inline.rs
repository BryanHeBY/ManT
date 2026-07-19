//! Converts renderer-neutral inline nodes to safe `CommonMark` phrasing.

use mant_ast::Inline;

pub(super) fn render_inline(children: &[Inline]) -> String {
    render_inline_raw(children)
        .split('\n')
        .map(|line| line.trim_matches([' ', '\t']))
        .filter(|line| !line.is_empty())
        .map(protect_block_prefix)
        .collect::<Vec<_>>()
        .join("  \n")
}

pub(super) fn flatten_inline(children: &[Inline]) -> String {
    let mut output = String::new();
    for child in children {
        match child {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => {
                output.push_str(&flatten_inline(children));
            }
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}

pub(super) fn escape_text(value: &str) -> String {
    let mut output = String::new();
    let mut remainder = value;
    while let Some((start, opening_width)) = find_angle_url(remainder) {
        output.push_str(&escape_plain_text(&remainder[..start]));
        let after_open = &remainder[start + opening_width..];
        let closing = if opening_width == 2 { ">>" } else { ">" };
        let Some(end) = after_open.find(closing) else {
            output.push_str(&escape_plain_text(&remainder[start..]));
            return output;
        };
        let url = &after_open[..end];
        if url.chars().any(char::is_whitespace) || url.contains(['<', '>']) {
            output.push_str(&escape_plain_text(&remainder[start..start + opening_width]));
            remainder = after_open;
            continue;
        }
        output.push('<');
        output.push_str(url);
        output.push('>');
        remainder = &after_open[end + closing.len()..];
    }
    output.push_str(&escape_plain_text(remainder));
    output
}

pub(super) fn fenced_code(value: &str, language: Option<&str>) -> String {
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
    let boundary = if value.ends_with('\n') { "" } else { "\n" };
    format!("{fence}{language}\n{value}{boundary}{fence}")
}

pub(super) fn code_span(value: &str) -> String {
    let width = longest_backtick_run(value).saturating_add(1).max(1);
    let delimiter = "`".repeat(width);
    let padding = value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']);
    if padding {
        format!("{delimiter} {value} {delimiter}")
    } else {
        format!("{delimiter}{value}{delimiter}")
    }
}

fn render_inline_raw(children: &[Inline]) -> String {
    let mut output = String::new();
    for child in children {
        match child {
            Inline::Text { value } => output.push_str(&escape_text(value)),
            Inline::Strong { children } => {
                output.push_str(&render_styled(children, "**"));
            }
            Inline::Emphasis { children } => {
                output.push_str(&render_styled(children, "*"));
            }
            Inline::Code { value } => output.push_str(&code_span(value)),
            Inline::ExternalLink {
                uri,
                title,
                children,
            } => output.push_str(&render_link(uri, title.as_deref(), children)),
            Inline::EmailLink { address, children } => {
                output.push_str(&render_link(&format!("mailto:{address}"), None, children));
            }
            Inline::ManualReference { children, .. } => {
                output.push_str(&render_inline_raw(children));
            }
            Inline::SectionReference { target, children } => {
                output.push_str(&render_link(&format!("#{target}"), None, children));
            }
            Inline::Anchor { id } => output.push_str(&html_anchor(id)),
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}

fn render_styled(children: &[Inline], marker: &str) -> String {
    let rendered = render_inline_raw(children);
    let core = rendered.trim_matches([' ', '\t']);
    if core.is_empty() {
        return rendered;
    }
    let leading_width = rendered.len() - rendered.trim_start_matches([' ', '\t']).len();
    let trailing_width = rendered.len() - rendered.trim_end_matches([' ', '\t']).len();
    format!(
        "{}{marker}{core}{marker}{}",
        &rendered[..leading_width],
        &rendered[rendered.len() - trailing_width..]
    )
}

fn render_link(target: &str, title: Option<&str>, children: &[Inline]) -> String {
    let label = render_inline_raw(children);
    if (target.starts_with("http://") || target.starts_with("https://"))
        && flatten_inline(children) == target
        && !target.chars().any(char::is_whitespace)
        && !target.contains(['<', '>'])
    {
        return format!("<{target}>");
    }
    let target = target
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace(' ', "%20");
    title.map_or_else(
        || format!("[{label}]({target})"),
        |title| format!("[{label}]({target} \"{}\")", title.replace('"', "\\\"")),
    )
}

pub(super) fn html_anchor(id: &str) -> String {
    format!("<a id=\"{}\"></a>", escape_html_attribute(id))
}

fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_plain_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '*' | '_' | '[' | ']' | '<' | '>') {
            output.push('\\');
        }
        output.push(character);
    }
    output
}

fn protect_block_prefix(line: &str) -> String {
    let bytes = line.as_bytes();
    let hashes = bytes.iter().take_while(|byte| **byte == b'#').count();
    let insertion = if (hashes > 0 && bytes.get(hashes).is_none_or(u8::is_ascii_whitespace))
        || bytes.starts_with(b">")
        || bytes.starts_with(b"- ")
        || bytes.starts_with(b"+ ")
        || bytes.starts_with(b"* ")
        || (bytes.len() >= 3 && bytes.iter().all(|byte| *byte == b'-'))
    {
        Some(0)
    } else {
        let digits = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        (digits > 0
            && bytes
                .get(digits..digits.saturating_add(2))
                .is_some_and(|suffix| matches!(suffix, b". " | b") ")))
        .then_some(digits)
    };
    insertion.map_or_else(
        || line.to_owned(),
        |width| format!("{}\\{}", &line[..width], &line[width..]),
    )
}

fn find_angle_url(value: &str) -> Option<(usize, usize)> {
    [
        ("<<http://", 2),
        ("<<https://", 2),
        ("<http://", 1),
        ("<https://", 1),
    ]
    .into_iter()
    .filter_map(|(needle, width)| value.find(needle).map(|index| (index, width)))
    .min_by_key(|(index, width)| (*index, usize::MAX - *width))
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
