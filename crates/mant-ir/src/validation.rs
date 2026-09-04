//! Validation for invariants shared by every normalized document source.

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
    )
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

/// Return whether an email link contains one conservative mailbox spelling.
///
/// The local part is one ASCII dot-atom and the domain uses conservative DNS
/// labels. Quoted strings, internationalized addresses, and address comments
/// remain outside the document contract.
#[must_use]
pub fn is_valid_email_address(address: &str) -> bool {
    if address.is_empty()
        || !address.is_ascii()
        || address
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || address.contains(['?', '#', ','])
    {
        return false;
    }
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    valid_dot_atom(local) && !domain.contains('@') && valid_email_domain(domain)
}

/// Decode one single-recipient `mailto:` URI into its typed email address.
///
/// Header fields, fragments, and recipient lists remain external URIs because
/// they cannot be represented by [`LinkTarget::Email`]. Percent escapes are
/// decoded exactly once before the conservative ASCII mailbox is validated.
#[must_use]
pub fn email_address_from_mailto_uri(uri: &str) -> Option<String> {
    let (scheme, remainder) = uri.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("mailto") || remainder.contains(',') {
        return None;
    }
    let (recipient, query, fragment) = uri_components(remainder)?;
    if query.is_some() || fragment.is_some() {
        return None;
    }
    decode_mailto_recipient(recipient)
}

/// Serialize one typed email address as a structurally valid `mailto:` URI.
///
/// URI-sensitive characters in the ASCII dot-atom local part are percent-
/// encoded exactly once. Unsupported mailbox forms return `None` rather than
/// producing a target that a consumer would later reject.
#[must_use]
pub fn mailto_uri_for_email_address(address: &str) -> Option<String> {
    if !is_valid_email_address(address) {
        return None;
    }
    let mut uri = String::with_capacity("mailto:".len() + address.len());
    uri.push_str("mailto:");
    for byte in address.bytes() {
        if is_mailto_addr_spec_byte(byte) {
            uri.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            uri.push('%');
            uri.push(char::from(HEX[usize::from(byte >> 4)]));
            uri.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    Some(uri)
}

/// Return whether an absolute external URI satisfies the document contract.
///
/// Every component uses RFC 3986 ASCII characters and complete percent
/// triplets. HTTP(S) additionally validates authority, userinfo, host, IPv6,
/// and port structure. `mailto` requires conservative dot-atom mailboxes.
/// Host activation applies a narrower scheme allowlist separately.
#[must_use]
pub fn is_valid_external_uri(uri: &str) -> bool {
    let Some((scheme, remainder)) = uri.split_once(':') else {
        return false;
    };
    let syntax_valid = !remainder.is_empty()
        && scheme
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
        && uri.is_ascii()
        && !uri
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ');
    if !syntax_valid {
        return false;
    }
    let Some((hierarchy, query, fragment)) = uri_components(remainder) else {
        return false;
    };
    if query.is_some_and(|value| !valid_query_or_fragment(value))
        || fragment.is_some_and(|value| !valid_query_or_fragment(value))
    {
        return false;
    }
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        let Some(authority_and_path) = hierarchy.strip_prefix("//") else {
            return false;
        };
        let (authority, path) = authority_and_path
            .find('/')
            .map_or((authority_and_path, ""), |index| {
                authority_and_path.split_at(index)
            });
        return valid_http_authority(authority) && valid_path(path);
    }
    if scheme.eq_ignore_ascii_case("mailto") {
        return !hierarchy.is_empty()
            && hierarchy
                .split(',')
                .all(|recipient| decode_mailto_recipient(recipient).is_some());
    }
    valid_generic_hierarchy(hierarchy)
}

fn decode_mailto_recipient(recipient: &str) -> Option<String> {
    if recipient.is_empty() || !recipient.is_ascii() {
        return None;
    }
    let bytes = recipient.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = decode_hex(*bytes.get(index + 1)?)?;
            let low = decode_hex(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else if is_mailto_addr_spec_byte(bytes[index]) {
            decoded.push(bytes[index]);
            index += 1;
        } else {
            return None;
        }
    }
    let address = String::from_utf8(decoded).ok()?;
    (address.is_ascii() && is_valid_email_address(&address)).then_some(address)
}

const fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

const fn is_mailto_addr_spec_byte(byte: u8) -> bool {
    is_unreserved(byte)
        || matches!(
            byte,
            b'!' | b'$' | b'\'' | b'(' | b')' | b'*' | b'+' | b':' | b'@'
        )
}

fn uri_components(remainder: &str) -> Option<(&str, Option<&str>, Option<&str>)> {
    let (before_fragment, fragment) = remainder
        .split_once('#')
        .map_or((remainder, None), |(value, fragment)| {
            (value, Some(fragment))
        });
    if fragment.is_some_and(|value| value.contains('#')) {
        return None;
    }
    let (hierarchy, query) = before_fragment
        .split_once('?')
        .map_or((before_fragment, None), |(value, query)| {
            (value, Some(query))
        });
    Some((hierarchy, query, fragment))
}

fn valid_generic_hierarchy(hierarchy: &str) -> bool {
    if let Some(authority_and_path) = hierarchy.strip_prefix("//") {
        let (authority, path) = authority_and_path
            .find('/')
            .map_or((authority_and_path, ""), |index| {
                authority_and_path.split_at(index)
            });
        return (authority.is_empty() || valid_http_authority(authority)) && valid_path(path);
    }
    valid_path(hierarchy)
}

fn valid_path(path: &str) -> bool {
    valid_uri_component(path, |byte| is_pchar(byte) || byte == b'/')
}

fn valid_query_or_fragment(value: &str) -> bool {
    valid_uri_component(value, |byte| is_pchar(byte) || matches!(byte, b'/' | b'?'))
}

fn valid_uri_component(value: &str, allowed: impl Fn(u8) -> bool) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else if allowed(bytes[index]) {
            index += 1;
        } else {
            return false;
        }
    }
    true
}

