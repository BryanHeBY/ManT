use mant_ir::ResolvedContent;
use mant_ir::{
    Block, DefinitionItem, DisplayLabel, DisplayRole, DisplayRow, DisplayRun, DisplayStyle,
    DisplaySurface, Document, DocumentBody, DocumentMeta, FixedBody, FlowBody, Inline, LayoutHint,
    Section, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
    TldrDocument, TldrOrigin,
};
use std::num::NonZeroU32;

use super::{render_query_man, render_query_text};

fn sources(format: SourceFormat) -> Vec<SourceRecord> {
    vec![SourceRecord {
        key: SourceKey::FIRST,
        identity: SourceIdentity::Anonymous {
            name: "test".to_owned(),
        },
        format,
        decoded_byte_length: 0,
        content_sha256: None,
        coordinates: SourceCoordinates::DecodedUtf8Bytes,
    }]
}

fn key(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

#[test]
fn malformed_public_fixed_surface_does_not_panic_plain_renderers() {
    // Public Rust fields bypass FixedBody's deserialization validation. These
    // are malformed relationships, not native formatting expectations.
    let invalid_surfaces = [
        DisplaySurface {
            text: String::new(),
            rows: vec![DisplayRow {
                key: key(1),
                first_run: key(2),
                run_count: 1,
                column_count: 1,
                break_after: false,
            }],
            runs: Vec::new(),
        },
        DisplaySurface {
            text: "é".to_owned(),
            rows: vec![DisplayRow {
                key: key(1),
                first_run: key(1),
                run_count: 1,
                column_count: 1,
                break_after: false,
            }],
            runs: vec![DisplayRun {
                key: key(1),
                row: key(1),
                column: 0,
                width: 1,
                byte_start: 1,
                byte_count: 1,
                label: DisplayLabel {
                    owner: None,
                    link: None,
                    source: None,
                    style: DisplayStyle {
                        bold: false,
                        underline: false,
                    },
                    role: DisplayRole::Body,
                },
            }],
        },
    ];
    for surface in invalid_surfaces {
        assert!(surface.validate().is_err());
        let query = ResolvedContent {
            address: None,
            label: "demo".to_owned(),
            document: Some(Document {
                parser: None,
                sources: sources(SourceFormat::Man),
                root_source: SourceKey::FIRST,
                body: DocumentBody::Fixed(FixedBody {
                    surface,
                    headings: Vec::new(),
                    owners: Vec::new(),
                    links: Vec::new(),
                    anchors: Vec::new(),
                    regions: Vec::new(),
                }),
                meta: DocumentMeta::default(),
                fragment_aliases: Vec::new(),
                diagnostics: Vec::new(),
            }),
            tldr: None,
        };
        assert_eq!(render_query_text(&query), "demo");
        assert_eq!(render_query_man(&query), "demo");
        assert_eq!(
            super::render_query_text_with(&query, |_, text| text.to_owned()),
            "demo"
        );
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One complete Fixed surface fixture keeps row/run keys auditable.
fn fixed_body_reads_exact_native_rows_without_reflow_or_duplicate_title() {
    // Source-neutral final cells, not a new Flow reconstruction. Pinned CVS
    // term.c::term_flushln/term_field emits the column gap; a matching no-fill
    // `a   b` reference was checked before asserting the consumer layout.
    let fixed = FixedBody {
        surface: DisplaySurface {
            text: "alpha  beta中".to_owned(),
            rows: vec![
                DisplayRow {
                    key: key(1),
                    first_run: key(1),
                    run_count: 2,
                    column_count: 13,
                    break_after: true,
                },
                DisplayRow {
                    key: key(2),
                    first_run: key(3),
                    run_count: 0,
                    column_count: 0,
                    break_after: true,
                },
                DisplayRow {
                    key: key(3),
                    first_run: key(3),
                    run_count: 1,
                    column_count: 2,
                    break_after: false,
                },
            ],
            runs: vec![
                DisplayRun {
                    key: key(1),
                    row: key(1),
                    column: 0,
                    width: 5,
                    byte_start: 0,
                    byte_count: 5,
                    label: DisplayLabel {
                        owner: None,
                        link: None,
                        source: None,
                        style: DisplayStyle {
                            bold: true,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                },
                DisplayRun {
                    key: key(2),
                    row: key(1),
                    column: 7,
                    width: 6,
                    byte_start: 5,
                    byte_count: 6,
                    label: DisplayLabel {
                        owner: None,
                        link: None,
                        source: None,
                        style: DisplayStyle {
                            bold: false,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                },
                DisplayRun {
                    key: key(3),
                    row: key(3),
                    column: 0,
                    width: 2,
                    byte_start: 11,
                    byte_count: 3,
                    label: DisplayLabel {
                        owner: None,
                        link: None,
                        source: None,
                        style: DisplayStyle {
                            bold: false,
                            underline: true,
                        },
                        role: DisplayRole::Body,
                    },
                },
            ],
        },
        headings: Vec::new(),
        owners: Vec::new(),
        links: Vec::new(),
        anchors: Vec::new(),
        regions: Vec::new(),
    };
    fixed.validate().unwrap();
    let query = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            parser: None,
            sources: sources(SourceFormat::Man),
            root_source: SourceKey::FIRST,
            body: DocumentBody::Fixed(fixed),
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        }),
        tldr: None,
    };
    let expected = "demo(1)\n\nalpha    beta\n\n中";
    assert_eq!(render_query_text(&query), expected);
    assert_eq!(render_query_man(&query), expected);
    let roles = std::cell::RefCell::new(Vec::new());
    assert_eq!(
        super::render_query_text_with(&query, |style, text| {
            roles.borrow_mut().push((style, text.to_owned()));
            text.to_owned()
        }),
        expected
    );
    let roles = roles.into_inner();
    assert!(
        roles
            .iter()
            .any(|(style, text)| text == "alpha" && style.inline.strong)
    );
    assert!(
        roles
            .iter()
            .any(|(style, text)| text == "中" && style.inline.emphasis)
    );
}

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
                    children: vec![crate::test_content::text("CONTENT")],
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
            parser: None,
            sources: sources(SourceFormat::Man),
            root_source: SourceKey::FIRST,
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            body: DocumentBody::Flow(FlowBody {
                heading: None,
                blocks: Vec::new(),
                sections: vec![Section {
                    id: "options-1".to_owned().into(),
                    fragment_aliases: Vec::new(),
                    heading: crate::test_content::heading("OPTIONS"),
                    spacing_before_lines: 0,
                    blocks: vec![paragraph("parent details", true)],
                    children: vec![Section {
                        id: "common-2".to_owned().into(),
                        fragment_aliases: Vec::new(),
                        heading: crate::test_content::heading("Common options"),
                        spacing_before_lines: 1,
                        blocks: vec![paragraph("child details", false)],
                        children: Vec::new(),
                        source: None,
                    }],
                    source: None,
                }],
                content_store: crate::test_content::store(),
            }),
        }),
        tldr: None,
    }
}

