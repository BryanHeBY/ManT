//! Recognizes semantic entries in ordinary Markdown lists.
//!
//! Markdown has no portable definition-list syntax. `ManT` therefore treats a
//! complete bullet list as semantic options only when every item starts with
//! one or more code spans containing options and an explicit description
//! delimiter, for example ``- `-h`, `--help`: Show help.``.

use std::collections::BTreeMap;

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Diagnostic,
    DiagnosticLevel, Inline, ListItem, ListKind, SourceSpan,
};

use crate::block::block_source;
use crate::definitions::{environment_variable_alias, option_names_from_terms, option_prefix};

#[derive(Debug, Clone, Copy)]
pub(super) struct EntryDeclaration {
    role: DefinitionRole,
    case: DefinitionCase,
    attached: AttachedValuePolicy,
    pub(super) source: SourceSpan,
}

#[derive(Debug, Clone, Copy, Default)]
enum AttachedValuePolicy {
    #[default]
    Infer,
    Fixed,
}

/// Remove invisible semantic-entry directives while retaining source offsets.
pub(super) fn extract_entry_directives(
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Option<String>, BTreeMap<u32, EntryDeclaration>) {
    let mut masked = source.as_bytes().to_vec();
    let mut declarations = BTreeMap::new();
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let mut offset = 0usize;
    let mut fence = None;

    for (index, line) in lines.iter().enumerate() {
        let without_newline = line.trim_end_matches(['\r', '\n']);
        let trimmed = without_newline.trim();
        let trimmed_start = without_newline.trim_start_matches(' ');
        let indentation = without_newline.len() - trimmed_start.len();
        if let Some((marker, width)) = fence {
            if is_closing_fence(trimmed_start, marker, width) {
                fence = None;
            }
            offset += line.len();
            continue;
        }
        if indentation <= 3
            && let Some(opening) = opening_fence(trimmed_start)
        {
            fence = Some(opening);
            offset += line.len();
            continue;
        }
        if indentation >= 4 {
            offset += line.len();
            continue;
        }
        if !trimmed.starts_with("<!-- mant:entries") {
            offset += line.len();
            continue;
        }
        let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let Some(declaration) = read_declaration(
            without_newline,
            offset,
            line_number,
            &mut masked,
            diagnostics,
        ) else {
            offset += line.len();
            continue;
        };
        let source_span = declaration.source;
        let target = lines[index + 1..]
            .iter()
            .enumerate()
            .find(|(_, candidate)| !candidate.trim().is_empty())
            .map(|(relative, candidate)| (index + relative + 2, candidate.trim_start()));
        let Some((target_line, target_text)) = target else {
            semantic_diagnostic(
                diagnostics,
                source_span,
                "semantic-entry directive is not followed by a bullet list".to_owned(),
            );
            offset += line.len();
            continue;
        };
        if !is_bullet_line(target_text) {
            semantic_diagnostic(
                diagnostics,
                source_span,
                "semantic-entry directive must immediately precede a complete bullet list"
                    .to_owned(),
            );
            offset += line.len();
            continue;
        }
        let target_line = u32::try_from(target_line).unwrap_or(u32::MAX);
        if declarations.insert(target_line, declaration).is_some() {
            semantic_diagnostic(
                diagnostics,
                source_span,
                "more than one semantic-entry directive targets the same list".to_owned(),
            );
        }
        offset += line.len();
    }

    let masked = (!declarations.is_empty() || masked.as_slice() != source.as_bytes())
        .then(|| String::from_utf8(masked).expect("masking ASCII preserves UTF-8"));
    (masked, declarations)
}