const fn is_pchar(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delimiter(byte) || matches!(byte, b':' | b'@')
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

const fn is_sub_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

fn valid_dot_atom(local: &str) -> bool {
    !local.is_empty()
        && local
            .split('.')
            .all(|atom| !atom.is_empty() && atom.bytes().all(is_atext))
}

const fn is_atext(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'/'
                | b'='
                | b'?'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
                | b'~'
        )
}

fn valid_http_authority(authority: &str) -> bool {
    if authority.is_empty() || authority.contains('\\') {
        return false;
    }
    let host_port = if let Some((userinfo, host_port)) = authority.rsplit_once('@') {
        if userinfo.is_empty()
            || userinfo.contains('@')
            || !valid_uri_component(userinfo, |byte| {
                is_unreserved(byte) || is_sub_delimiter(byte) || byte == b':'
            })
        {
            return false;
        }
        host_port
    } else {
        authority
    };
    if let Some(bracketed) = host_port.strip_prefix('[') {
        let Some((host, suffix)) = bracketed.split_once(']') else {
            return false;
        };
        return host.parse::<std::net::Ipv6Addr>().is_ok()
            && (suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port));
    }
    if host_port.contains(['[', ']']) {
        return false;
    }
    let (host, port) = host_port
        .rsplit_once(':')
        .map_or((host_port, None), |(host, port)| (host, Some(port)));
    valid_uri_reg_name(host) && port.is_none_or(valid_port)
}

fn valid_uri_reg_name(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && valid_uri_component(label, |byte| is_unreserved(byte) || is_sub_delimiter(byte))
        })
}

fn valid_email_domain(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
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

    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        if let Some(ValueDomain::EntrySet {
            reference,
            entry_kinds,
            source,
        }) = item
            .identity
            .as_ref()
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
        visit::walk_definition_item(self, item);
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
