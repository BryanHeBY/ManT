//! Annotates semantic entries without changing ordinary Markdown content.

mod bindings;

use super::directives::{
    AttachedValuePolicy, DomainDeclaration, DomainDeclarationState, SemanticDeclarations,
    domain_diagnostic, semantic_diagnostic,
};
use mant_ir::{
    Block, DefinitionRole, Diagnostic, DiagnosticLevel, Inline, LinkTarget, ListItem, ListKind,
    SourceSpan, ValueDomain,
};

use crate::block::block_source;
use crate::definitions::{environment_variable_alias, option_names_from_terms, option_prefix};

/// Attach facts to each declared owner without consuming its head or delimiter.
pub(super) fn normalize_entry_lists(
    blocks: &mut [Block],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    entry_coverage(blocks, declarations, diagnostics);
}

/// Coverage of direct semantic children through transparent containers. A
/// successfully annotated owner is a boundary: failures inside its own body
/// cannot invalidate a sibling's or parent's enumeration of that owner.
#[derive(Clone, Copy, Default)]
struct EntryCoverage {
    rejected: bool,
}

fn entry_coverage(
    blocks: &mut [Block],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> EntryCoverage {
    let mut coverage = EntryCoverage::default();
    for block in blocks {
        let Block::List {
            kind,
            items,
            source,
            ..
        } = block
        else {
            coverage.rejected |= normalize_nested_blocks(block, declarations, diagnostics).rejected;
            continue;
        };
        let owner_offsets = source
            .and_then(|source| source.byte_range)
            .and_then(|range| usize::try_from(range.start.get()).ok())
            .and_then(|start| declarations.list_items.remove(&start))
            .unwrap_or_default();
        let child_coverage = items
            .iter_mut()
            .enumerate()
            .map(|(index, item)| {
                item_child_coverage(
                    item,
                    owner_offsets.get(index).copied(),
                    declarations,
                    diagnostics,
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            continue;
        }
        let declaration = source.and_then(|source| declarations.entries.remove(&source.line));
        if declaration.is_none() && *kind != ListKind::Bullet {
            coverage.rejected |= child_coverage.iter().any(|value| value.rejected);
            continue;
        }
        let role = declaration.map_or(DefinitionRole::Option, |value| value.role);
        let case = declaration.map_or(mant_ir::DefinitionCase::Sensitive, |value| value.case);
        let attached = declaration.map_or(AttachedValuePolicy::Infer, |value| value.attached);
        let signatures = items
            .iter()
            .map(|item| entry_signature(item, role, declaration.is_some(), attached))
            .collect::<Vec<_>>();
        // Undeclared option recognition retains its conservative whole-list
        // admission rule. An explicit declaration instead owns each item's
        // validation independently; one rejection cannot erase valid siblings.
        if declaration.is_none() && signatures.iter().any(Result::is_err) {
            coverage.rejected |= child_coverage.iter().any(|value| value.rejected);
            if resembles_rejected_option_list(items) {
                semantic_diagnostic(
                    diagnostics,
                    source.unwrap_or(SourceSpan {
                        byte_range: None,
                        line: 1,
                        column: 1,
                        end_line: None,
                        end_column: None,
                    }),
                    "option-like list is not complete; every item needs code terms and a ':' or dash description delimiter"
                        .to_owned(),
                );
            }
            continue;
        }
        for (item_index, ((item, signature), children)) in items
            .iter_mut()
            .zip(signatures)
            .zip(child_coverage)
            .enumerate()
        {
            let signature = match signature {
                Ok(signature) => signature,
                Err(rejection) => {
                    coverage.rejected = true;
                    rejection.emit(
                        diagnostics,
                        source.unwrap_or(declaration.expect("declared rejection").source),
                    );
                    continue;
                }
            };
            let domain = owner_offsets
                .get(item_index)
                .and_then(|offset| declarations.domains.remove(offset))
                .and_then(DomainDeclarationState::into_unique);
            item.entry = Some(bindings::entry_facts(
                item,
                &signature,
                role,
                case,
                attached,
                declaration.is_some(),
            ));
            if let Some(declaration) = domain {
                attach_domain(item, declaration, children, diagnostics);
            }
        }
    }
    coverage
}

fn attach_domain(
    item: &mut ListItem,
    declaration: DomainDeclaration,
    children: EntryCoverage,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if matches!(declaration.value, ValueDomain::Choices { exhaustive: true }) && children.rejected {
        domain_diagnostic(diagnostics, declaration.source, "exhaustive choices requires complete extraction of the direct child entries; rejected children remain visible and the declared domain was omitted".to_owned());
    } else if matches!(declaration.value, ValueDomain::Choices { .. }) && !item.has_value_choices()
    {
        domain_diagnostic(diagnostics, declaration.source, "choices requires nonempty direct semantic children of role=value; the declared domain was omitted".to_owned());
    } else {
        item.entry
            .as_mut()
            .expect("an annotated entry has facts")
            .value_domain = Some(declaration.value);
    }
}

/// Collect both kinds of child failure before deciding whether this item is a
/// semantic owner. Ordinary bullet/ordered containers must carry declaration
/// failures upward just like failures returned by their nested content.
fn item_child_coverage(
    item: &mut ListItem,
    owner_offset: Option<usize>,
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> EntryCoverage {
    let rejected_declaration =
        owner_offset.is_some_and(|offset| declarations.incomplete_entry_children.remove(&offset));
    let mut coverage = entry_coverage(&mut item.blocks, declarations, diagnostics);
    coverage.rejected |= rejected_declaration;
    coverage
}

fn normalize_nested_blocks(
    block: &mut Block,
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> EntryCoverage {
    let mut coverage = EntryCoverage::default();
    match block {
        Block::List { .. } => {
            return entry_coverage(std::slice::from_mut(block), declarations, diagnostics);
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                normalize_entry_lists(&mut item.description, declarations, diagnostics);
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                coverage.rejected |=
                    entry_coverage(&mut cell.blocks, declarations, diagnostics).rejected;
            }
        }
        Block::Paragraph { .. }
        | Block::Preformatted { .. }
        | Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => {}
    }
    coverage
}

#[derive(Clone)]
struct EntrySignature {
    inline_index: usize,
    byte_index: usize,
    names: Vec<String>,
    form_breaks: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
enum EntryRejectionReason {
    MissingLeadingParagraph,
    MissingLeadingCode,
    UnsupportedOptionPrefix,
    InvalidOptionName,
    InvalidEntryName,
    InvalidPlaceholder,
    InvalidAliasSeparator,
    MissingDescription,
    UnsupportedInline,
}

impl EntryRejectionReason {
    const ALL: [Self; 9] = [
        Self::MissingLeadingParagraph,
        Self::MissingLeadingCode,
        Self::UnsupportedOptionPrefix,
        Self::InvalidOptionName,
        Self::InvalidEntryName,
        Self::InvalidPlaceholder,
        Self::InvalidAliasSeparator,
        Self::MissingDescription,
        Self::UnsupportedInline,
    ];

    const fn code(self) -> &'static str {
        match self {
            Self::MissingLeadingParagraph => "markdown.semantic-entry.missing-leading-paragraph",
            Self::MissingLeadingCode => "markdown.semantic-entry.missing-leading-code",
            Self::UnsupportedOptionPrefix => "markdown.semantic-entry.unsupported-option-prefix",
            Self::InvalidOptionName => "markdown.semantic-entry.invalid-option-name",
            Self::InvalidEntryName => "markdown.semantic-entry.invalid-entry-name",
            Self::InvalidPlaceholder => "markdown.semantic-entry.invalid-placeholder",
            Self::InvalidAliasSeparator => "markdown.semantic-entry.invalid-alias-separator",
            Self::MissingDescription => "markdown.semantic-entry.missing-description",
            Self::UnsupportedInline => "markdown.semantic-entry.unsupported-inline",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::MissingLeadingParagraph => "item must start with a paragraph",
            Self::MissingLeadingCode => "item must start with a code term",
            Self::UnsupportedOptionPrefix => "option term uses an unsupported prefix",
            Self::InvalidOptionName => "option term has an invalid name",
            Self::InvalidEntryName => "entry term has an invalid name",
            Self::InvalidPlaceholder => "option term has an invalid placeholder",
            Self::InvalidAliasSeparator => {
                "entry aliases must be separated by whitespace, ',', '/', or '|'"
            }
            Self::MissingDescription => {
                "entry term must be followed by a ':' or dash description delimiter"
            }
            Self::UnsupportedInline => "entry term contains an unsupported inline construct",
        }
    }
}

pub(crate) fn is_semantic_entry_rejection_code(code: &str) -> bool {
    code == "markdown.semantic-entry-list"
        || code == "markdown.semantic-value-domain"
        || EntryRejectionReason::ALL
            .iter()
            .any(|reason| reason.code() == code)
}

#[derive(Debug, Clone)]
struct EntryRejection {
    reason: EntryRejectionReason,
    term: Option<String>,
    source: Option<SourceSpan>,
}

impl EntryRejection {
    fn new(reason: EntryRejectionReason, term: Option<&str>, source: Option<SourceSpan>) -> Self {
        Self {
            reason,
            term: term.map(ToOwned::to_owned),
            source,
        }
    }

    fn emit(self, diagnostics: &mut Vec<Diagnostic>, fallback: SourceSpan) {
        let subject = self.term.as_deref().map_or_else(
            || "semantic-entry item".to_owned(),
            |term| format!("semantic-entry term '{term}'"),
        );
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some(self.reason.code().to_owned()),
            message: format!(
                "{subject} is invalid: {}; the declared list was left unchanged",
                self.reason.message()
            ),
            source: Some(self.source.unwrap_or(fallback)),
        });
    }
}

/// Validate one leading paragraph and record how to split it after ownership
/// moves out of the source list.
fn entry_signature(
    item: &ListItem,
    role: DefinitionRole,
    explicitly_declared: bool,
    attached: AttachedValuePolicy,
) -> Result<EntrySignature, EntryRejection> {
    let source = item.blocks.first().and_then(block_source);
    let Some(Block::Paragraph { children, .. }) = item.blocks.first() else {
        return Err(EntryRejection::new(
            EntryRejectionReason::MissingLeadingParagraph,
            None,
            source,
        ));
    };
    let mut names = Vec::new();
    let mut leading_term = None;
    let mut form_breaks = Vec::new();
    let mut form_has_term = false;
    for (delimiter_inline, inline) in children.iter().enumerate() {
        if let Some(value) = entry_term_text(inline) {
            form_has_term = true;
            leading_term.get_or_insert(value);
            let parsed = entry_names(value, role, explicitly_declared, attached)
                .map_err(|reason| EntryRejection::new(reason, Some(value), source))?;
            extend_unique(&mut names, parsed);
            continue;
        }
        match inline {
            Inline::Text { value } => {
                if let Some((delimiter_byte, _)) = delimiter_location(value) {
                    if names.is_empty() {
                        return Err(EntryRejection::new(
                            EntryRejectionReason::MissingLeadingCode,
                            leading_term,
                            source,
                        ));
                    }
                    if !form_has_term
                        || value[..delimiter_byte].contains('|')
                        || !is_alias_separator(&value[..delimiter_byte])
                    {
                        return Err(EntryRejection::new(
                            EntryRejectionReason::InvalidAliasSeparator,
                            leading_term,
                            source,
                        ));
                    }
                    return Ok(EntrySignature {
                        inline_index: delimiter_inline,
                        byte_index: delimiter_byte,
                        names,
                        form_breaks,
                    });
                }
                if names.is_empty() {
                    return Err(EntryRejection::new(
                        EntryRejectionReason::MissingLeadingCode,
                        leading_term,
                        source,
                    ));
                }
                if value.trim() == "|" && form_has_term {
                    form_breaks.push(delimiter_inline);
                    form_has_term = false;
                } else if value.contains('|') || !is_alias_separator(value) {
                    return Err(EntryRejection::new(
                        EntryRejectionReason::InvalidAliasSeparator,
                        leading_term,
                        source,
                    ));
                }
            }
            _ => {
                return Err(EntryRejection::new(
                    EntryRejectionReason::UnsupportedInline,
                    leading_term,
                    source,
                ));
            }
        }
    }
    Err(EntryRejection::new(
        if names.is_empty() {
            EntryRejectionReason::MissingLeadingCode
        } else {
            EntryRejectionReason::MissingDescription
        },
        leading_term,
        source,
    ))
}

/// Linked and unlinked terms share exactly the same name/form grammar.
fn entry_term_text(inline: &Inline) -> Option<&str> {
    match inline {
        Inline::Code { value } => Some(value),
        Inline::Link {
            target: LinkTarget::Document { .. } | LinkTarget::Manual { .. },
            children,
            ..
        } => match children.as_slice() {
            [Inline::Code { value }] => Some(value),
            _ => None,
        },
        _ => None,
    }
}

fn is_option_code(value: &str) -> bool {
    let terms = vec![vec![Inline::Code {
        value: value.to_owned(),
    }]];
    !option_names_from_terms(&terms).is_empty() && value.trim_start().starts_with('-')
}

fn entry_names(
    value: &str,
    role: DefinitionRole,
    explicitly_declared: bool,
    attached: AttachedValuePolicy,
) -> Result<Vec<String>, EntryRejectionReason> {
    match role {
        DefinitionRole::Option if explicitly_declared => option_entry_names(value, attached),
        DefinitionRole::Option => value
            .trim_start()
            .starts_with('-')
            .then(|| {
                let terms = vec![vec![Inline::Code {
                    value: value.to_owned(),
                }]];
                option_names_from_terms(&terms)
            })
            .filter(|names| !names.is_empty())
            .ok_or(EntryRejectionReason::InvalidOptionName),
        DefinitionRole::Command => plain_entry_name(value, is_command_name),
        DefinitionRole::EnvironmentVariable => environment_variable_alias(value)
            .map(|name| vec![name])
            .ok_or(EntryRejectionReason::InvalidEntryName),
        DefinitionRole::Variable => plain_entry_name(value, is_variable_name),
        DefinitionRole::Marker
        | DefinitionRole::Operand
        | DefinitionRole::ConfigurationKey
        | DefinitionRole::Value
        | DefinitionRole::Term => plain_entry_name(value, |name| {
            !name.is_empty() && !name.contains(['\r', '\n'])
        }),
    }
}

fn plain_entry_name(
    value: &str,
    validate: fn(&str) -> bool,
) -> Result<Vec<String>, EntryRejectionReason> {
    let name = value.trim();
    validate(name)
        .then(|| vec![name.to_owned()])
        .ok_or(EntryRejectionReason::InvalidEntryName)
}

fn is_command_name(value: &str) -> bool {
    !value.is_empty() && !value.contains(['\r', '\n']) && !value.starts_with(['-', '/'])
}

fn is_variable_name(value: &str) -> bool {
    let Some(value) = value.strip_prefix('$') else {
        return false;
    };
    if matches!(value, "?" | "$" | "^") {
        return true;
    }
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
}

fn option_entry_names(
    value: &str,
    attached: AttachedValuePolicy,
) -> Result<Vec<String>, EntryRejectionReason> {
    let mut names = Vec::new();
    for alias in value.split([',', '|']).map(str::trim) {
        if let Some(parts) = crate::definitions::slash_option_forms(alias) {
            for part in parts {
                names.push(dash_option_name(part, attached)?);
            }
            continue;
        }
        names.push(option_entry_name(alias, attached)?);
    }
    (!names.is_empty())
        .then_some(names)
        .ok_or(EntryRejectionReason::InvalidOptionName)
}

fn option_entry_name(
    value: &str,
    attached: AttachedValuePolicy,
) -> Result<String, EntryRejectionReason> {
    let value = value.trim();
    if value.starts_with('-') {
        return dash_option_name(value, attached);
    }
    if value.starts_with('+') {
        return fixed_prefixed_name(value, "+");
    }
    if let Some(negated) = value.strip_prefix('!') {
        if !negated.starts_with('-') {
            return Err(EntryRejectionReason::UnsupportedOptionPrefix);
        }
        return dash_option_name(negated, attached).map(|name| format!("!{name}"));
    }
    if !value.starts_with('/') {
        return equals_option_name(value, attached);
    }
    if value.starts_with("/+") {
        return fixed_prefixed_name(value, "/+");
    }
    slash_option_name(value, attached)
}

fn equals_option_name(
    value: &str,
    attached: AttachedValuePolicy,
) -> Result<String, EntryRejectionReason> {
    let Some((name, visible_value)) = value.split_once('=') else {
        return Err(EntryRejectionReason::UnsupportedOptionPrefix);
    };
    if !is_ascii_identifier(name) {
        return Err(EntryRejectionReason::InvalidOptionName);
    }
    let placeholder = visible_value.trim();
    if matches!(attached, AttachedValuePolicy::Fixed) {
        if placeholder.is_empty() || is_explicit_placeholder(placeholder) {
            return Ok(format!("{name}="));
        }
        if placeholder != visible_value || !is_safe_segment(placeholder) {
            return Err(EntryRejectionReason::InvalidOptionName);
        }
        return Ok(value.to_owned());
    }
    if !placeholder.is_empty() && !is_placeholder(placeholder) {
        return Err(EntryRejectionReason::InvalidPlaceholder);
    }
    Ok(format!("{name}="))
}

fn fixed_prefixed_name(value: &str, prefix: &str) -> Result<String, EntryRejectionReason> {
    let Some(body) = value.strip_prefix(prefix) else {
        return Err(EntryRejectionReason::UnsupportedOptionPrefix);
    };
    if body.is_empty() || body.contains(char::is_whitespace) || !is_safe_segment(body) {
        return Err(EntryRejectionReason::InvalidOptionName);
    }
    Ok(value.to_owned())
}

fn slash_option_name(
    value: &str,
    attached: AttachedValuePolicy,
) -> Result<String, EntryRejectionReason> {
    let mut parts = value.split_whitespace();
    let token = parts
        .next()
        .ok_or(EntryRejectionReason::InvalidOptionName)?;
    if let Some(placeholder) = parts.next()
        && (parts.next().is_some() || !is_placeholder(placeholder))
    {
        return Err(EntryRejectionReason::InvalidPlaceholder);
    }
    if matches!(token, "/?" | "//?") {
        return Ok(token.to_owned());
    }
    let prefix_width = if token.starts_with("//") { 2 } else { 1 };
    let (head, suffix) = token.split_once(':').unwrap_or((token, ""));
    if head.len() <= prefix_width || !is_safe_dotted_name(&head[prefix_width..]) {
        return Err(EntryRejectionReason::InvalidOptionName);
    }
    if suffix.is_empty() {
        return Ok(head.to_owned());
    }
    Ok(if is_explicit_placeholder(suffix)
        || matches!(attached, AttachedValuePolicy::Infer) && is_placeholder(suffix)
    {
        head
    } else {
        token
    }
    .to_owned())
}

fn is_ascii_identifier(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn is_safe_segment(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn is_safe_dotted_name(value: &str) -> bool {
    value
        .split('.')
        .all(|segment| !segment.is_empty() && is_safe_segment(segment))
}

fn is_placeholder(value: &str) -> bool {
    let value = value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(value);
    !value.is_empty()
        && value.bytes().any(|byte| byte.is_ascii_alphabetic())
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn is_explicit_placeholder(value: &str) -> bool {
    value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .is_some_and(is_placeholder)
}

fn dash_option_name(
    value: &str,
    attached: AttachedValuePolicy,
) -> Result<String, EntryRejectionReason> {
    let value = value.trim();
    let mut parts = value.split_whitespace();
    let token = parts
        .next()
        .ok_or(EntryRejectionReason::InvalidOptionName)?;
    let trailing = parts.next();
    if parts.next().is_some() {
        return Err(EntryRejectionReason::InvalidPlaceholder);
    }
    let (head, suffix) = token.split_once('=').unwrap_or((token, ""));
    let name = option_prefix(head).ok_or(EntryRejectionReason::InvalidOptionName)?;
    if name != head || !name.starts_with('-') {
        return Err(EntryRejectionReason::InvalidOptionName);
    }
    if let Some(placeholder) = trailing
        && (!suffix.is_empty() || !is_placeholder(placeholder))
    {
        return Err(EntryRejectionReason::InvalidPlaceholder);
    }
    if suffix.is_empty() {
        return Ok(name.to_owned());
    }
    if is_explicit_placeholder(suffix)
        || matches!(attached, AttachedValuePolicy::Infer) && is_placeholder(suffix)
    {
        return Ok(name.to_owned());
    }
    if matches!(attached, AttachedValuePolicy::Fixed) && is_safe_segment(suffix) {
        return Ok(token.to_owned());
    }
    Err(if matches!(attached, AttachedValuePolicy::Infer) {
        EntryRejectionReason::InvalidPlaceholder
    } else {
        EntryRejectionReason::InvalidOptionName
    })
}

fn extend_unique(output: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !output.contains(&value) {
            output.push(value);
        }
    }
}

fn resembles_rejected_option_list(items: &[ListItem]) -> bool {
    let option_like = items
        .iter()
        .filter(|item| {
            matches!(
                item.blocks.first(),
                Some(Block::Paragraph { children, .. })
                    if children.iter().any(|inline| matches!(inline, Inline::Code { value } if is_option_code(value)))
            )
        })
        .count();
    option_like > 0 && option_like.saturating_add(1) >= items.len()
}

#[cfg(test)]
fn normalize_option_lists(blocks: &mut [Block]) {
    normalize_entry_lists(
        blocks,
        &mut SemanticDeclarations::default(),
        &mut Vec::new(),
    );
}

fn is_alias_separator(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, ',' | '/' | '|'))
}

fn delimiter_location(value: &str) -> Option<(usize, usize)> {
    value.char_indices().find_map(|(index, character)| {
        matches!(character, ':' | '—' | '–').then_some((index, character.len_utf8()))
    })
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionCase, DefinitionRole, Inline, LayoutHint, ListItem, ListKind};

    use super::normalize_option_lists;

    fn paragraph(children: Vec<Inline>) -> Block {
        Block::Paragraph {
            children,
            layout: LayoutHint::default(),
            source: None,
        }
    }

    #[test]
    fn annotates_only_complete_undeclared_option_lists() {
        let option = |name: &str, description: &str| ListItem {
            entry: None,
            blocks: vec![paragraph(vec![
                Inline::Code {
                    value: name.to_owned(),
                },
                Inline::Text {
                    value: format!(": {description}"),
                },
            ])],
        };
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: true,
            items: vec![
                option("-h, --help", "Show help."),
                option("--version", "Print version."),
            ],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&mut blocks);

        let Block::List { items, .. } = &blocks[0] else {
            panic!("ordinary list must remain intact");
        };
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| {
            item.entry.as_ref().is_some_and(|identity| {
                identity.role == DefinitionRole::Option
                    && identity.case == DefinitionCase::Sensitive
            })
        }));
        assert!(matches!(
            &items[0].blocks[0],
            Block::Paragraph { children, .. }
                if matches!(&children[1], Inline::Text { value } if value == ": Show help.")
        ));
    }

    #[test]
    fn keeps_trailing_content_blocks_in_the_original_item() {
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: false,
            items: vec![ListItem {
                entry: None,
                blocks: vec![
                    paragraph(vec![
                        Inline::Code {
                            value: "--config".to_owned(),
                        },
                        Inline::Text {
                            value: ": Read configuration.".to_owned(),
                        },
                    ]),
                    Block::Preformatted {
                        children: vec![Inline::Text {
                            value: "tool --config path".to_owned(),
                        }],
                        language: None,
                        layout: LayoutHint::default(),
                        source: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&mut blocks);

        let Block::List { items, .. } = &blocks[0] else {
            panic!("ordinary list must remain intact");
        };
        assert!(matches!(
            items[0].blocks.as_slice(),
            [Block::Paragraph { .. }, Block::Preformatted { children, .. }]
                if matches!(&children[0], Inline::Text { value } if value == "tool --config path")
        ));
    }

    #[test]
    fn leaves_mixed_lists_unchanged() {
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: true,
            items: vec![
                ListItem {
                    entry: None,
                    blocks: vec![paragraph(vec![
                        Inline::Code {
                            value: "--color".to_owned(),
                        },
                        Inline::Text {
                            value: ": Control colour.".to_owned(),
                        },
                    ])],
                },
                ListItem {
                    entry: None,
                    blocks: vec![paragraph(vec![Inline::Text {
                        value: "ordinary prose".to_owned(),
                    }])],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        let original = blocks.clone();

        normalize_option_lists(&mut blocks);

        assert_eq!(blocks, original, "a rejected mixed list remains untouched");
    }
}
