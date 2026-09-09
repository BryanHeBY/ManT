//! Render authored IR and report DTOs without parsing, loading, or querying.

use std::cell::RefCell;

use mant_ir::{
    Block, Document, DocumentMeta, DocumentSource, Inline, LayoutHint, LinkTarget, ResolvedContent,
    Section, SourceFormat,
};
use mant_protocol::{
    EntryProjection, ExcerptSchema, ExcerptSelection, OutlineNode, OutlineNodeReference,
    OutlineSchema, OutlineTrail, QueryBundle, QueryExcerpt, QueryOutline,
};
use mant_render::{
    TextRole, render_excerpt_json, render_excerpt_markdown, render_excerpt_text,
    render_outline_json, render_outline_markdown, render_outline_text, render_query_json,
    render_query_text, render_query_text_with,
};

fn source() -> DocumentSource {
    DocumentSource {
        format: SourceFormat::Markdown,
        // Provenance is opaque to the renderer, never a file to open.
        path: Some("not-opened/独立 source.md".into()),
    }
}

fn section() -> Section {
    Section {
        id: "overview".into(),
        fragment_aliases: vec![],
        heading: "Overview".into(),
        spacing_before_lines: 0,
        children: vec![],
        source: None,
        blocks: vec![
            Block::Paragraph {
                children: vec![
                    Inline::Strong {
                        children: vec![Inline::Text {
                            value: "Cafe\u{301} 👩‍💻".into(),
                        }],
                    },
                    Inline::Text {
                        value: " → ".into(),
                    },
                    Inline::Link {
                        children: vec![Inline::Text {
                            value: "details".into(),
                        }],
                        target: LinkTarget::Document {
                            name: "target".into(),
                            fragment: Some("details".into()),
                        },
                        title: None,
                    },
                ],
                layout: LayoutHint {
                    indent_columns: 2,
                    spacing_before_lines: 1,
                    ..LayoutHint::default()
                },
                source: None,
            },
            Block::Preformatted {
                children: vec![Inline::Text {
                    value: "α\n\nβ".into(),
                }],
                language: None,
                layout: LayoutHint {
                    indent_columns: 4,
                    spacing_before_lines: 1,
                    ..LayoutHint::default()
                },
                source: None,
            },
        ],
    }
}

fn content() -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "specimen".into(),
        tldr: None,
        document: Some(Document {
            parser: None,
            source: source(),
            meta: DocumentMeta::default(),
            heading: Some("Render specimen".into()),
            fragment_aliases: vec![],
            diagnostics: vec![],
            blocks: vec![],
            sections: vec![section()],
        }),
    }
}

fn outline() -> QueryOutline {
    QueryOutline {
        schema: OutlineSchema::V0Dot11,
        entries: EntryProjection::None,
        root: None,
        references: Default::default(),
        label: "specimen".into(),
        display_title: Some("Render specimen".into()),
        address: None,
        source: Some(source()),
        meta: None,
        diagnostics: vec![],
        semantics_complete: true,
        nodes: vec![OutlineNode::DocumentSection {
            path: "1".into(),
            id: "overview".into(),
            title: "Overview".into(),
            entry_summary: None,
            children: vec![],
        }],
    }
}

fn excerpt() -> QueryExcerpt {
    QueryExcerpt {
        schema: ExcerptSchema::V0Dot11,
        label: "specimen".into(),
        display_title: Some("Render specimen".into()),
        address: None,
        semantics_complete: true,
        producer: None,
        source: Some(source()),
        meta: None,
        diagnostics: vec![],
        selections: vec![ExcerptSelection::DocumentSection {
            outline: OutlineTrail {
                ancestors: vec![],
                node: OutlineNodeReference::DocumentSection {
                    path: "1".into(),
                    id: "overview".into(),
                    title: "Overview".into(),
                },
            },
            section: section(),
        }],
    }
}