fn read_declaration(
    line: &str,
    offset: usize,
    line_number: u32,
    masked: &mut [u8],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<EntryDeclaration> {
    let comment_start = line
        .find("<!--")
        .expect("a recognized directive starts with an HTML comment");
    let comment_end = line[comment_start..]
        .find("-->")
        .map(|relative| comment_start + relative + 3);
    let source = SourceSpan {
        byte_range: Some(mant_ir::TextRange::new(
            mant_ir::TextSize::from_usize_saturating(offset + comment_start),
            mant_ir::TextSize::from_usize_saturating(offset + comment_end.unwrap_or(line.len())),
        )),
        line: line_number,
        column: u32::try_from(comment_start + 1).unwrap_or(u32::MAX),
        end_line: Some(line_number),
        end_column: comment_end.map(|end| u32::try_from(end + 1).unwrap_or(u32::MAX)),
    };
    let Some(comment_end) = comment_end else {
        masked[offset + comment_start..offset + line.len()].fill(b' ');
        semantic_diagnostic(
            diagnostics,
            source,
            "unterminated semantic-entry directive".to_owned(),
        );
        return None;
    };
    masked[offset + comment_start..offset + comment_end].fill(b' ');
    if !line[..comment_start].trim().is_empty() || !line[comment_end..].trim().is_empty() {
        semantic_diagnostic(
            diagnostics,
            source,
            "semantic-entry directive must be the only construct on its line".to_owned(),
        );
        return None;
    }
    match parse_declaration(&line[comment_start..comment_end], source) {
        Ok(declaration) => Some(declaration),
        Err(message) => {
            semantic_diagnostic(diagnostics, source, message);
            None
        }
    }
}

fn opening_fence(value: &str) -> Option<(u8, usize)> {
    let marker = *value.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let width = value.bytes().take_while(|byte| *byte == marker).count();
    (width >= 3).then_some((marker, width))
}

fn is_closing_fence(value: &str, marker: u8, opening_width: usize) -> bool {
    let width = value.bytes().take_while(|byte| *byte == marker).count();
    width >= opening_width && value[width..].trim().is_empty()
}

fn parse_declaration(value: &str, source: SourceSpan) -> Result<EntryDeclaration, String> {
    let Some(fields) = value
        .strip_prefix("<!--")
        .and_then(|value| value.strip_suffix("-->"))
        .map(str::trim)
        .and_then(|value| value.strip_prefix("mant:entries"))
    else {
        return Err("malformed semantic-entry directive".to_owned());
    };
    let mut role = None;
    let mut case = None;
    let mut attached = None;
    for field in fields.split_whitespace() {
        let Some((key, value)) = field.split_once('=') else {
            return Err(format!("invalid semantic-entry field '{field}'"));
        };
        match key {
            "role" if role.is_none() => {
                role = Some(match value {
                    "option" => DefinitionRole::Option,
                    "command" => DefinitionRole::Command,
                    "environment-variable" => DefinitionRole::EnvironmentVariable,
                    "variable" => DefinitionRole::Variable,
                    _ => return Err(format!("unknown semantic-entry role '{value}'")),
                });
            }
            "case" if case.is_none() => {
                case = Some(match value {
                    "sensitive" => DefinitionCase::Sensitive,
                    "insensitive" => DefinitionCase::Insensitive,
                    _ => return Err(format!("unknown semantic-entry case policy '{value}'")),
                });
            }
            "attached" if attached.is_none() => {
                attached = Some(match value {
                    "infer" => AttachedValuePolicy::Infer,
                    "fixed" => AttachedValuePolicy::Fixed,
                    _ => {
                        return Err(format!(
                            "unknown semantic-entry attached-value policy '{value}'"
                        ));
                    }
                });
            }
            "role" | "case" | "attached" => {
                return Err(format!("duplicate semantic-entry field '{key}'"));
            }
            _ => return Err(format!("unknown semantic-entry field '{key}'")),
        }
    }
    let role = role.ok_or_else(|| "semantic-entry directive requires role=...".to_owned())?;
    if attached.is_some() && role != DefinitionRole::Option {
        return Err("semantic-entry field 'attached' applies only to role=option".to_owned());
    }
    Ok(EntryDeclaration {
        role,
        case: case.ok_or_else(|| {
            "semantic-entry directive requires case=sensitive|insensitive".to_owned()
        })?,
        attached: attached.unwrap_or_default(),
        source,
    })
}

fn is_bullet_line(value: &str) -> bool {
    matches!(value.as_bytes(), [b'-' | b'*' | b'+', b' ' | b'\t', ..])
}

/// Convert unambiguous entry lists without changing mixed or prose lists.
pub(super) fn normalize_entry_lists(
    blocks: &mut Vec<Block>,
    declarations: &mut BTreeMap<u32, EntryDeclaration>,
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
        let declaration = source.and_then(|source| declarations.remove(&source.line));
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
            .map(|(item, signature)| entry_definition(item, signature, role, case))
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
    declarations: &mut BTreeMap<u32, EntryDeclaration>,
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
    EntryRejectionReason::ALL
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
    for (delimiter_inline, inline) in children.iter().enumerate() {
        match inline {
            Inline::Code { value } => {
                leading_term.get_or_insert(value.as_str());
                let parsed = entry_names(value, role, explicitly_declared, attached)
                    .map_err(|reason| EntryRejection::new(reason, Some(value), source))?;
                extend_unique(&mut names, parsed);
            }
            Inline::Text { value } => {
                if let Some((delimiter_byte, delimiter_width)) = delimiter_location(value) {
                    if names.is_empty() {
                        return Err(EntryRejection::new(
                            EntryRejectionReason::MissingLeadingCode,
                            leading_term,
                            source,
                        ));
                    }
                    if !is_alias_separator(&value[..delimiter_byte]) {
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
                    });
                }
                if names.is_empty() {
                    return Err(EntryRejection::new(
                        EntryRejectionReason::MissingLeadingCode,
                        leading_term,
                        source,
                    ));
                }
                if !is_alias_separator(value) {
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

/// Move one previously validated item into its semantic definition.
fn entry_definition(
    item: ListItem,
    signature: EntrySignature,
    role: DefinitionRole,
    case: DefinitionCase,
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
            id: String::new().into(),
            role,
            case,
            names: signature.names,
        }),
        inline_term: false,
        terms: vec![terms],
        description,
        spacing_before_lines: None,
    }
}

fn apply_entry_signature(
    children: Vec<Inline>,
    signature: &EntrySignature,
) -> (Vec<Inline>, Vec<Inline>) {
    let mut terms = Vec::new();
    let mut description = Vec::new();
    for (index, inline) in children.into_iter().enumerate() {
        if index < signature.inline_index {
            terms.push(inline);
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
            terms.push(Inline::Text {
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
        DefinitionRole::Command => {
            plain_entry_names(value, is_command_name).ok_or(EntryRejectionReason::InvalidEntryName)
        }
        DefinitionRole::EnvironmentVariable => {
            environment_entry_names(value).ok_or(EntryRejectionReason::InvalidEntryName)
        }
        DefinitionRole::Variable => {
            plain_entry_names(value, is_variable_name).ok_or(EntryRejectionReason::InvalidEntryName)
        }
        DefinitionRole::Marker
        | DefinitionRole::Operand
        | DefinitionRole::ConfigurationKey
        | DefinitionRole::Value
        | DefinitionRole::Term => plain_entry_names(value, |name| {
            !name.is_empty() && !name.contains(['\r', '\n'])
        })
        .ok_or(EntryRejectionReason::InvalidEntryName),
    }
}

fn plain_entry_names(value: &str, validate: fn(&str) -> bool) -> Option<Vec<String>> {
    let names = value
        .split([',', '|'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!names.is_empty() && names.iter().all(|name| validate(name))).then_some(names)
}

fn is_command_name(value: &str) -> bool {
    !value.is_empty() && !value.contains(['\r', '\n']) && !value.starts_with(['-', '/'])
}

fn environment_entry_names(value: &str) -> Option<Vec<String>> {
    let names = value
        .split([',', '|'])
        .map(environment_variable_alias)
        .collect::<Option<Vec<_>>>()?;
    (!names.is_empty()).then_some(names)
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
        if alias.starts_with('-') && alias.contains('/') {
            let parts = alias.split('/').collect::<Vec<_>>();
            if parts.iter().all(|part| part.starts_with('-')) {
                for part in parts {
                    names.push(dash_option_name(part, attached)?);
                }
                continue;
            }
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

fn semantic_diagnostic(diagnostics: &mut Vec<Diagnostic>, source: SourceSpan, message: String) {
    diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-entry-list".to_owned()),
        message,
        source: Some(source),
    });
}

#[cfg(test)]
fn normalize_option_lists(blocks: &mut Vec<Block>) {
    normalize_entry_lists(blocks, &mut BTreeMap::new(), &mut Vec::new());
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
