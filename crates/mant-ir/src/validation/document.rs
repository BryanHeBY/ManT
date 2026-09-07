//! Validation for invariants shared by every normalized document source.

#[cfg(test)]
use super::links::{email_address_from_mailto_uri, mailto_uri_for_email_address};
use super::{
    links::{is_valid_email_address, is_valid_external_uri},
    source::validate_source_span,
};
use crate::{
    Block, DefinitionItem, Diagnostic, DiagnosticLevel, Document, DocumentIndex, IndexedRole,
    Inline, LinkTarget, NodeId, Section, SemanticDocumentReference, SourceSpan, ValueDomain,
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

    for source in document
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.source)
    {
        validate_source_span(&mut diagnostics, source);
    }

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

    for alias in index.authored_fragments() {
        if alias.is_empty() {
            diagnostics.push(invariant(
                "ir.empty-fragment-alias",
                "source-authored fragment alias must not be empty".to_owned(),
            ));
        } else if alias
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        {
            diagnostics.push(invariant(
                "ir.invalid-fragment-alias",
                format!(
                    "source-authored fragment alias '{alias}' contains whitespace or control characters"
                ),
            ));
        }
    }
    for (alias, targets) in index.ambiguous_fragments() {
        diagnostics.push(invariant(
            "ir.ambiguous-fragment-alias",
            format!(
                "fragment '{alias}' resolves to multiple document-local IDs: {}",
                targets
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }

    let mut collector = InvariantCollector::default();
    collector.visit_document(document);
    diagnostics.extend(collector.diagnostics);
    for id in collector.section_targets {
        if !index.contains(id.as_str()) {
            diagnostics.push(invariant(
                "ir.dangling-section-link",
                format!("section link target '{id}' does not exist"),
            ));
        }
    }

    diagnostics.extend(crate::entry::validate_relations(document, &index));
    diagnostics
}

/// Return whether a shared IR invariant diagnostic makes semantic projection incomplete.
///
/// Producers may add source-specific diagnostics, but consumers should use
/// this classification for source-neutral identity and relationship failures
/// instead of maintaining their own subsets of `ir.*` codes.
#[must_use]
pub fn is_semantic_completeness_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "ir.empty-identity"
            | "ir.invalid-entry-content"
            | "ir.invalid-entry-name-binding"
            | "ir.invalid-entry-alias-groups"
            | "ir.invalid-entry-alias-of"
            | "ir.cyclic-entry-alias"
            | "ir.invalid-identity"
            | "ir.identity-role-collision"
            | "ir.duplicate-identity"
            | "ir.empty-fragment-alias"
            | "ir.invalid-fragment-alias"
            | "ir.ambiguous-fragment-alias"
            | "ir.empty-semantic-document-reference"
            | "ir.invalid-semantic-document-reference"
            | "ir.empty-entry-value-domain"
            | "ir.duplicate-entry-value-kind"
            | "ir.invalid-entry-choices"
    )
}

/// Whether an exact authored ID satisfies the canonical identity grammar.
/// This does not check document-local uniqueness or reserved selector names.
#[must_use]
pub fn is_normalized_node_id(id: &str) -> bool {
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

fn invariant(code: &str, message: String) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: None,
    }
}