fn check_source_rendering() -> Result<(), Box<dyn std::error::Error>> {
    let content = content();
    let document = content.document.as_ref().unwrap();
    assert!(mant_ir::validate_document(document).is_empty());
    let original = serde_json::to_value(document)?;
    let source_blocks = document.sections[0].blocks.as_ptr();
    let plain = render_query_text(&content);
    assert!(
        plain.starts_with("Render specimen\n\nOverview\n"),
        "{plain:?}"
    );
    assert!(
        plain.contains("\n  Cafe\u{301} 👩‍💻 → details\n"),
        "{plain:?}"
    );
    assert!(plain.contains("\n    α\n\n    β"), "{plain:?}");
    assert_eq!(
        plain,
        render_query_text_with(&content, |_, text| text.to_owned())
    );

    let observed = RefCell::new(Vec::new());
    let decorated = render_query_text_with(&content, |style, text| {
        observed.borrow_mut().push((style, text.to_owned()));
        format!("\u{1b}[1m{text}\u{1b}[0m")
    });
    assert_eq!(
        decorated.replace("\u{1b}[1m", "").replace("\u{1b}[0m", ""),
        plain
    );
    let observed = observed.into_inner();
    assert!(
        observed
            .iter()
            .any(|(s, t)| s.role == TextRole::Document && t == "Render specimen")
    );
    assert!(
        observed
            .iter()
            .any(|(s, t)| s.role == TextRole::Heading && t == "Overview")
    );
    assert!(
        observed
            .iter()
            .any(|(s, t)| s.inline.strong && t == "Cafe\u{301} 👩‍💻")
    );
    assert!(
        observed
            .iter()
            .any(|(s, t)| s.inline.link && t == "details")
    );

    // Neither body painting nor JSON transport may rewrite source facts/layout.
    assert_eq!(serde_json::to_value(document)?, original);
    assert_eq!(document.sections[0].blocks.as_ptr(), source_blocks);
    assert_eq!(
        document.source.path.as_deref(),
        Some("not-opened/独立 source.md")
    );
    for pretty in [false, true] {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&render_query_json(&content, pretty)?)?,
            serde_json::to_value(QueryBundle::from(&content))?
        );
    }
    Ok(())
}

fn check_materialized_reports() -> Result<(), Box<dyn std::error::Error>> {
    let outline = outline();
    let excerpt = excerpt();
    let outline_before = serde_json::to_value(&outline)?;
    let excerpt_before = serde_json::to_value(&excerpt)?;
    let text = render_outline_text(&outline);
    assert!(text.starts_with("specimen\n"), "{text}");
    assert!(text.contains("1 Overview"), "{text}");
    assert!(render_outline_markdown(&outline).contains("Overview"));
    let text = render_excerpt_text(&excerpt);
    assert!(text.contains("Outline 1: Overview"));
    assert!(text.contains("\n  Cafe\u{301} 👩‍💻 → details\n"));
    let markdown = render_excerpt_markdown(&excerpt);
    assert!(
        markdown.contains("[details](target.md#details)"),
        "{markdown}"
    );
    assert!(markdown.contains("Cafe\u{301} 👩‍💻"));
    for pretty in [false, true] {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&render_outline_json(&outline, pretty)?)?,
            outline_before
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&render_excerpt_json(&excerpt, pretty)?)?,
            excerpt_before
        );
    }
    assert_eq!(serde_json::to_value(&outline)?, outline_before);
    assert_eq!(serde_json::to_value(&excerpt)?, excerpt_before);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    check_source_rendering()?;
    check_materialized_reports()
}

#[cfg(test)]
mod tests {
    #[test]
    fn authored_ir_retains_unicode_layout_and_source_roles() {
        super::check_source_rendering().unwrap();
    }

    #[test]
    fn supplied_report_dtos_keep_json_facts_and_markdown_destinations() {
        super::check_materialized_reports().unwrap();
    }
}
