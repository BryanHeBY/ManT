//! Validation for invariants shared by every normalized document source.

use crate::{
    Diagnostic, DiagnosticLevel, Document, DocumentIndex, IndexedRole, Inline, LinkTarget, NodeId,
    visit::{self, Visit},
};

/// Validate invariants that parsers must satisfy before consumers receive IR.
///
/// Findings are ordinary document diagnostics so best-effort parsing remains
/// possible, while every parser and consumer sees the same contract failures.
#[must_use]
pub fn validate_document(document: &Document) -> Vec<Diagnostic> {
    let index = DocumentIndex::build(document);
    let mut diagnostics = Vec::new();

    for (id, node) in index.iter() {
        if id.trim().is_empty() {
            for role in node.roles() {
                diagnostics.push(invariant(
                    "ir.empty-identity",
                    format!("{role:?} identity must not be empty"),
                ));
            }
        }
    }

    for duplicate in index.duplicates() {
        diagnostics.push(invariant(
            "ir.duplicate-identity",
            format!("duplicate {:?} identity '{}'", duplicate.role, duplicate.id),
        ));
    }

    let mut links = SectionLinkCollector::default();
    links.visit_document(document);
    for id in links.targets {
        let resolved = index.get(id.as_str()).is_some_and(|node| {
            node.has_role(IndexedRole::Section) || node.has_role(IndexedRole::Anchor)
        });
        if !resolved {
            diagnostics.push(invariant(
                "ir.dangling-section-link",
                format!("section link target '{id}' does not exist"),
            ));
        }
    }

    diagnostics
}

fn invariant(code: &str, message: String) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: None,
    }
}

#[derive(Default)]
struct SectionLinkCollector {
    targets: Vec<NodeId>,
}

impl<'ir> Visit<'ir> for SectionLinkCollector {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link {
            target: LinkTarget::Section { id },
            ..
        } = inline
        {
            self.targets.push(id.clone());
        }
        visit::walk_inline(self, inline);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Block, DocumentMeta, DocumentSource, LayoutHint, Section, SourceFormat};

    use super::*;

    fn document(sections: Vec<Section>, blocks: Vec<Block>) -> Document {
        Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            diagnostics: Vec::new(),
            blocks,
            sections,
        }
    }

    fn section(id: &str) -> Section {
        Section {
            id: id.into(),
            title: id.to_owned(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        }
    }

    #[test]
    fn reports_duplicate_and_empty_section_identities() {
        let diagnostics = validate_document(&document(
            vec![section(""), section("duplicate"), section("duplicate")],
            Vec::new(),
        ));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.empty-identity"));
        assert!(codes.contains(&"ir.duplicate-identity"));
    }

    #[test]
    fn accepts_links_to_sections_and_inline_anchors() {
        let link = |id: &str| Inline::Link {
            target: LinkTarget::Section { id: id.into() },
            title: None,
            children: vec![Inline::Text {
                value: id.to_owned(),
            }],
        };
        let blocks = vec![Block::Paragraph {
            children: vec![
                Inline::Anchor {
                    id: "anchor".into(),
                },
                link("section"),
                link("anchor"),
                link("missing"),
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        let diagnostics = validate_document(&document(vec![section("section")], blocks));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code.as_deref(),
            Some("ir.dangling-section-link")
        );
    }
}