pub(super) fn invariant_at(code: &str, message: String, source: SourceSpan) -> Diagnostic {
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

impl InvariantCollector {
    fn validate_entry(&mut self, item: crate::EntryOwner<'_>) {
        if item.facts().is_some() && item.forms().is_none() {
            self.diagnostics.push(invariant(
                "ir.invalid-entry-content",
                "entry form references must address valid owner content".to_owned(),
            ));
        }
        if matches!(
            item.facts()
                .and_then(|identity| identity.value_domain.as_ref()),
            Some(ValueDomain::Choices { .. })
        ) && !item.has_value_choices()
        {
            self.diagnostics.push(invariant(
                "ir.invalid-entry-choices",
                "a choices domain requires nonempty direct semantic children of kind value"
                    .to_owned(),
            ));
        }
        if let Some(ValueDomain::EntrySet {
            reference,
            entry_kinds,
            source,
        }) = item
            .facts()
            .and_then(|identity| identity.value_domain.as_ref())
        {
            validate_semantic_document_reference(&mut self.diagnostics, reference);
            if let Some(source) = source {
                validate_source_span(&mut self.diagnostics, *source);
            }
            if entry_kinds.is_empty() {
                self.diagnostics.push(invariant(
                    "ir.empty-entry-value-domain",
                    "cross-document entry value domain must select at least one entry kind"
                        .to_owned(),
                ));
            }
            if entry_kinds
                .iter()
                .enumerate()
                .any(|(index, kind)| entry_kinds[..index].contains(kind))
            {
                self.diagnostics.push(invariant(
                    "ir.duplicate-entry-value-kind",
                    "cross-document entry value domain must not repeat entry kinds".to_owned(),
                ));
            }
        }
    }
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

    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        self.validate_entry(crate::EntryOwner::Definition(item));
        visit::walk_definition_item(self, item);
    }

    fn visit_list_item(&mut self, item: &'ir crate::ListItem) {
        self.validate_entry(crate::EntryOwner::List(item));
        visit::walk_list_item(self, item);
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
            } if !is_valid_external_uri(uri) => self.diagnostics.push(invariant(
                "ir.invalid-external-uri",
                format!("external link target '{uri}' is not an absolute URI"),
            )),
            Inline::Link {
                target: LinkTarget::Email { address },
                ..
            } if !is_valid_email_address(address) => self.diagnostics.push(invariant(
                "ir.invalid-email-address",
                format!("email link target '{address}' is not a valid mailbox"),
            )),
            _ => {}
        }
        visit::walk_inline(self, inline);
    }
}

