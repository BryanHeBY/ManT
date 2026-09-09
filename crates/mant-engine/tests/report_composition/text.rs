//! Query projection and plain report rendering compose over the same content.

use mant_ir::{
    Block, Document, DocumentMeta, DocumentSource, EntryKind, Inline, LayoutHint, ResolvedContent,
    Section, SourceFormat, TldrDocument, TldrOrigin,
};
use mant_protocol::EntryProjection;
use mant_query::{build_outline, build_outline_projection, select_excerpt};
use mant_render::{render_excerpt_text, render_outline_markdown, render_outline_text};

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
fn renders_copyable_outline_trees_and_contextual_excerpts() {
    let query = query();
    let outline = build_outline(&query).expect("outline");
    assert_eq!(
        render_outline_text(&outline),
        "demo(1)\n└─ 1 OPTIONS\n     ID: options-1\n  └─ 1.1 Common options\n       ID: common-2\nReferences: occurrences=exact(0), targets=exact(0); coverage=Complete; offset=0, returned=0"
    );

    let excerpt =
        select_excerpt(&query, &[mant_protocol::ContentSelector::path("1.1")]).expect("excerpt");
    let output = render_excerpt_text(&excerpt);
    assert!(output.contains("Outline 1.1: OPTIONS > Common options"));
    assert!(output.contains("child details"));
    assert!(!output.contains("parent details"));
}

#[test]
fn renders_an_explicit_zero_for_an_empty_kind_projection() {
    let outline = build_outline_projection(
        &query(),
        EntryProjection::Kinds {
            kinds: vec![EntryKind::EnvironmentVariable],
        },
        None,
    )
    .expect("empty kind projection");

    assert_eq!(
        render_outline_text(&outline),
        "demo(1)\n0 matching semantic entries for: environment variables\nReferences: occurrences=exact(0), targets=exact(0); coverage=Complete; offset=0, returned=0"
    );
    assert!(
        render_outline_markdown(&outline)
            .contains("0 matching semantic entries for: environment variables")
    );
}

#[test]
fn renders_tldr_as_zero_in_outlines_and_standalone_excerpts() {
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

    let outline = render_outline_text(&build_outline(&query).expect("combined outline"));
    assert!(outline.contains("├─ 0 TLDR QUICK REFERENCE\n│    ID: tldr"));
    assert!(outline.contains("└─ 1 OPTIONS\n     ID: options-1"));

    let excerpt = select_excerpt(&query, &[mant_protocol::ContentSelector::id("tldr")])
        .expect("tldr excerpt");
    assert_eq!(
        render_excerpt_text(&excerpt),
        "demo\n\nOutline 0: TLDR QUICK REFERENCE\n\nTLDR\n\nA small demonstration.\n\ntldr-pages · CC BY 4.0 · common · en"
    );
}
