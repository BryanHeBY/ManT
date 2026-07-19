//! Parses the constrained tldr-pages Markdown dialect into the shared AST.

use std::{error::Error, fmt};

use mant_ast::{TldrCommandPart, TldrDocument, TldrExample};

/// Cache identity attached to a parsed tldr page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TldrPageLocation {
    pub platform: String,
    pub language: String,
    pub source_path: String,
}

/// A tldr page lacks the minimum structure required by the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TldrParseError {
    MissingCommandHeading,
}

impl fmt::Display for TldrParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCommandHeading => {
                formatter.write_str("tldr page is missing its command heading")
            }
        }
    }
}

impl Error for TldrParseError {}

/// Parse the tldr placeholder extension and choose the long option variant.
#[must_use]
pub fn parse_tldr_command(command: &str) -> Vec<TldrCommandPart> {
    let mut parts = Vec::new();
    let mut cursor = 0;

    while cursor < command.len() {
        let remainder = &command[cursor..];
        if let Some(escaped) = remainder.strip_prefix(r"\{\{") {
            if let Some(close) = escaped.find(r"\}\}") {
                push_part(
                    &mut parts,
                    PartKind::Text,
                    format!("{{{{{}}}}}", &escaped[..close]),
                );
                cursor += 4 + close + 4;
                continue;
            }
        }

        if let Some(placeholder) = remainder.strip_prefix("{{") {
            if let Some(close) = placeholder.find("}}") {
                let value = resolve_option_placeholder(&placeholder[..close])
                    .unwrap_or(&placeholder[..close]);
                push_part(&mut parts, PartKind::Placeholder, value.to_owned());
                cursor += 2 + close + 2;
                continue;
            }
        }

        let Some(character) = remainder.chars().next() else {
            break;
        };
        push_part(&mut parts, PartKind::Text, character.to_string());
        cursor += character.len_utf8();
    }

    parts
}