fn validate_semantic_document_reference(
    diagnostics: &mut Vec<Diagnostic>,
    reference: &SemanticDocumentReference,
) {
    let empty = match reference {
        SemanticDocumentReference::Document { name, fragment } => {
            name.trim().is_empty()
                || fragment
                    .as_deref()
                    .is_some_and(|fragment| fragment.trim().is_empty())
        }
        SemanticDocumentReference::Manual {
            name,
            manual_section,
        } => {
            name.trim().is_empty()
                || manual_section
                    .as_deref()
                    .is_some_and(|section| section.trim().is_empty())
        }
    };
    if empty {
        diagnostics.push(invariant(
            "ir.empty-semantic-document-reference",
            "semantic document reference components must not be empty".to_owned(),
        ));
    } else if !reference.is_well_formed() {
        diagnostics.push(invariant(
            "ir.invalid-semantic-document-reference",
            "semantic document reference does not follow the document or manual grammar".to_owned(),
        ));
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
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks,
            sections,
        }
    }

    fn section(id: &str) -> Section {
        Section {
            id: id.into(),
            fragment_aliases: Vec::new(),
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
                Inline::anchor("anchor"),
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
    fn fragment_aliases_keep_source_spelling_but_must_resolve_uniquely() {
        let mut first = section("first");
        first.fragment_aliases = vec!["Mixed.Target".into(), "--option".into()];
        let diagnostics = validate_document(&document(vec![first.clone()], Vec::new()));
        assert!(diagnostics.is_empty());

        let mut second = section("second");
        second.fragment_aliases = vec!["Mixed.Target".into(), "bad fragment".into()];
        let diagnostics = validate_document(&document(vec![first, second], Vec::new()));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.invalid-fragment-alias"));
        assert!(codes.contains(&"ir.ambiguous-fragment-alias"));
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
            fragment_aliases: Vec::new(),
            title: "invalid".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                items: vec![DefinitionItem {
                    identity: Some(DefinitionIdentity {
                        name_bindings: Vec::new(),
                        alias_groups: Vec::new(),
                        alias_of: None,
                        forms: Vec::new(),
                        id: shared.clone(),
                        role: DefinitionRole::Term,
                        case: DefinitionCase::Sensitive,
                        names: vec!["term".to_owned()],
                        value_domain: None,
                    }),
                    terms: vec![vec![Inline::anchor(shared.clone())]],
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
                children: vec![
                    Inline::Link {
                        target: LinkTarget::External {
                            uri: "relative target".to_owned(),
                        },
                        title: None,
                        children: Vec::new(),
                    },
                    Inline::Link {
                        target: LinkTarget::Email {
                            address: "missing-domain".to_owned(),
                        },
                        title: None,
                        children: Vec::new(),
                    },
                ],
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
            "ir.invalid-email-address",
        ] {
            assert!(codes.contains(&expected), "missing {expected}: {codes:?}");
        }
    }

    #[test]
    fn validates_external_uri_structure() {
        for uri in [
            "https:relative",
            "https:///missing-host",
            "https://example.test:",
            "https://[::1",
            "https://[::1]:invalid",
            "https://%ZZ@example.test/path",
            "https://example.test/%ZZ",
            "https://user]name@example.test/path",
            "https://example%ZZ.test/path",
            "https://example..test/path",
            "https://例.example/path",
            "https://example.test/path#one#two",
            "mailto:",
            "mailto:?subject=x",
            "mailto:a..b@example.test",
            "mailto:.a@example.test",
            "mailto:a.@example.test",
            "mailto:user%ZZ@example.test",
            "mailto:%2Euser@example.test",
            "mailto:user%2E%2Ename@example.test",
            "mailto:user%40evil@example.test",
            "mailto:user%2Csecond@example.test",
            "mailto:%80@example.test",
            "mailto:%2Euser@example.test?subject=x",
            "mailto:user%2E%2Ename@example.test?subject=x",
            "mailto:user%40evil@example.test?subject=x",
            "mailto:%2Euser@example.test#fragment",
        ] {
            assert!(!is_valid_external_uri(uri), "accepted invalid URI {uri}");
        }
        for uri in [
            "https://example.test/path",
            "https://user@example.test:443/path",
            "https://user%40name@example.test/path",
            "https://[::1]:8443/path",
            "https://[::1]:8443/path?q=x#part",
            "https://service_name.example.test/path",
            "https://example.test./path",
            "https://ex%41mple.test/path",
            "https://xn--fsq.example/path",
            "mailto:user@example.test",
            "mailto:user@example.test?subject=hello",
            "mailto:user%25tag@example.test",
            "mailto:a%2Fb@example.test",
            "mailto:user@example.test,second@example.test",
            "mailto:user%252Etag@example.test",
        ] {
            assert!(is_valid_external_uri(uri), "rejected valid URI {uri}");
        }
    }

    #[test]
    fn validates_email_and_mailto_round_trips() {
        for address in [
            "",
            "missing-domain",
            "@example.test",
            "docs@",
            ".docs@example.test",
            "docs.@example.test",
            "docs..team@example.test",
            "quoted\"name@example.test",
        ] {
            assert!(
                !is_valid_email_address(address),
                "accepted invalid email address {address}"
            );
        }
        for address in [
            "docs@example.test",
            "support@sub.example.test",
            "build+notifications@example.test",
        ] {
            assert!(
                is_valid_email_address(address),
                "rejected valid email address {address}"
            );
        }

        for (uri, address) in [
            ("mailto:docs@example.test", "docs@example.test"),
            ("MAILTO:user%25tag@example.test", "user%tag@example.test"),
            ("mailto:a%2Fb@example.test", "a/b@example.test"),
            (
                "mailto:user%252Etag@example.test",
                "user%2Etag@example.test",
            ),
        ] {
            let (_, remainder) = uri.split_once(':').expect("mailto URI has a scheme");
            let canonical_uri = format!("mailto:{remainder}");
            assert_eq!(
                email_address_from_mailto_uri(uri).as_deref(),
                Some(address),
                "failed to decode {uri}"
            );
            assert_eq!(
                mailto_uri_for_email_address(address).as_deref(),
                Some(canonical_uri.as_str()),
                "failed to serialize {address}"
            );
        }
        for uri in [
            "mailto:%2Euser@example.test",
            "mailto:user%2E%2Ename@example.test",
            "mailto:user%40evil@example.test",
            "mailto:user%2Csecond@example.test",
            "mailto:user@example.test,second@example.test",
            "mailto:user@example.test?subject=x",
            "mailto:user@example.test#fragment",
        ] {
            assert!(
                email_address_from_mailto_uri(uri).is_none(),
                "classified non-typed mailto URI {uri}"
            );
        }
    }

    #[test]
    fn validates_source_spans_owned_by_document_diagnostics() {
        let source = SourceSpan {
            byte_range: Some(TextRange {
                start: TextSize::new(8),
                end: TextSize::new(3),
            }),
            line: 0,
            column: 0,
            end_line: Some(0),
            end_column: Some(0),
        };
        let mut document = document(Vec::new(), Vec::new());
        document.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("producer.finding".to_owned()),
            message: "producer finding".to_owned(),
            source: Some(source),
        });

        let codes = validate_document(&document)
            .into_iter()
            .filter_map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .any(|code| code == "ir.invalid-source-position")
        );
        assert!(codes.iter().any(|code| code == "ir.reverse-source-range"));
    }

    #[test]
    fn reports_invalid_cross_document_entry_domains() {
        let mut definition = DefinitionItem {
            identity: Some(DefinitionIdentity {
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                forms: Vec::new(),
                id: "option-output".into(),
                role: DefinitionRole::Option,
                case: DefinitionCase::Sensitive,
                names: vec!["--output".to_owned()],
                value_domain: Some(crate::ValueDomain::EntrySet {
                    reference: crate::SemanticDocumentReference::Manual {
                        name: String::new(),
                        manual_section: Some(String::new()),
                    },
                    entry_kinds: Vec::new(),
                    source: None,
                }),
            }),
            terms: vec![vec![Inline::anchor("option-output")]],
            description: Vec::new(),
            inline_term: false,
            spacing_before_lines: None,
        };
        let blocks = vec![Block::DefinitionList {
            items: vec![definition.clone()],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }];
        let diagnostics = validate_document(&document(Vec::new(), blocks));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.empty-semantic-document-reference"));
        assert!(codes.contains(&"ir.empty-entry-value-domain"));

        definition.identity.as_mut().expect("identity").value_domain =
            Some(crate::ValueDomain::EntrySet {
                reference: crate::SemanticDocumentReference::Manual {
                    name: "ssh_config".to_owned(),
                    manual_section: Some("qgroup".to_owned()),
                },
                entry_kinds: vec![
                    crate::EntryKind::ConfigurationKey,
                    crate::EntryKind::ConfigurationKey,
                ],
                source: None,
            });
        let diagnostics = validate_document(&document(
            Vec::new(),
            vec![Block::DefinitionList {
                items: vec![definition],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        ));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.invalid-semantic-document-reference"));
        assert!(codes.contains(&"ir.duplicate-entry-value-kind"));
    }

    #[test]
    fn classifies_only_semantic_invariant_diagnostics_as_incomplete() {
        for code in [
            "ir.invalid-identity",
            "ir.ambiguous-fragment-alias",
            "ir.invalid-semantic-document-reference",
            "ir.empty-entry-value-domain",
        ] {
            assert!(is_semantic_completeness_diagnostic(code), "{code}");
        }
        for code in [
            "ir.invalid-table-span",
            "ir.invalid-source-position",
            "ir.invalid-external-uri",
        ] {
            assert!(!is_semantic_completeness_diagnostic(code), "{code}");
        }
    }
}
