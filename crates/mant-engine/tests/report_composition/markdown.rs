//! Query projection and Markdown report rendering preserve document contracts.

use mant_codec::encode::{MarkdownOptions, render_markdown, render_markdown_with_options};
use mant_engine::{render_excerpt_markdown, render_outline_markdown};
use mant_ir::{
    Block, Document, DocumentMeta, DocumentSource, Inline, LayoutHint, ResolvedContent, Section,
    SourceFormat, TldrCommandPart, TldrDocument, TldrExample, TldrOrigin,
};
use mant_query::{build_outline, select_excerpt};

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn manual(sections: Vec<Section>) -> Document {
    Document {
        heading: None,
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Man,
            path: None,
        },
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: Vec::new(),
        sections,
    }
}

fn section(title: &str, blocks: Vec<Block>, children: Vec<Section>) -> Section {
    Section {
        id: title.to_lowercase().into(),
        fragment_aliases: Vec::new(),
        heading: title.into(),
        spacing_before_lines: 0,
        blocks,
        children,
        source: None,
    }
}

#[test]
fn renders_tldr_before_manual_and_resolves_placeholders() {
    let query = ResolvedContent {
        address: None,
        label: "ls".to_owned(),
        document: Some(manual(vec![section("NAME", Vec::new(), Vec::new())])),
        tldr: Some(TldrDocument {
            title: "ls".to_owned(),
            description: vec!["List directory contents.".to_owned()],
            more_information: Some("https://example.com/manual_page.html.".to_owned()),
            examples: vec![TldrExample {
                description: "List all files".to_owned(),
                command: "ls {{[-a|--all]}}".to_owned(),
                command_parts: vec![TldrCommandPart::Text {
                    value: "ls --all".to_owned(),
                }],
            }],
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/pages/common/ls.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        }),
    };

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# ls\n\n## TLDR"));
    assert!(markdown.find("## TLDR") < markdown.find("## NAME"));
    assert!(markdown.contains("```sh\nls --all\n```"));
    assert!(!markdown.contains("{{[-a|--all]}}"));
    assert!(markdown.contains("**More information:** <https://example.com/manual_page.html>."));
    assert!(markdown.contains("*tldr-pages · CC BY 4.0 · common · en*"));
    assert!(markdown.contains("\n\n---\n\n## NAME"));
    assert!(!markdown.contains("<a "));
    assert!(!markdown.ends_with('\n'));

    let outline = render_outline_markdown(&build_outline(&query).expect("combined outline"));
    assert!(outline.contains("- `0` (`tldr`) TLDR QUICK REFERENCE"));
    assert!(outline.contains("- `1` (`name`) NAME"));

    let excerpt =
        select_excerpt(&query, &[mant_protocol::ContentSelector::path("0")]).expect("tldr excerpt");
    let excerpt = render_excerpt_markdown(&excerpt);
    assert!(excerpt.contains("*Outline `0`: TLDR QUICK REFERENCE*"));
    assert!(excerpt.contains("## TLDR"));
    assert!(excerpt.contains("```sh\nls --all\n```"));
    assert!(!excerpt.contains("## NAME"));
}

#[test]
fn renders_and_selects_content_before_the_first_heading() {
    let mut document = manual(vec![section("GUIDE", Vec::new(), Vec::new())]);
    document.source.format = SourceFormat::Markdown;
    document.blocks = vec![paragraph(vec![Inline::Text {
        value: "Document preface.".to_owned(),
    }])];
    let query = ResolvedContent {
        address: None,
        label: "guide.md".to_owned(),
        document: Some(document),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("# guide.md\n\nDocument preface.\n\n## GUIDE"));
    assert!(!markdown.contains("<a "));

    let addressable = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(addressable.contains("<a id=\"document-overview\"></a>\n\nDocument preface."));

    let outline = build_outline(&query).expect("Markdown outline");
    assert_eq!(outline.nodes[0].path(), "root");
    assert_eq!(outline.nodes[1].path(), "1");

    let excerpt = select_excerpt(&query, &[mant_protocol::ContentSelector::path("root")])
        .expect("root excerpt");
    let excerpt = render_excerpt_markdown(&excerpt);
    assert!(excerpt.contains("*Outline `root`: OVERVIEW*"));
    assert!(excerpt.contains("Document preface."));
    assert!(!excerpt.contains("## GUIDE"));
}

#[test]
fn uses_markdown_document_title_without_changing_its_logical_label() {
    let mut document = manual(Vec::new());
    document.source.format = SourceFormat::Markdown;
    document.heading = Some("Actual Doc Title".into());
    document.blocks = vec![paragraph(vec![Inline::Text {
        value: "body".to_owned(),
    }])];
    let query = ResolvedContent {
        address: None,
        label: "filename.md".to_owned(),
        document: Some(document),
        tldr: None,
    };

    assert!(render_markdown(&query).starts_with("# Actual Doc Title\n\nbody"));
    let outline = render_outline_markdown(&build_outline(&query).expect("outline"));
    assert!(
        outline.starts_with("# Actual Doc Title outline"),
        "{outline}"
    );
    let excerpt = render_excerpt_markdown(
        &select_excerpt(&query, &[mant_protocol::ContentSelector::path("root")])
            .expect("root excerpt"),
    );
    assert!(excerpt.starts_with("# Actual Doc Title"), "{excerpt}");
}

#[test]
fn renders_selectable_outline_paths_and_excerpt_breadcrumbs() {
    let query = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some({
            let mut document = manual(vec![section(
                "OPTIONS",
                vec![paragraph(vec![Inline::Text {
                    value: "parent details".to_owned(),
                }])],
                vec![section(
                    "Common options",
                    vec![paragraph(vec![Inline::Strong {
                        children: vec![Inline::Text {
                            value: "child details".to_owned(),
                        }],
                    }])],
                    Vec::new(),
                )],
            )]);
            document.meta.manual_section = Some("1".to_owned());
            document
        }),
        tldr: None,
    };

    let outline = build_outline(&query).expect("outline");
    let outline_markdown = render_outline_markdown(&outline);
    assert!(outline_markdown.starts_with("# demo(1) outline"));
    assert!(outline_markdown.contains("- `1` (`options`) OPTIONS"));
    assert!(outline_markdown.contains("  - `1.1` (`common options`) Common options"));

    let excerpt =
        select_excerpt(&query, &[mant_protocol::ContentSelector::path("1.1")]).expect("excerpt");
    let excerpt_markdown = render_excerpt_markdown(&excerpt);
    assert!(excerpt_markdown.starts_with("# demo(1)"));
    assert!(excerpt_markdown.contains("*Outline `1.1`: OPTIONS → Common options*"));
    assert!(excerpt_markdown.contains("## Common options"));
    assert!(excerpt_markdown.contains("**child details**"));
    assert!(!excerpt_markdown.contains("parent details"));
}
