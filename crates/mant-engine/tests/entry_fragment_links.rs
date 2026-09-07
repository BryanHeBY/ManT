//! Every indexed content owner is a local navigation destination.
use mant_engine::{MarkdownOptions, query_markdown_text, render_markdown_with_options};
use mant_ir::{Block, DefinitionItem, DocumentIndex, Inline, LayoutHint, LinkTarget};

#[test]
fn entry_fragments_validate_without_inserting_a_head_anchor() {
    for definition_owner in [false, true] {
        let mut query = query_markdown_text("# Probe\n\nSee [option](#option-help).\n\n<!-- mant:entries role=option case=sensitive -->\n- `--help`: Help.\n", None).unwrap();
        let doc = query.document.as_mut().unwrap();
        assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
        if definition_owner {
            let Block::List { items, .. } = &mut doc.blocks[1] else {
                panic!("list")
            };
            let item = items.pop().unwrap();
            doc.blocks[1] = Block::DefinitionList {
                items: vec![DefinitionItem {
                    identity: item.entry,
                    terms: Vec::new(),
                    description: item.blocks,
                    inline_term: false,
                    spacing_before_lines: None,
                }],
                compact: false,
                layout: LayoutHint::default(),
                source: None,
            };
        }
        let index = DocumentIndex::build(doc);
        assert_eq!(
            index.fragment_target("option-help").unwrap().as_str(),
            "option-help"
        );
        assert_eq!(index.get("option-help").unwrap().roles().len(), 1);
        assert!(mant_ir::validate_document(doc).is_empty());
        let copied = serde_json::from_str(&serde_json::to_string(doc).unwrap()).unwrap();
        assert_eq!(doc, &copied);
        assert!(mant_ir::validate_document(&copied).is_empty());
        if !definition_owner {
            let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
            assert!(
                markdown.contains("<a id=\"option-help\"></a>"),
                "{markdown}"
            );
        }
    }
}

#[test]
fn missing_ids_and_identity_collisions_remain_diagnostics() {
    let mut query = query_markdown_text("# Probe\n\n[missing](#absent)\n\n<!-- mant:entries role=option case=sensitive -->\n- `--help`: Help.\n", None).unwrap();
    let doc = query.document.as_mut().unwrap();
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.dangling-section-link"))
    );
    doc.blocks.push(doc.blocks[1].clone());
    doc.sections.push(mant_ir::Section {
        id: "option-help".into(),
        fragment_aliases: vec!["Mixed.Target".into()],
        title: "Collision".into(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    });
    let diagnostics = mant_ir::validate_document(doc);
    for code in [
        "ir.duplicate-identity",
        "ir.identity-role-collision",
        "ir.dangling-section-link",
    ] {
        assert!(
            diagnostics.iter().any(|d| d.code.as_deref() == Some(code)),
            "{code}"
        );
    }
    let Block::Paragraph { children, .. } = &mut doc.blocks[0] else {
        panic!("paragraph")
    };
    children.push(Inline::Link {
        target: LinkTarget::Section {
            id: "Mixed.Target".into(),
        },
        title: None,
        children: Vec::new(),
    });
    // Typed local links use canonical IDs; exact authored aliases resolve at
    // the source boundary, not by loosening canonical identity validation.
    assert!(
        mant_ir::validate_document(doc)
            .iter()
            .any(|d| d.message.contains("Mixed.Target")
                && d.code.as_deref() == Some("ir.dangling-section-link"))
    );
}
