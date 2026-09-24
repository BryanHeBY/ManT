//! Query-to-report contracts: query DTOs remain useful after source disposal.
use mant_ir::ResolvedContent;
use mant_ir::{
    ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Diagnostic,
    DiagnosticLevel, Document, DocumentMeta, Heading, Inline, LinkTarget, Provenance,
    ReferenceScope, Section, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey,
    SourceRecord,
};
use mant_protocol::{
    ReferenceInventory, ReferenceProjection, ReferenceProjectionMode, ReferenceTargetType,
};
use mant_query::{build_outline, project_references, select_excerpt};

fn section(
    builder: &mut ContentStoreBuilder,
    id: &str,
    title: &str,
    children: Vec<Section>,
) -> Section {
    let owner = builder.push_owner(ContentOwnerKind::Section, Provenance::Unknown);
    let root = builder.push_root(owner, ContentRootKind::Heading, Provenance::Unknown);
    let content = builder.push_text(
        root,
        title.to_owned(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    Section {
        id: id.to_owned().into(),
        fragment_aliases: Vec::new(),
        heading: Heading {
            content: vec![Inline::Text { content }],
            source: None,
        },
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children,
        source: None,
    }
}

fn query() -> ResolvedContent {
    let mut builder = ContentStoreBuilder::new();
    let name = section(&mut builder, "name-1", "NAME", Vec::new());
    let common = section(&mut builder, "common-3", "Common options", Vec::new());
    let other = section(&mut builder, "other-4", "Other options", Vec::new());
    let options = section(&mut builder, "options-2", "OPTIONS", vec![common, other]);
    let files = section(&mut builder, "files-5", "FILES", Vec::new());
    ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Path {
                    name: "/man/demo.1".to_owned(),
                },
                format: SourceFormat::Man,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            body: mant_ir::DocumentBody::Flow(mant_ir::FlowBody {
                content_store: builder.finish(),
                heading: None,
                blocks: Vec::new(),
                sections: vec![name, options, files],
            }),
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        }),
        tldr: None,
    }
}

fn document() -> Document {
    let mut builder = ContentStoreBuilder::new();
    let owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
    let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    let occurrence = builder.push_link(
        owner,
        LinkTarget::Document {
            name: "target".into(),
            fragment: Some("Mixed.Target".into()),
        },
        None,
        Provenance::Unknown,
    );
    let label = builder.push_text(
        root,
        "label\u{1b}[2J".into(),
        None,
        ContentStyle::default(),
        None,
        Some(occurrence),
        Provenance::Unknown,
    );
    Document {
        parser: None,
        sources: vec![SourceRecord {
            key: SourceKey::FIRST,
            identity: SourceIdentity::Anonymous {
                name: "test".into(),
            },
            format: SourceFormat::Markdown,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: SourceCoordinates::DecodedUtf8Bytes,
        }],
        root_source: SourceKey::FIRST,
        body: mant_ir::DocumentBody::Flow(mant_ir::FlowBody {
            content_store: builder.finish(),
            heading: None,
            sections: Vec::new(),
            blocks: vec![mant_ir::Block::Paragraph {
                children: vec![Inline::Link {
                    occurrence,
                    children: vec![Inline::Text { content: label }],
                }],
                layout: mant_ir::LayoutHint::default(),
                source: None,
            }],
        }),
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
    }
}
fn all() -> ReferenceProjection {
    ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        target_types: vec![
            ReferenceTargetType::Document,
            ReferenceTargetType::Manual,
            ReferenceTargetType::Local,
            ReferenceTargetType::External,
            ReferenceTargetType::Email,
        ],
        ..Default::default()
    }
}

