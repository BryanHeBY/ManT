use mant_ir::ResolvedContent;
use mant_ir::{
    Block, DefinitionItem, Document, DocumentMeta, DocumentSource, Inline, LayoutHint, Section,
    SourceFormat, TldrDocument, TldrOrigin,
};

use super::{render_query_man, render_query_text};

#[test]
fn explicit_spacing_overrides_the_definition_join_default_even_at_zero() {
    for leading_gap in [0, 1] {
        for lines in [0, 1, 3] {
            let blocks = [
                Block::VerticalSpace {
                    lines,
                    source: None,
                },
                Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "CONTENT".into(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
            ];
            assert_eq!(
                super::plain_renderer().render_block_sequence(&blocks, 0, Some(leading_gap)),
                format!("{}CONTENT", "\n".repeat(usize::from(lines) + 1))
            );
        }
    }
}

fn query() -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            heading: None,
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Man,
                path: None,
            },
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "options-1".to_owned().into(),
                fragment_aliases: Vec::new(),
                heading: "OPTIONS".into(),
                spacing_before_lines: 0,
                blocks: vec![paragraph("parent details", true)],
                children: vec![Section {
                    id: "common-2".to_owned().into(),
                    fragment_aliases: Vec::new(),
                    heading: "Common options".into(),
                    spacing_before_lines: 1,
                    blocks: vec![paragraph("child details", false)],
                    children: Vec::new(),
                    source: None,
                }],
                source: None,
            }],
        }),
        tldr: None,
    }
}

fn paragraph(value: &str, strong: bool) -> Block {
    let text = vec![Inline::Text {
        value: value.to_owned(),
    }];
    Block::Paragraph {
        children: if strong {
            vec![Inline::Strong { children: text }]
        } else {
            text
        },
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn renders_plain_queries_without_markup_and_uses_resolved_manual_sections() {
    let output = render_query_text(&query());

    assert!(output.starts_with("demo(1)\n\nOPTIONS"));
    assert!(output.contains("parent details"));
    assert!(output.contains("Common options"));
    assert!(!output.contains("**"));
}

#[test]
fn attributes_only_community_tldr_in_plain_text() {
    let mut community = query();
    community.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["A small demonstration.".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "/cache/tldr/demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    });
    assert!(render_query_text(&community).contains("tldr-pages · CC BY 4.0 · common · en"));

    let mut embedded = community;
    embedded.tldr.as_mut().expect("tldr").origin = TldrOrigin::Embedded;
    assert!(!render_query_text(&embedded).contains("CC BY 4.0"));
}

#[test]
fn man_format_renders_the_manual_but_omits_the_prepended_tldr() {
    let mut query = query();
    query.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["A small demonstration.".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "/cache/tldr/demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    });

    let text = render_query_text(&query);
    let man = render_query_man(&query);

    // text keeps the tldr block; man drops it entirely.
    assert!(text.contains("TLDR"));
    assert!(text.contains("A small demonstration."));
    assert!(!man.contains("TLDR"));
    assert!(!man.contains("A small demonstration."));

    // man still renders the manual body verbatim, without markup.
    assert!(man.starts_with("demo(1)\n\nOPTIONS"));
    assert!(man.contains("parent details"));
    assert!(man.contains("Common options"));
    assert!(!man.contains("**"));
}

#[test]
fn man_format_does_not_invent_a_document_for_tldr_only_queries() {
    let mut query = query();
    query.document = None;
    query.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["A small demonstration.".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "/cache/tldr/demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    });

    assert!(render_query_man(&query).is_empty());
}

#[test]
fn vertical_space_sets_the_gap_instead_of_stacking_blank_lines() {
    fn document_with(blocks: Vec<Block>) -> ResolvedContent {
        ResolvedContent {
            address: None,
            label: "demo".to_owned(),
            document: Some(Document {
                heading: None,
                parser: None,
                source: DocumentSource {
                    format: SourceFormat::Man,
                    path: None,
                },
                meta: DocumentMeta {
                    manual_section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
                fragment_aliases: Vec::new(),
                diagnostics: Vec::new(),
                blocks: Vec::new(),
                sections: vec![Section {
                    id: "s-1".to_owned().into(),
                    fragment_aliases: Vec::new(),
                    heading: "S".into(),
                    spacing_before_lines: 0,
                    blocks,
                    children: Vec::new(),
                    source: None,
                }],
            }),
            tldr: None,
        }
    }
    fn para(value: &str) -> Block {
        Block::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }
    }
    let vspace = |lines: u16| Block::VerticalSpace {
        lines,
        source: None,
    };

    // One vertical-space line yields exactly one blank line, not several.
    let one = render_query_text(&document_with(vec![
        para("first"),
        vspace(1),
        para("second"),
    ]));
    assert!(one.contains("first\n\nsecond"), "got: {one:?}");
    assert!(!one.contains("first\n\n\nsecond"), "got: {one:?}");

    // A larger explicit gap is preserved rather than collapsed.
    let wide = render_query_text(&document_with(vec![
        para("first"),
        vspace(2),
        para("second"),
    ]));
    assert!(wide.contains("first\n\n\nsecond"), "got: {wide:?}");

    // Heading/content and trailing gaps are source-owned. Only the page
    // title receives an independent presentation separator.
    let edges = render_query_text(&document_with(vec![vspace(2), para("only"), vspace(3)]));
    assert!(edges.ends_with("only\n\n\n"), "got: {edges:?}");
    assert!(edges.contains("S\n\n\nonly"), "got: {edges:?}");
}

#[test]
fn inline_definition_descriptions_are_tight_against_their_terms() {
    let bundle = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            heading: None,
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Man,
                path: None,
            },
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "ops".to_owned().into(),
                fragment_aliases: Vec::new(),
                heading: "OPERATORS".into(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    compact: false,
                    layout: LayoutHint::default(),
                    source: None,
                    items: vec![
                        DefinitionItem {
                            source: None,
                            entry: None,
                            layout: mant_ir::DefinitionLayout {
                                inline_term: true,
                                spacing_before_lines: Some(1),
                                ..Default::default()
                            },
                            terms: vec![vec![Inline::Text {
                                value: "* / %".to_owned(),
                            }]],
                            description: vec![Block::Paragraph {
                                children: vec![Inline::Text {
                                    value: "Multiplication, division, and modulus.".to_owned(),
                                }],
                                layout: LayoutHint::default(),
                                source: None,
                            }],
                        },
                        DefinitionItem {
                            source: None,
                            entry: None,
                            layout: mant_ir::DefinitionLayout {
                                inline_term: true,
                                spacing_before_lines: Some(1),
                                ..Default::default()
                            },
                            terms: vec![vec![Inline::Text {
                                value: "space".to_owned(),
                            }]],
                            description: vec![Block::Paragraph {
                                children: vec![Inline::Text {
                                    value: "String concatenation.".to_owned(),
                                }],
                                layout: LayoutHint::default(),
                                source: None,
                            }],
                        },
                    ],
                }],
                children: Vec::new(),
                source: None,
            }],
        }),
        tldr: None,
    };

    let output = render_query_text(&bundle);
    // Tight: exactly one space between term and description, no leaked indent.
    assert!(
        output.contains("* / % Multiplication, division, and modulus."),
        "got: {output:?}"
    );
    assert!(
        output.contains("space String concatenation."),
        "got: {output:?}"
    );
    // No double-space gap between term and description.
    assert!(!output.contains("* / %  "), "got: {output:?}");
    assert!(!output.contains("space  "), "got: {output:?}");
}

