//! Shared semantic layout for interactive and one-shot tldr presentation.

use mant_ir::{TldrCommandPart, TldrDocument, TldrOrigin};

/// Presentation role independent from Ratatui and ANSI escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TldrRole {
    /// Quick-reference heading.
    Title,
    /// Introductory prose.
    Body,
    /// Human explanation for a command example.
    Example,
    /// Literal command syntax, including executable names and options.
    Command,
    /// Replaceable command value rendered without command emphasis.
    Placeholder,
    /// More-information link.
    Link,
    /// Upstream source and license notice.
    Attribution,
}

/// One styled fragment in the shared tldr presentation model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TldrSpan {
    /// Visible text.
    pub text: String,
    /// Presentation role mapped independently by each frontend.
    pub role: TldrRole,
}

/// One logical terminal line in the shared tldr presentation model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TldrLine {
    /// Leading indentation in terminal cells.
    pub indent: usize,
    /// Styled fragments in display order.
    pub spans: Vec<TldrSpan>,
}

/// Build the canonical foreground content used by both terminal frontends.
#[must_use]
pub fn layout_tldr(document: &TldrDocument) -> Vec<TldrLine> {
    let mut lines = vec![line(
        0,
        TldrRole::Title,
        format!("TLDR QUICK REFERENCE · {}", document.title),
    )];
    for description in &document.description {
        lines.push(TldrLine::default());
        lines.push(line(0, TldrRole::Body, description.trim()));
    }
    for example in &document.examples {
        lines.push(TldrLine::default());
        lines.push(line(0, TldrRole::Example, example.description.trim()));
        let spans = example
            .command_parts
            .iter()
            .map(|part| match part {
                TldrCommandPart::Text { value } => TldrSpan {
                    text: value.clone(),
                    role: TldrRole::Command,
                },
                TldrCommandPart::Placeholder { value } => TldrSpan {
                    text: value.clone(),
                    role: TldrRole::Placeholder,
                },
            })
            .collect::<Vec<_>>();
        lines.push(TldrLine {
            indent: 2,
            spans: if spans.is_empty() {
                vec![TldrSpan {
                    text: example.command.clone(),
                    role: TldrRole::Command,
                }]
            } else {
                spans
            },
        });
    }
    if let Some(link) = &document.more_information {
        lines.push(TldrLine::default());
        lines.push(line(
            0,
            TldrRole::Link,
            format!("More information: {}", link.trim()),
        ));
    }
    if document.origin == TldrOrigin::TldrPages {
        lines.push(line(
            0,
            TldrRole::Attribution,
            format!(
                "tldr-pages · CC BY 4.0 · {} · {}",
                document.platform, document.language
            ),
        ));
    }
    lines
}

/// Render the shared layout as terminal text, optionally with true-color ANSI.
#[must_use]
pub fn render_tldr_terminal(document: &TldrDocument, color: bool) -> String {
    let mut output = String::new();
    for line in layout_tldr(document) {
        output.push_str(&" ".repeat(line.indent));
        for span in line.spans {
            if color {
                output.push_str(ansi_style(span.role));
            }
            output.push_str(&crate::text::sanitize_terminal_text(&span.text));
            if color {
                output.push_str("\x1b[0m");
            }
        }
        output.push('\n');
    }
    output
}

fn line(indent: usize, role: TldrRole, text: impl Into<String>) -> TldrLine {
    TldrLine {
        indent,
        spans: vec![TldrSpan {
            text: text.into(),
            role,
        }],
    }
}

const fn ansi_style(role: TldrRole) -> &'static str {
    match role {
        TldrRole::Title => "\x1b[1;38;2;203;166;247m",
        TldrRole::Body | TldrRole::Placeholder => "\x1b[38;2;166;173;200m",
        TldrRole::Example => "\x1b[38;2;166;227;161m",
        TldrRole::Command => "\x1b[38;2;250;179;135m",
        TldrRole::Link => "\x1b[4;38;2;137;180;250m",
        TldrRole::Attribution => "\x1b[38;2;127;132;156m",
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::{TldrCommandPart, TldrDocument, TldrExample, TldrOrigin};

    use super::render_tldr_terminal;

    #[test]
    fn terminal_color_changes_only_escape_sequences() {
        let page = TldrDocument {
            title: "demo".to_owned(),
            description: vec!["Do the thing.".to_owned()],
            more_information: None,
            examples: vec![TldrExample {
                description: "Write a file.".to_owned(),
                command: "demo --output file".to_owned(),
                command_parts: vec![
                    TldrCommandPart::Text {
                        value: "demo --output ".to_owned(),
                    },
                    TldrCommandPart::Placeholder {
                        value: "file".to_owned(),
                    },
                ],
            }],
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "demo.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        };
        let plain = render_tldr_terminal(&page, false);
        let colored = render_tldr_terminal(&page, true);
        assert!(colored.contains("\x1b["));
        assert!(plain.contains("TLDR QUICK REFERENCE · demo"));
        assert!(colored.contains("TLDR QUICK REFERENCE · demo"));
        assert!(plain.contains("tldr-pages · CC BY 4.0 · common · en"));
        assert!(plain.contains("  demo --output file\n"));
        assert!(colored.contains("\x1b[38;2;250;179;135mdemo --output \x1b[0m"));
        assert!(colored.contains("\x1b[38;2;166;173;200mfile\x1b[0m"));

        let embedded = TldrDocument {
            origin: TldrOrigin::Embedded,
            ..page
        };
        assert!(!render_tldr_terminal(&embedded, false).contains("CC BY 4.0"));
    }
}
