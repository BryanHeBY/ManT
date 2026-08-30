//! Validation for invariants shared by every normalized document source.

use crate::{
    Block, Diagnostic, DiagnosticLevel, Document, DocumentIndex, IndexedRole, Inline, LinkTarget,
    NodeId, Section, SourceSpan,
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
        } else if !is_normalized_node_id(id) {
            diagnostics.push(invariant(
                "ir.invalid-identity",
                format!("identity '{id}' is not a normalized document-local ID"),
            ));
        }
        if node.roles().len() > 1
            && !(node.roles().len() == 2
                && node.has_role(IndexedRole::Entry)
                && node.has_role(IndexedRole::Anchor))
        {
            diagnostics.push(invariant(
                "ir.identity-role-collision",
                format!(
                    "identity '{id}' is shared by incompatible roles {:?}",
                    node.roles()
                ),
            ));
        }
    }

    for duplicate in index.duplicates() {
        diagnostics.push(invariant(
            "ir.duplicate-identity",
            format!("duplicate {:?} identity '{}'", duplicate.role, duplicate.id),
        ));
    }

    let mut collector = InvariantCollector::default();
    collector.visit_document(document);
    diagnostics.extend(collector.diagnostics);
    for id in collector.section_targets {
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

fn is_normalized_node_id(id: &str) -> bool {
    let mut characters = id.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    let Some(last) = id.chars().next_back() else {
        return false;
    };
    (first.is_alphanumeric() || first == '_')
        && (last.is_alphanumeric() || last == '_')
        && id
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
        && id.chars().flat_map(char::to_lowercase).eq(id.chars())
}

fn valid_external_uri(uri: &str) -> bool {
    let Some((scheme, remainder)) = uri.split_once(':') else {
        return false;
    };
    !remainder.is_empty()
        && scheme.starts_with(char::is_alphabetic)
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
        && !uri
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

fn validate_source_span(diagnostics: &mut Vec<Diagnostic>, source: SourceSpan) {
    if source.line == 0 || source.column == 0 {
        diagnostics.push(invariant_at(
            "ir.invalid-source-position",
            "source lines and columns must be one-based".to_owned(),
            source,
        ));
    }
    if source.end_line == Some(0) || source.end_column == Some(0) {
        diagnostics.push(invariant_at(
            "ir.invalid-source-position",
            "source end lines and columns must be one-based".to_owned(),
            source,
        ));
    }
    if source.end_line.is_none() != source.end_column.is_none() {
        diagnostics.push(invariant_at(
            "ir.incomplete-source-end",
            "source end line and column must be supplied together".to_owned(),
            source,
        ));
    }
    if let (Some(end_line), Some(end_column)) = (source.end_line, source.end_column)
        && (end_line < source.line || (end_line == source.line && end_column < source.column))
    {
        diagnostics.push(invariant_at(
            "ir.reverse-source-position",
            "source end position precedes its start".to_owned(),
            source,
        ));
    }
    if source
        .byte_range
        .is_some_and(|range| range.end < range.start)
    {
        diagnostics.push(invariant_at(
            "ir.reverse-source-range",
            "source byte range ends before it starts".to_owned(),
            source,
        ));
    }
}

fn invariant(code: &str, message: String) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: None,
    }
}

fn invariant_at(code: &str, message: String, source: SourceSpan) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: Some(source),
    }
}

#[derive(Default)]
struct InvariantCollector {
    section_targets: Vec<NodeId>,
    diagnostics: Vec<Diagnostic>,
}

impl<'ir> Visit<'ir> for InvariantCollector {
    fn visit_section(&mut self, section: &'ir Section) {
        if let Some(source) = section.source {
            validate_source_span(&mut self.diagnostics, source);
        }
        visit::walk_section(self, section);
    }

    fn visit_block(&mut self, block: &'ir Block) {
        let source = match block {
            Block::Paragraph { source, .. }
            | Block::Preformatted { source, .. }
            | Block::List { source, .. }
            | Block::DefinitionList { source, .. }
            | Block::Table { source, .. }
            | Block::Equation { source, .. }
            | Block::VerticalSpace { source, .. }
            | Block::ThematicBreak { source }
            | Block::Unsupported { source, .. } => *source,
        };
        if let Some(source) = source {
            validate_source_span(&mut self.diagnostics, source);
        }
        if let Block::Table { rows, .. } = block {
            for cell in rows.iter().flat_map(|row| &row.cells) {
                if cell.column_span == 0 || cell.row_span == 0 {
                    self.diagnostics.push(invariant(
                        "ir.invalid-table-span",
                        "table row and column spans must be at least one".to_owned(),
                    ));
                }
            }
        }
        visit::walk_block(self, block);
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        match inline {
            Inline::Link {
                target: LinkTarget::Section { id },
                ..
            } => self.section_targets.push(id.clone()),
            Inline::Link {
                target: LinkTarget::External { uri },
                ..
            } if !valid_external_uri(uri) => self.diagnostics.push(invariant(
                "ir.invalid-external-uri",
                format!("external link target '{uri}' is not an absolute URI"),
            )),
            _ => {}
        }
        visit::walk_inline(self, inline);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentMeta,
        DocumentSource, LayoutHint, Section, SourceFormat, TableCell, TableRow, TextRange,
        TextSize,
    };

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

    #[test]
    fn reports_invalid_ids_role_collisions_ranges_tables_and_uris() {
        let source = SourceSpan {
            byte_range: Some(TextRange {
                start: TextSize::new(9),
                end: TextSize::new(3),
            }),
            line: 0,
            column: 0,
            end_line: Some(0),
            end_column: Some(0),
        };
        let shared: NodeId = "Bad ID".into();
        let section = Section {
            id: shared.clone(),
            title: "invalid".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                items: vec![DefinitionItem {
                    identity: Some(DefinitionIdentity {
                        id: shared.clone(),
                        role: DefinitionRole::Term,
                        case: DefinitionCase::Sensitive,
                        names: vec!["term".to_owned()],
                    }),
                    terms: vec![vec![Inline::Anchor { id: shared.clone() }]],
                    description: Vec::new(),
                    inline_term: false,
                    spacing_before_lines: None,
                }],
                compact: true,
                layout: LayoutHint::default(),
                source: Some(source),
            }],
            children: Vec::new(),
            source: None,
        };
        let blocks = vec![
            Block::Paragraph {
                children: vec![Inline::Link {
                    target: LinkTarget::External {
                        uri: "relative target".to_owned(),
                    },
                    title: None,
                    children: Vec::new(),
                }],
                layout: LayoutHint::default(),
                source: None,
            },
            Block::Table {
                rows: vec![TableRow {
                    cells: vec![TableCell {
                        blocks: Vec::new(),
                        column_span: 0,
                        row_span: 0,
                        alignment: None,
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            },
        ];

        let diagnostics = validate_document(&document(vec![section], blocks));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        for expected in [
            "ir.invalid-identity",
            "ir.identity-role-collision",
            "ir.invalid-source-position",
            "ir.reverse-source-range",
            "ir.invalid-table-span",
            "ir.invalid-external-uri",
        ] {
            assert!(codes.contains(&expected), "missing {expected}: {codes:?}");
        }
    }
}
