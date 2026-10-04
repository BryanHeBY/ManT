//! Pure IR stress tests for optional layout admission, without native parsing.

use super::{DOCUMENT_HINT_BUDGET, bound_layout_hints};
use mant_ir::{
    Block, DefinitionItem, DefinitionTerm, DiagnosticImpact, DiagnosticLevel, Document,
    DocumentMeta, DocumentSource, Heading, Inline, InlineContentRef, InlineLayout, LayoutHint,
    RowLayoutHint, Section, SourceFormat,
    visit::{self, Visit},
};

fn document() -> Document {
    Document {
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Man,
            path: None,
        },
        meta: DocumentMeta::default(),
        heading: None,
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: Vec::new(),
        sections: Vec::new(),
    }
}

fn positioned(rows: usize) -> DefinitionTerm {
    DefinitionTerm {
        content: vec![Inline::Strong {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.org".into(),
                },
                title: Some("accepted title".into()),
                children: vec![
                    Inline::Text {
                        value: "  é\u{a0}\n".repeat(rows - 1),
                    },
                    Inline::Code {
                        value: "END".into(),
                    },
                ],
            }],
        }],
        inline_layout: InlineLayout {
            row_hints: (0..rows)
                .map(|row| RowLayoutHint {
                    row: u32::try_from(row).unwrap(),
                    indent_columns: if row % 2 == 0 { 2 } else { -3 },
                })
                .collect(),
        },
    }
}

fn paragraph(term: DefinitionTerm) -> Block {
    Block::Paragraph {
        children: term.content,
        inline_layout: term.inline_layout,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(term: DefinitionTerm) -> Block {
    Block::Preformatted {
        children: term.content,
        inline_layout: term.inline_layout,
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn definition(term: DefinitionTerm) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            source: None,
            entry: None,
            terms: vec![term],
            description: Vec::new(),
            layout: mant_ir::DefinitionLayout::default(),
        }],
        compact: true,
        declaration_groups: Vec::new(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn heading(term: DefinitionTerm) -> Heading {
    Heading {
        content: term.content,
        inline_layout: term.inline_layout,
        source: None,
    }
}

fn section(term: DefinitionTerm) -> Section {
    Section {
        id: "tail".into(),
        fragment_aliases: Vec::new(),
        heading: heading(term),
        blocks: Vec::new(),
        children: Vec::new(),
        spacing_before_lines: 0,
        source: None,
    }
}

fn owners(document: &Document) -> Vec<InlineContentRef<'_>> {
    struct Roots<'ir>(Vec<InlineContentRef<'ir>>);
    impl<'ir> Visit<'ir> for Roots<'ir> {
        fn visit_heading(&mut self, heading: &'ir Heading) {
            self.0.push(InlineContentRef {
                content: &heading.content,
                layout: &heading.inline_layout,
            });
        }
        fn visit_inline(&mut self, _: &'ir Inline) {}
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::Paragraph {
                children,
                inline_layout,
                ..
            }
            | Block::Preformatted {
                children,
                inline_layout,
                ..
            } = block
            {
                self.0.push(InlineContentRef {
                    content: children,
                    layout: inline_layout,
                });
            }
            visit::walk_block(self, block);
        }
        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            self.0
                .extend(item.terms.iter().map(DefinitionTerm::inline_content));
            visit::walk_definition_item(self, item);
        }
    }
    let mut roots = Roots(Vec::new());
    roots.visit_document(document);
    roots.0
}

fn assert_round_trip_and_coverage(document: &Document, omitted: usize) {
    let wire = serde_json::to_string(document).unwrap();
    let decoded: Document = serde_json::from_str(&wire).unwrap();
    assert_eq!(&decoded, document);
    assert_eq!(document.diagnostics.len(), 1);
    let diagnostic = &document.diagnostics[0];
    assert_eq!(diagnostic.code.as_deref(), Some("layout.row-hint-budget"));
    assert_eq!(diagnostic.impact, DiagnosticImpact::None);
    assert_eq!(diagnostic.level, DiagnosticLevel::Warning);
    assert!(
        diagnostic
            .message
            .starts_with(&format!("omitted {omitted} exceptional"))
    );
    assert!(mant_ir::content_complete(&decoded.diagnostics));
    assert!(mant_ir::semantics_complete(&decoded.diagnostics));
}

#[test]
fn each_owner_budget_omits_only_optional_hints_and_remains_json_decodable() {
    let rows = mant_ir::MAX_INLINE_ROW_HINTS + 1;
    for root in 0..5 {
        let mut document = document();
        let term = positioned(rows);
        let expected = term.content.clone();
        let hints = term.inline_layout.row_hints.clone();
        match root {
            0 => document.heading = Some(heading(term)),
            1 => document.blocks.push(paragraph(term)),
            2 => document.blocks.push(literal(term)),
            3 => document.blocks.push(definition(term)),
            4 => document.sections.push(section(term)),
            _ => unreachable!(),
        }
        bound_layout_hints(&mut document);
        let roots = owners(&document);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].content, expected);
        assert_eq!(
            roots[0].layout.row_hints,
            hints[..mant_ir::MAX_INLINE_ROW_HINTS]
        );
        assert_round_trip_and_coverage(&document, 1);
    }
}

#[test]
fn document_budget_is_shared_across_owners_without_truncating_authoritative_body() {
    let per_owner = mant_ir::MAX_INLINE_ROW_HINTS;
    let full_owners = DOCUMENT_HINT_BUDGET / per_owner;
    let mut document = document();
    document.heading = Some(heading(positioned(per_owner)));
    for index in 1..full_owners {
        let term = positioned(per_owner);
        document.blocks.push(match index % 3 {
            0 => paragraph(term),
            1 => literal(term),
            _ => definition(term),
        });
    }
    document.sections.push(section(positioned(1)));
    let expected: Vec<_> = owners(&document)
        .iter()
        .map(|root| root.content.to_vec())
        .collect();
    bound_layout_hints(&mut document);
    let roots = owners(&document);
    assert_eq!(roots.len(), full_owners + 1);
    assert_eq!(
        roots
            .iter()
            .map(|root| root.layout.row_hints.len())
            .sum::<usize>(),
        DOCUMENT_HINT_BUDGET
    );
    assert!(
        roots
            .iter()
            .take(full_owners)
            .all(|root| root.layout.row_hints.len() == per_owner)
    );
    assert!(roots.last().unwrap().layout.is_empty());
    for (root, original) in roots.iter().zip(&expected) {
        assert_eq!(root.content, original.as_slice());
    }
    assert_round_trip_and_coverage(&document, 1);
}
