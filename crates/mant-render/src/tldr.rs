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

fn line(indent: usize, role: TldrRole, text: impl Into<String>) -> TldrLine {
    TldrLine {
        indent,
        spans: vec![TldrSpan {
            text: text.into(),
            role,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::TldrExample;

    #[test]
    fn shared_layout_preserves_commands_placeholders_and_attribution_policy() {
        let mut page = TldrDocument {
            title: "demo".into(),
            description: vec![],
            more_information: Some("https://example.test".into()),
            examples: vec![TldrExample {
                description: "Write a name.".into(),
                command: "demo --name 日本".into(),
                command_parts: vec![
                    TldrCommandPart::Text {
                        value: "demo --name ".into(),
                    },
                    TldrCommandPart::Placeholder {
                        value: "日本".into(),
                    },
                ],
            }],
            platform: "common".into(),
            language: "en".into(),
            source_path: "not-read/demo.md".into(),
            origin: TldrOrigin::Embedded,
        };
        let lines = layout_tldr(&page);
        assert_eq!(lines[3].indent, 2);
        assert_eq!(
            lines[3].spans,
            [
                TldrSpan {
                    text: "demo --name ".into(),
                    role: TldrRole::Command
                },
                TldrSpan {
                    text: "日本".into(),
                    role: TldrRole::Placeholder
                },
            ]
        );
        assert!(lines[1].spans.is_empty());
        assert_eq!(lines.last().unwrap().spans[0].role, TldrRole::Link);
        page.origin = TldrOrigin::TldrPages;
        page.examples[0].command_parts.clear();
        let lines = layout_tldr(&page);
        assert_eq!(
            lines[3].spans,
            [TldrSpan {
                text: "demo --name 日本".into(),
                role: TldrRole::Command,
            }]
        );
        assert_eq!(
            lines.last().unwrap().spans[0],
            TldrSpan {
                text: "tldr-pages · CC BY 4.0 · common · en".into(),
                role: TldrRole::Attribution,
            }
        );
    }
}