#[test]
fn semantic_completeness_distinguishes_rejections_from_author_warnings() {
    let mut markdown_query = query();
    {
        let document = markdown_query.document.as_mut().expect("document");
        document.diagnostics.push(Diagnostic {
            impact: mant_ir::DiagnosticImpact::None,
            level: DiagnosticLevel::Warning,
            code: Some("markdown.unsupported-html".to_owned()),
            message: "author warning".to_owned(),
            source: None,
            source_key: None,
            coverage_scope: None,
        });
    }
    assert!(
        build_outline(&markdown_query)
            .expect("complete outline")
            .semantics_complete
    );

    markdown_query
        .document
        .as_mut()
        .expect("document")
        .diagnostics
        .push(Diagnostic {
            impact: mant_ir::DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Warning,
            code: Some("markdown.semantic-entry-list".to_owned()),
            message: "rejected declaration".to_owned(),
            source: None,
            source_key: None,
            coverage_scope: None,
        });
    assert!(
        !build_outline(&markdown_query)
            .expect("partial outline")
            .semantics_complete
    );

    markdown_query
        .document
        .as_mut()
        .expect("document")
        .diagnostics
        .push(Diagnostic {
            impact: mant_ir::DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Warning,
            code: Some("markdown.semantic-entry.invalid-entry-name".to_owned()),
            message: "rejected entry".to_owned(),
            source: None,
            source_key: None,
            coverage_scope: None,
        });
    assert!(
        !build_outline(&markdown_query)
            .expect("partial outline")
            .semantics_complete
    );

    let mut ir_query = query();
    ir_query
        .document
        .as_mut()
        .expect("document")
        .diagnostics
        .push(Diagnostic {
            impact: mant_ir::DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Warning,
            code: Some("ir.invalid-semantic-document-reference".to_owned()),
            message: "invalid producer relationship".to_owned(),
            source: None,
            source_key: None,
            coverage_scope: None,
        });
    assert!(
        !build_outline(&ir_query)
            .expect("IR-invalid outline")
            .semantics_complete
    );
    ir_query.address = Some(mant_ir::DocumentAddress::Manual {
        name: "demo".into(),
        manual_section: "1".into(),
    });
    let excerpt = select_excerpt(&ir_query, &[mant_protocol::ContentSelector::path("1")])
        .expect("excerpt with invalid producer semantics");
    assert!(!excerpt.semantics_complete);
    assert_eq!(excerpt.address, ir_query.address);
    assert!(mant_render::render_excerpt_text(&excerpt).contains("Semantic entries are incomplete"));
    assert!(
        mant_render::render_excerpt_markdown(&excerpt).contains("Semantic entries are incomplete")
    );
    // MCP strips parser findings but must not erase completeness.
    let mut compact = excerpt;
    compact.diagnostics.clear();
    assert!(
        mant_render::render_excerpt_markdown(&compact).contains("Semantic entries are incomplete")
    );
}

#[test]
fn snapshot_relative_positions_can_remain_legal_after_an_unrelated_insertion() {
    let before =
        mant_loader::load_markdown_text("# Catalog\n\n## Old\n\n[Old](old.md)\n", None).unwrap();
    let after = mant_loader::load_markdown_text(
        "# Catalog\n\n## Inserted\n\n[New](new.md)\n\n## Old\n\n[Old](old.md)\n",
        None,
    )
    .unwrap();
    let old = project_references(
        before.document.as_ref().unwrap(),
        None,
        ReferenceScope::Document,
        &all(),
    );
    let old_record = &old.records[0];
    let target = old_record
        .origin
        .resolve_link(after.document.as_ref().unwrap())
        .unwrap();
    assert!(
        matches!(after.document.as_ref().unwrap().content().link(target).unwrap().unwrap().target(), LinkTarget::Document { name, .. } if name == "new")
    );
    let excerpt =
        mant_query::select_excerpt(&after, std::slice::from_ref(&old_record.source_read)).unwrap();
    assert!(mant_render::render_excerpt_text(&excerpt).contains("New"));
    assert_eq!(
        old_record.source_read,
        mant_protocol::ContentSelector::path("1")
    );
    // Typed paths describe the actually loaded snapshot, not caller intent in
    // an earlier revision; no expected-snapshot token is promised this release.
}

#[test]
fn offline_reference_rendering_uses_only_serialized_facts_and_preserves_decorated_text() {
    let document = document();
    let inventory = project_references(&document, None, ReferenceScope::Document, &all());
    let wire = serde_json::to_vec(&inventory).unwrap();
    drop(inventory);
    drop(document);
    let rebuilt: ReferenceInventory = serde_json::from_slice(&wire).unwrap();
    let plain = mant_render::render_reference_inventory(&rebuilt);
    assert!(plain.contains("target#Mixed.Target"));
    assert!(plain.contains("readSource=path:root"));
    assert!(!plain.contains('\u{1b}'));
    let decorated = mant_render::render_reference_inventory_with(&rebuilt, |_, text| {
        format!("<paint>{text}</paint>")
    });
    assert_eq!(
        plain,
        decorated.replace("<paint>", "").replace("</paint>", "")
    );
}