/// Parse one cached tldr Markdown page without performing any I/O.
///
/// # Errors
///
/// Returns [`TldrParseError::MissingCommandHeading`] when no `# command`
/// heading is present.
pub fn parse_tldr_page(
    markdown: &str,
    location: TldrPageLocation,
) -> Result<TldrDocument, TldrParseError> {
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let mut title = String::new();
    let mut description = Vec::new();
    let mut more_information = None;
    let mut examples = Vec::new();
    let mut pending_description = None;

    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if title.is_empty() {
            if let Some(heading) = trimmed.strip_prefix("# ") {
                title = flatten_markdown(heading);
                continue;
            }
        }

        if let Some(quote) = trimmed.strip_prefix('>') {
            let quote = flatten_markdown(quote);
            if let Some(value) = strip_prefix_ascii_case(&quote, "More information:") {
                let value = value.trim();
                if !value.is_empty() {
                    more_information = Some(value.to_owned());
                }
            } else if !quote.is_empty() {
                description.push(quote);
            }
            continue;
        }

        if let Some(item) = trimmed.strip_prefix("- ") {
            flush_pending(&mut pending_description, &mut examples);
            if let Some((example_description, command)) = extract_trailing_code(item) {
                examples.push(make_example(example_description, command));
            } else {
                let value = flatten_markdown(item.trim_end_matches(':'));
                if !value.is_empty() {
                    pending_description = Some(value);
                }
            }
            continue;
        }

        if let Some(command) = standalone_code(trimmed) {
            if let Some(example_description) = pending_description.take() {
                examples.push(make_example(example_description, command.to_owned()));
            }
        }
    }

    flush_pending(&mut pending_description, &mut examples);
    if title.is_empty() {
        return Err(TldrParseError::MissingCommandHeading);
    }

    Ok(TldrDocument {
        title,
        description,
        more_information,
        examples,
        platform: location.platform,
        language: location.language,
        source_path: location.source_path,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartKind {
    Text,
    Placeholder,
}

fn push_part(parts: &mut Vec<TldrCommandPart>, kind: PartKind, value: String) {
    if value.is_empty() {
        return;
    }
    match (parts.last_mut(), kind) {
        (Some(TldrCommandPart::Text { value: previous }), PartKind::Text)
        | (Some(TldrCommandPart::Placeholder { value: previous }), PartKind::Placeholder) => {
            previous.push_str(&value);
        }
        (_, PartKind::Text) => parts.push(TldrCommandPart::Text { value }),
        (_, PartKind::Placeholder) => parts.push(TldrCommandPart::Placeholder { value }),
    }
}

fn resolve_option_placeholder(value: &str) -> Option<&str> {
    let choices = value.strip_prefix('[')?.strip_suffix(']')?;
    let (_, long) = choices.split_once('|')?;
    (!long.is_empty()).then_some(long)
}

fn flush_pending(pending: &mut Option<String>, examples: &mut Vec<TldrExample>) {
    if let Some(description) = pending.take() {
        examples.push(make_example(description, String::new()));
    }
}

fn make_example(description: String, command: String) -> TldrExample {
    TldrExample {
        description,
        command_parts: parse_tldr_command(&command),
        command,
    }
}

fn extract_trailing_code(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim_end();
    let close = trimmed.strip_suffix('`')?;
    let open = close.rfind('`')?;
    let command = &close[open + 1..];
    if command.is_empty() || command.contains('`') {
        return None;
    }
    let description = flatten_markdown(close[..open].trim_end().trim_end_matches(':'));
    Some((description, command.to_owned()))
}

fn standalone_code(value: &str) -> Option<&str> {
    value.strip_prefix('`')?.strip_suffix('`')
}

fn strip_prefix_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = value.get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

fn flatten_markdown(value: &str) -> String {
    let mut flattened = flatten_links(value);
    for marker in ["**", "__", "*", "_"] {
        flattened = strip_paired_marker(&flattened, marker);
    }
    flattened = flattened.replace(['`', '<', '>'], "");
    flattened.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn flatten_links(value: &str) -> String {
    let mut flattened = String::new();
    let mut remainder = value;
    while let Some(open) = remainder.find('[') {
        flattened.push_str(&remainder[..open]);
        let after_open = &remainder[open + 1..];
        let Some(label_end) = after_open.find("](") else {
            flattened.push_str(&remainder[open..]);
            return flattened;
        };
        let after_target_open = &after_open[label_end + 2..];
        let Some(target_end) = after_target_open.find(')') else {
            flattened.push_str(&remainder[open..]);
            return flattened;
        };
        flattened.push_str(&after_open[..label_end]);
        remainder = &after_target_open[target_end + 1..];
    }
    flattened.push_str(remainder);
    flattened
}

fn strip_paired_marker(value: &str, marker: &str) -> String {
    let mut stripped = String::new();
    let mut remainder = value;
    while let Some(open) = remainder.find(marker) {
        let after_open = &remainder[open + marker.len()..];
        let Some(close) = after_open.find(marker) else {
            break;
        };
        stripped.push_str(&remainder[..open]);
        stripped.push_str(&after_open[..close]);
        remainder = &after_open[close + marker.len()..];
    }
    stripped.push_str(remainder);
    stripped
}

#[cfg(test)]
mod tests {
    use mant_ast::TldrCommandPart;

    use super::{TldrPageLocation, TldrParseError, parse_tldr_command, parse_tldr_page};

    const PAGE: &str = r"# tar

> Archiving utility.
> More information: <https://www.gnu.org/software/tar>.

- Create an archive:
  `tar {{[-c|--create]}} {{path/to/archive.tar}} {{path/to/file}}`

- Extract an archive: `tar --extract --file {{path/to/archive.tar}}`
";

    fn location() -> TldrPageLocation {
        TldrPageLocation {
            platform: "linux".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/pages/linux/tar.md".to_owned(),
        }
    }

    #[test]
    fn parses_examples_markup_and_long_option_placeholders() {
        let page = parse_tldr_page(PAGE, location()).expect("valid tldr page");

        assert_eq!(page.title, "tar");
        assert_eq!(page.description, ["Archiving utility."]);
        assert_eq!(
            page.more_information.as_deref(),
            Some("https://www.gnu.org/software/tar.")
        );
        assert_eq!(page.examples.len(), 2);
        assert_eq!(
            page.examples[0].command,
            "tar {{[-c|--create]}} {{path/to/archive.tar}} {{path/to/file}}"
        );
        assert_eq!(
            page.examples[0].command_parts,
            [
                TldrCommandPart::Text {
                    value: "tar ".to_owned()
                },
                TldrCommandPart::Placeholder {
                    value: "--create".to_owned()
                },
                TldrCommandPart::Text {
                    value: " ".to_owned()
                },
                TldrCommandPart::Placeholder {
                    value: "path/to/archive.tar".to_owned()
                },
                TldrCommandPart::Text {
                    value: " ".to_owned()
                },
                TldrCommandPart::Placeholder {
                    value: "path/to/file".to_owned()
                },
            ]
        );
    }

    #[test]
    fn preserves_escaped_braces_and_unicode_text() {
        assert_eq!(
            parse_tldr_command(r"echo \{\{不是占位符\}\} {{值}}"),
            [
                TldrCommandPart::Text {
                    value: "echo {{不是占位符}} ".to_owned()
                },
                TldrCommandPart::Placeholder {
                    value: "值".to_owned()
                },
            ]
        );
    }

    #[test]
    fn accepts_inline_examples_and_flattens_description_markup() {
        let page = parse_tldr_page(
            "# demo\n> Use **demo** with [docs](https://example.test).\n- Run it: `demo _x_`\n",
            location(),
        )
        .expect("valid tldr page");

        assert_eq!(page.description, ["Use demo with docs."]);
        assert_eq!(page.examples[0].description, "Run it");
        assert_eq!(page.examples[0].command, "demo _x_");
    }

    #[test]
    fn retains_an_example_description_when_its_command_is_missing() {
        let page =
            parse_tldr_page("# demo\n- Explain only:\n", location()).expect("valid tldr page");
        assert_eq!(page.examples[0].description, "Explain only");
        assert!(page.examples[0].command.is_empty());
    }

    #[test]
    fn rejects_a_page_without_a_command_heading() {
        assert_eq!(
            parse_tldr_page("> description only", location()),
            Err(TldrParseError::MissingCommandHeading)
        );
    }
}
