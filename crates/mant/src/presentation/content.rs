//! Styles source-aware spans without querying, searching or classifying text.
use super::terminal::{TerminalRole, terminal_style};
use anstyle::{AnsiColor, Style};
use mant_protocol::{TextPresentation, TextRole, sanitize_terminal_text};
use std::fmt::Write as _;

pub(super) fn decorate(presentation: TextPresentation, value: &str, color: bool) -> String {
    let value = match presentation.role {
        TextRole::Body | TextRole::DefinitionTerm => value
            .chars()
            .map(|c| {
                if c.is_control() && !matches!(c, '\n' | '\t') {
                    '\u{fffd}'
                } else {
                    c
                }
            })
            .collect(),
        _ => sanitize_terminal_text(value).into_owned(),
    };
    if !color {
        return value;
    }
    let style = span_style(presentation);
    if style == Style::new() {
        return value;
    }
    let mut output = String::new();
    for line in value.split_inclusive('\n') {
        // Leave layout whitespace outside the escape pair. The common renderer
        // can still trim/indent/prefix exactly as it does for undecorated text.
        let core = line.trim();
        if core.is_empty() {
            output.push_str(line);
            continue;
        }
        let start = line.len() - line.trim_start().len();
        write!(
            output,
            "{}{style}{core}{style:#}{}",
            &line[..start],
            &line[start + core.len()..]
        )
        .expect("String writer");
    }
    output
}

fn span_style(presentation: TextPresentation) -> Style {
    let mut style = match presentation.role {
        TextRole::Body | TextRole::DefinitionTerm => Style::new(),
        TextRole::Document => terminal_style(TerminalRole::Document),
        TextRole::Heading | TextRole::EvidenceClass(_) => terminal_style(TerminalRole::Heading),
        TextRole::Reference => AnsiColor::BrightBlue.on_default().underline(),
        TextRole::EntryLabel(kind) => terminal_style(TerminalRole::Entry(kind)),
        TextRole::Coordinate => terminal_style(TerminalRole::Coordinate),
        TextRole::Path => terminal_style(TerminalRole::Path),
        TextRole::Guide => terminal_style(TerminalRole::TreeGuide),
        TextRole::Metadata => terminal_style(TerminalRole::Muted),
        TextRole::Notice => AnsiColor::BrightYellow.on_default().bold(),
    };
    let inline = presentation.inline;
    if inline.strong {
        style = style.bold();
    }
    if inline.emphasis {
        style = style.italic();
    }
    if inline.code {
        style = style.fg_color(Some(AnsiColor::BrightCyan.into()));
    }
    if inline.link {
        style = style
            .fg_color(Some(AnsiColor::BrightBlue.into()))
            .underline();
    }
    if let Some(kind) = inline.entry_kind {
        style = style.fg_color(terminal_style(TerminalRole::Entry(kind)).get_fg_color());
    }
    if presentation.matched {
        style = style.bold().underline();
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoration_preserves_layout_whitespace_and_composes_all_modifiers() {
        let presentation = TextPresentation {
            role: TextRole::Body,
            inline: mant_protocol::InlinePresentation {
                strong: true,
                emphasis: true,
                code: true,
                link: true,
                entry_kind: Some(mant_ir::EntryKind::Command),
            },
            matched: true,
        };
        let text = decorate(presentation, " \t前\n\n  after  \n", true);
        assert!(text.starts_with(" \t\u{1b}["));
        assert!(text.ends_with("  \n"));
        assert!(text.contains("\n\n  \u{1b}["));
        assert_eq!(decorate(presentation, " \n\t", true), " \n\t");
        let style = span_style(presentation);
        assert!(style.get_effects().contains(
            anstyle::Effects::BOLD | anstyle::Effects::ITALIC | anstyle::Effects::UNDERLINE
        ));
        assert_eq!(
            style.get_fg_color(),
            terminal_style(TerminalRole::Entry(mant_ir::EntryKind::Command)).get_fg_color()
        );
        let environment = span_style(TextPresentation {
            inline: mant_protocol::InlinePresentation {
                entry_kind: Some(mant_ir::EntryKind::EnvironmentVariable),
                ..presentation.inline
            },
            ..presentation
        });
        assert_eq!(environment.get_fg_color(), Some(AnsiColor::Magenta.into()));
        assert_eq!(environment.get_effects(), style.get_effects());
    }
}