fn paragraph(value: &str, strong: bool) -> Block {
    let text = vec![crate::test_content::text(value.to_owned())];
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
fn text_output_uses_profile_glyphs_without_rewriting_logical_atoms() {
    // Fixed CVS term.c::term_word/encode1 on `.TH DISPLAY 1`, `.SH TEST`,
    // `left\[em]right` prints `left--right` in ASCII and `left—right` in UTF-8.
    let mut query = query();
    let left = crate::test_content::text("left");
    let dash = crate::test_content::text("—");
    let right = crate::test_content::text("right");
    let atom = match &dash {
        Inline::Text { content } => content.atom,
        _ => unreachable!(),
    };
    let document = query.document.as_mut().expect("manual document");
    let flow = document.flow_mut().expect("flow fixture");
    flow.sections[0].blocks = vec![Block::Paragraph {
        children: vec![left, dash, right],
        layout: LayoutHint::default(),
        source: None,
    }];
    flow.content_store = crate::test_content::store();
    let root = {
        let record = &mut flow.content_store.atoms[(atom.get() - 1) as usize];
        let mant_ir::ContentAtomKind::Text {
            text,
            display_override,
        } = &mut record.kind
        else {
            unreachable!();
        };
        assert_eq!(text, "—");
        *display_override = Some("--".to_owned());
        record.root
    };
    assert!(
        document
            .content()
            .root_logical_text(root)
            .expect("logical root")
            .contains("left—right")
    );
    assert!(render_query_text(&query).contains("left--right"));
}

#[test]
fn text_output_keeps_each_projection_in_a_combining_grapheme() {
    // Fixed CVS `term.c::term_word` on `.TH X 1`, `.SH NAME`,
    // `X \[em]́ Y` prints `X --<?> Y` in ASCII and `X —́ Y` in UTF-8.
    let mut query = query();
    let left = crate::test_content::text("X");
    let dash = crate::test_content::text("—");
    let mark = crate::test_content::text("\u{0301}");
    let right = crate::test_content::text("Y");
    let atom = |inline: &Inline| match inline {
        Inline::Text { content } => content.atom,
        _ => unreachable!(),
    };
    let dash_atom = atom(&dash);
    let mark_atom = atom(&mark);
    let document = query.document.as_mut().expect("manual document");
    let flow = document.flow_mut().expect("flow fixture");
    flow.sections[0].blocks = vec![Block::Paragraph {
        children: vec![left, dash, mark, right],
        layout: LayoutHint::default(),
        source: None,
    }];
    flow.content_store = crate::test_content::store();
    for (key, expected, glyphs) in [(dash_atom, "—", "--"), (mark_atom, "\u{0301}", "<?>")] {
        let record = &mut flow.content_store.atoms[(key.get() - 1) as usize];
        let mant_ir::ContentAtomKind::Text {
            text,
            display_override,
        } = &mut record.kind
        else {
            unreachable!();
        };
        assert_eq!(text, expected);
        *display_override = Some(glyphs.into());
    }
    mant_ir::validate_content_store(&flow.content_store).unwrap();
    assert!(render_query_text(&query).contains("X--<?>Y"));
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
                parser: None,
                sources: sources(SourceFormat::Man),
                root_source: SourceKey::FIRST,
                meta: DocumentMeta {
                    manual_section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
                fragment_aliases: Vec::new(),
                diagnostics: Vec::new(),
                body: DocumentBody::Flow(FlowBody {
                    heading: None,
                    blocks: Vec::new(),
                    sections: vec![Section {
                        id: "s-1".to_owned().into(),
                        fragment_aliases: Vec::new(),
                        heading: crate::test_content::heading("S"),
                        spacing_before_lines: 0,
                        blocks,
                        children: Vec::new(),
                        source: None,
                    }],
                    content_store: crate::test_content::store(),
                }),
            }),
            tldr: None,
        }
    }
    fn para(value: &str) -> Block {
        Block::Paragraph {
            children: vec![crate::test_content::text(value.to_owned())],
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
            parser: None,
            sources: sources(SourceFormat::Man),
            root_source: SourceKey::FIRST,
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            body: DocumentBody::Flow(FlowBody {
                heading: None,
                blocks: Vec::new(),
                sections: vec![Section {
                    id: "ops".to_owned().into(),
                    fragment_aliases: Vec::new(),
                    heading: crate::test_content::heading("OPERATORS"),
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
                                terms: vec![vec![crate::test_content::text("* / %".to_owned())]],
                                description: vec![Block::Paragraph {
                                    children: vec![crate::test_content::text(
                                        "Multiplication, division, and modulus.".to_owned(),
                                    )],
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
                                terms: vec![vec![crate::test_content::text("space".to_owned())]],
                                description: vec![Block::Paragraph {
                                    children: vec![crate::test_content::text(
                                        "String concatenation.".to_owned(),
                                    )],
                                    layout: LayoutHint::default(),
                                    source: None,
                                }],
                            },
                        ],
                    }],
                    children: Vec::new(),
                    source: None,
                }],
                content_store: crate::test_content::store(),
            }),
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
            parser: None,
            sources: sources(SourceFormat::Man),
            root_source: SourceKey::FIRST,
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            body: DocumentBody::Flow(FlowBody {
                heading: None,
                blocks: Vec::new(),
                sections: vec![Section {
                    id: "ops".to_owned().into(),
                    fragment_aliases: Vec::new(),
                    heading: crate::test_content::heading("OPERATORS"),
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
                                terms: vec![vec![crate::test_content::text("&&".to_owned())]],
                                description: vec![Block::Paragraph {
                                    children: vec![crate::test_content::text(
                                        "Logical AND.".to_owned(),
                                    )],
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
                                terms: vec![vec![crate::test_content::text(
                                    "--long-option-name".to_owned(),
                                )]],
                                description: vec![Block::Paragraph {
                                    children: vec![crate::test_content::text(
                                        "A lengthy flag.".to_owned(),
                                    )],
                                    layout: LayoutHint::default(),
                                    source: None,
                                }],
                            },
                        ],
                    }],
                    children: Vec::new(),
                    source: None,
                }],
                content_store: crate::test_content::store(),
            }),
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
