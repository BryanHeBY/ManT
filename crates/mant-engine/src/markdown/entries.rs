//! Recognizes semantic entries in ordinary Markdown lists.
//!
//! Markdown has no portable definition-list syntax. `ManT` therefore treats a
//! complete bullet list as semantic options only when every item starts with
//! one or more code spans containing options and an explicit description
//! delimiter, for example ``- `-h`, `--help`: Show help.``.

use super::directives::{
    AttachedValuePolicy, DomainDeclarationState, SemanticDeclarations, domain_diagnostic,
    semantic_diagnostic,
};
use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Diagnostic,
    DiagnosticLevel, Inline, LinkTarget, ListItem, ListKind, SourceSpan, ValueDomain,
};

use crate::block::block_source;
use crate::definitions::{environment_variable_alias, option_names_from_terms, option_prefix};

/// Convert unambiguous entry lists without changing mixed or prose lists.
pub(super) fn normalize_entry_lists(
    blocks: &mut Vec<Block>,
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for block in blocks.iter_mut() {
        normalize_nested_blocks(block, declarations, diagnostics);
    }

    for block in blocks {
        let Block::List {
            kind: ListKind::Bullet,
            items,
            compact,
            layout,
            source,
            ..
        } = block
        else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        // Plan every signature before taking ownership so a mixed or prose
        // list remains untouched. Plans retain only delimiter coordinates:
        // successful conversion can then move the original IR exactly once,
        // including potentially large nested description blocks.
        let declaration = source.and_then(|source| declarations.entries.remove(&source.line));
        let role = declaration.map_or(DefinitionRole::Option, |value| value.role);
        let case = declaration.map_or(DefinitionCase::Sensitive, |value| value.case);
        let attached = declaration.map_or(AttachedValuePolicy::Infer, |value| value.attached);
        let signatures = items
            .iter()
            .map(|item| entry_signature(item, role, declaration.is_some(), attached))
            .collect::<Result<Vec<_>, _>>();
        let Ok(signatures) = signatures else {
            if let Some(declaration) = declaration {
                for rejection in items
                    .iter()
                    .filter_map(|item| entry_signature(item, role, true, attached).err())
                {
                    rejection.emit(diagnostics, source.unwrap_or(declaration.source));
                }
            } else if resembles_rejected_option_list(items) {
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
        };
        let definitions = std::mem::take(items)
            .into_iter()
            .zip(signatures)
            .map(|(item, signature)| {
                let declaration = item
                    .blocks
                    .first()
                    .and_then(block_source)
                    .and_then(|source| source.byte_range)
                    .and_then(|range| {
                        declarations
                            .domains
                            .remove(&usize::try_from(range.start.get()).unwrap_or(usize::MAX))
                    })
                    .and_then(DomainDeclarationState::into_unique);
                let mut definition = entry_definition(item, signature, role, case, None);
                if let Some(declaration) = declaration {
                    if matches!(declaration.value, ValueDomain::Choices { .. }) && !definition.has_value_choices() {
                        domain_diagnostic(diagnostics, declaration.source, "choices requires nonempty direct semantic children of role=value; the declared domain was omitted".to_owned());
                    } else {
                        definition.identity.as_mut().expect("a declared entry has an identity").value_domain = Some(declaration.value);
                    }
                }
                definition
            })
            .collect();
        *block = Block::DefinitionList {
            items: definitions,
            compact: *compact,
            layout: *layout,
            source: *source,
        };
    }
}

fn normalize_nested_blocks(
    block: &mut Block,
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match block {
        Block::List { items, .. } => {
            for item in items {
                normalize_entry_lists(&mut item.blocks, declarations, diagnostics);
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                normalize_entry_lists(&mut item.description, declarations, diagnostics);
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                normalize_entry_lists(&mut cell.blocks, declarations, diagnostics);
            }
        }
        Block::Paragraph { .. }
        | Block::Preformatted { .. }
        | Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => {}
    }
}

#[derive(Clone)]
struct EntrySignature {
    inline_index: usize,
    byte_index: usize,
    width: usize,
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
                if let Some((delimiter_byte, delimiter_width)) = delimiter_location(value) {
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
                        width: delimiter_width,
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

/// Move one previously validated item into its semantic definition.
fn entry_definition(
    item: ListItem,
    signature: EntrySignature,
    role: DefinitionRole,
    case: DefinitionCase,
    value_domain: Option<ValueDomain>,
) -> DefinitionItem {
    let mut blocks = item.blocks.into_iter();
    let Some(Block::Paragraph {
        children,
        layout,
        source,
    }) = blocks.next()
    else {
        unreachable!("option_signature accepts only a leading paragraph");
    };
    let (terms, description_inlines) = apply_entry_signature(children, &signature);
    let mut description = Vec::new();
    if !description_inlines.is_empty() {
        description.push(Block::Paragraph {
            children: description_inlines,
            layout,
            source,
        });
    }
    description.extend(blocks);

    DefinitionItem {
        identity: Some(DefinitionIdentity {
            name_bindings: Vec::new(),
            alias_groups: Vec::new(),
            alias_of: None,
            forms: Vec::new(),
            id: String::new().into(),
            role,
            case,
            names: signature.names,
            value_domain,
        }),
        inline_term: false,
        terms,
        description,
        spacing_before_lines: None,
    }
}

fn apply_entry_signature(
    children: Vec<Inline>,
    signature: &EntrySignature,
) -> (Vec<Vec<Inline>>, Vec<Inline>) {
    let mut terms = vec![Vec::new()];
    let mut form_breaks = signature.form_breaks.iter().copied().peekable();
    let mut description = Vec::new();
    for (index, inline) in children.into_iter().enumerate() {
        if index < signature.inline_index {
            if form_breaks.peek() == Some(&index) {
                form_breaks.next();
                terms.push(Vec::new());
            } else {
                terms.last_mut().expect("at least one form").push(inline);
            }
            continue;
        }
        if index > signature.inline_index {
            description.push(inline);
            continue;
        }
        let Inline::Text { value } = inline else {
            unreachable!("option_signature records a text delimiter");
        };
        let after_start = signature.byte_index + signature.width;
        let before = &value[..signature.byte_index];
        if !before.is_empty() {
            terms
                .last_mut()
                .expect("at least one form")
                .push(Inline::Text {
                    value: before.to_owned(),
                });
        }
        let after = value[after_start..].trim_start();
        if !after.is_empty() {
            description.push(Inline::Text {
                value: after.to_owned(),
            });
        }
    }
    (terms, description)
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
fn normalize_option_lists(blocks: &mut Vec<Block>) {
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
    fn converts_only_complete_explicit_option_lists() {
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

        let Block::DefinitionList { items, .. } = &blocks[0] else {
            panic!("explicit option list should become definitions");
        };
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| {
            item.identity.as_ref().is_some_and(|identity| {
                identity.role == DefinitionRole::Option
                    && identity.case == DefinitionCase::Sensitive
            })
        }));
        assert!(matches!(
            &items[0].description[0],
            Block::Paragraph { children, .. }
                if matches!(&children[0], Inline::Text { value } if value == "Show help.")
        ));
    }

    #[test]
    fn moves_trailing_description_blocks_into_the_definition() {
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

        let Block::DefinitionList { items, .. } = &blocks[0] else {
            panic!("explicit option list should become definitions");
        };
        assert!(matches!(
            items[0].description.as_slice(),
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
