//! Semantic styles preserve original text and typed link coordinates.

use super::*;

#[test]
fn every_link_kind_uses_the_reference_role_without_changing_text_or_targets() {
    let current = DocumentAddress::Markdown {
        path: "guide/source".into(),
        origin: mant_ir::MarkdownOrigin::Documents,
    };
    for (target, expected) in link_targets() {
        let nodes = [
            Inline::Strong {
                children: vec![Inline::Emphasis {
                    children: vec![Inline::Link {
                        target,
                        title: None,
                        children: vec![Inline::Code {
                            value: "文档✨".into(),
                        }],
                    }],
                }],
            },
            Inline::Text {
                value: " after".into(),
            },
        ];
        let lines =
            styled_inline_lines(&nodes, theme::style(theme::StyleRole::Text), Some(&current));
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "文档✨ after"
        );
        let label = &lines[0].spans[0];
        assert_eq!(label.style.fg, Some(theme::LINK));
        assert_eq!(label.style.bg, Some(theme::SURFACE));
        assert!(
            label
                .style
                .add_modifier
                .contains(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED)
        );
        assert_eq!(
            lines[0].spans[1].style,
            theme::style(theme::StyleRole::Text)
        );
        assert_eq!(lines[0].links.len(), 1);
        assert_eq!(lines[0].links[0].target, expected);
        assert_eq!(lines[0].links[0].start_scalar, 0);
        assert_eq!(lines[0].links[0].end_scalar, 3);
    }
}

fn link_targets() -> Vec<(mant_ir::LinkTarget, LinkTarget)> {
    use mant_ir::LinkTarget as Source;
    vec![
        (
            Source::Section { id: "body".into() },
            LinkTarget::Section("body".into()),
        ),
        (
            Source::Document {
                name: "target".into(),
                fragment: Some("part".into()),
            },
            LinkTarget::Document {
                address: DocumentAddress::Markdown {
                    path: "guide/target".into(),
                    origin: mant_ir::MarkdownOrigin::Documents,
                },
                fragment: Some("part".into()),
            },
        ),
        (
            Source::Manual {
                name: "printf".into(),
                manual_section: Some("3".into()),
            },
            LinkTarget::Document {
                address: DocumentAddress::Manual {
                    name: "printf".into(),
                    manual_section: "3".into(),
                },
                fragment: None,
            },
        ),
        (
            Source::Manual {
                name: "printf".into(),
                manual_section: None,
            },
            LinkTarget::Manual {
                name: "printf".into(),
                manual_section: None,
            },
        ),
        (
            Source::External {
                uri: "https://example.test".into(),
            },
            LinkTarget::External(ExternalUri::parse("https://example.test").unwrap()),
        ),
        (
            Source::Email {
                address: "docs@example.test".into(),
            },
            LinkTarget::External(ExternalUri::parse("mailto:docs@example.test").unwrap()),
        ),
    ]
}