#[test]
fn man_format_keeps_inline_definitions_tight() {
    let bundle = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            heading: None,
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Man,
                path: None,
            },
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "ops".to_owned().into(),
                fragment_aliases: Vec::new(),
                heading: "OPERATORS".into(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    compact: false,
                    layout: LayoutHint::default(),
                    source: None,
                    items: vec![
                        DefinitionItem {
                            source: None,
                            entry: None,
                            layout: mant_ir::DefinitionLayout {
                                inline_term: true,
                                spacing_before_lines: Some(1),
                                ..Default::default()
                            },
                            terms: vec![vec![Inline::Text {
                                value: "&&".to_owned(),
                            }]],
                            description: vec![Block::Paragraph {
                                children: vec![Inline::Text {
                                    value: "Logical AND.".to_owned(),
                                }],
                                layout: LayoutHint::default(),
                                source: None,
                            }],
                        },
                        DefinitionItem {
                            source: None,
                            entry: None,
                            layout: mant_ir::DefinitionLayout {
                                inline_term: false,
                                spacing_before_lines: Some(1),
                                ..Default::default()
                            },
                            terms: vec![vec![Inline::Text {
                                value: "--long-option-name".to_owned(),
                            }]],
                            description: vec![Block::Paragraph {
                                children: vec![Inline::Text {
                                    value: "A lengthy flag.".to_owned(),
                                }],
                                layout: LayoutHint::default(),
                                source: None,
                            }],
                        },
                    ],
                }],
                children: Vec::new(),
                source: None,
            }],
        }),
        tldr: None,
    };

    let man = render_query_man(&bundle);
    // Inline terms use the same structural body origin as standalone
    // paragraphs, rather than introducing a label-length-dependent origin.
    assert!(man.contains("&&  Logical AND."), "got: {man:?}");
    // inline_term=false in --format man: term on its own line.
    assert!(
        man.contains("--long-option-name\n    A lengthy flag."),
        "got: {man:?}"
    );
}
