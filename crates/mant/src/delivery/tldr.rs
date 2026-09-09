//! Host-owned ANSI mapping for the shared quick-reference layout.

use mant_ir::TldrDocument;
use mant_render::{TldrRole, layout_tldr};

/// Render the shared layout as terminal text, optionally with true-color ANSI.
#[must_use]
pub(crate) fn render_tldr_terminal(document: &TldrDocument, color: bool) -> String {
    let mut output = String::new();
    for line in layout_tldr(document) {
        output.push_str(&" ".repeat(line.indent));
        for span in line.spans {
            if color {
                output.push_str(ansi_style(span.role));
            }
            output.push_str(&mant_render::sanitize_terminal_text(&span.text));
            if color {
                output.push_str("\x1b[0m");
            }
        }
        output.push('\n');
    }
    output
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
