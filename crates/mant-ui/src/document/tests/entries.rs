//! Existing regressions grouped by entries behavior; expected values remain independent.
use super::*;

#[test]
fn inline_styles_preserve_the_renderer_neutral_ir_semantics() {
    let lines = styled_inline_lines(
        &[
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "strong".to_owned(),
                }],
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "emphasis".to_owned(),
                }],
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Code {
                value: "--option".to_owned(),
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test".to_owned(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "link".to_owned(),
                }],
            },
        ],
        Style::default().fg(theme::TEXT),
        None,
    );
    let spans = &lines[0].spans;

    assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(spans[0].style.fg, Some(theme::STRONG));
    assert!(spans[2].style.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(spans[2].style.fg, Some(theme::SUBTEXT));
    assert_eq!(spans[4].style.fg, Some(theme::HEADING));
    assert_eq!(spans[6].style.fg, Some(theme::BLUE));
    assert!(spans[6].style.add_modifier.contains(Modifier::UNDERLINED));
    assert_eq!(
        lines[0].links[0].target,
        LinkTarget::External(
            ExternalUri::parse("https://example.test").expect("valid external URI")
        )
    );
}

#[test]
fn tldr_commands_use_terminal_soft_wrapping_instead_of_prose_reflow() {
    let mut bundle = bundle();
    bundle.document = None;
    bundle.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["Quick reference".to_owned()],
        more_information: None,
        examples: vec![TldrExample {
            description: "Run a long command".to_owned(),
            command: "abc defghij".to_owned(),
            command_parts: vec![TldrCommandPart::Text {
                value: "abc defghij".to_owned(),
            }],
        }],
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "demo.md".to_owned(),
        origin: TldrOrigin::Embedded,
    });

    let rendered = DocumentView::new(&bundle).render(12);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert!(rows.iter().any(|row| row.contains("abc de")));
    assert!(rows.iter().any(|row| row.contains("fghij")));
    assert_eq!(rendered.search("abc defghij").len(), 1);
}

#[test]
fn definition_lists_honour_compact_and_per_item_spacing() {
    let definition = |term: &str, description: &str, spacing_before_lines| DefinitionItem {
        source: None,
        identity: None,
        terms: vec![vec![Inline::Text {
            value: term.to_owned(),
        }]],
        description: vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: description.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }],
        inline_term: false,
        spacing_before_lines,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::DefinitionList {
        items: vec![
            definition("-E", "Run the preprocessor.", None),
            definition("-S", "Run the compiler.", Some(2)),
        ],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }];

    let rows = DocumentView::new(&bundle)
        .render(80)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let first_description = rows
        .iter()
        .position(|row| row.contains("Run the preprocessor."))
        .expect("first description");
    let second_term = rows
        .iter()
        .position(|row| row.contains("-S"))
        .expect("second term");

    assert_eq!(second_term, first_description + 3);
    assert!(rows[first_description + 1].trim().is_empty());
    assert!(rows[first_description + 2].trim().is_empty());
}
