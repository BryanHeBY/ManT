//! Recognizes semantic entries in ordinary Markdown lists.
//!
//! Markdown has no portable definition-list syntax. `ManT` therefore treats a
//! complete bullet list as semantic options only when every item starts with
//! one or more code spans containing options and an explicit description
//! delimiter, for example ``- `-h`, `--help`: Show help.``.

use std::collections::BTreeMap;

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Diagnostic,
    DiagnosticLevel, EntryKind, Inline, LinkTarget, ListItem, ListKind, SemanticDocumentReference,
    SourceSpan, ValueDomain,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::block::block_source;
use crate::definitions::{environment_variable_alias, option_names_from_terms, option_prefix};

#[derive(Debug, Clone, Copy)]
pub(super) struct EntryDeclaration {
    role: DefinitionRole,
    case: DefinitionCase,
    attached: AttachedValuePolicy,
    pub(super) source: SourceSpan,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SemanticDeclarations {
    pub(super) entries: BTreeMap<u32, EntryDeclaration>,
    pub(super) domains: BTreeMap<usize, DomainDeclaration>,
}

#[derive(Debug, Clone)]
pub(super) struct DomainDeclaration {
    value: ValueDomain,
    pub(super) source: SourceSpan,
}

#[derive(Debug, Clone, Copy, Default)]
enum AttachedValuePolicy {
    #[default]
    Infer,
    Fixed,
}

/// Remove invisible semantic directives while retaining source offsets.
pub(super) fn extract_semantic_directives(
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Option<String>, SemanticDeclarations) {
    let mut masked = source.as_bytes().to_vec();
    let mut declarations = SemanticDeclarations::default();
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let line_starts = lines
        .iter()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset = offset.saturating_add(line.len());
            Some(start)
        })
        .collect::<Vec<_>>();
    let events = Parser::new_ext(source, super::markdown_options())
        .into_offset_iter()
        .collect::<Vec<_>>();

    collect_entry_declarations(
        &events,
        &lines,
        &line_starts,
        &mut masked,
        &mut declarations,
        diagnostics,
    );
    collect_domain_declarations(
        &events,
        &lines,
        &line_starts,
        &mut masked,
        &mut declarations,
        diagnostics,
    );

    let masked = (!declarations.entries.is_empty()
        || !declarations.domains.is_empty()
        || masked.as_slice() != source.as_bytes())
    .then(|| String::from_utf8(masked).expect("masking ASCII preserves UTF-8"));
    (masked, declarations)
}

fn collect_entry_declarations(
    events: &[(Event<'_>, std::ops::Range<usize>)],
    lines: &[&str],
    line_starts: &[usize],
    masked: &mut [u8],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (event_index, (event, range)) in events.iter().enumerate() {
        let Event::Html(raw) = event else {
            continue;
        };
        if !is_semantic_directive(raw, "mant:entries") {
            continue;
        }
        let Some(block_end_index) = events[event_index + 1..]
            .iter()
            .position(|(event, _)| matches!(event, Event::End(TagEnd::HtmlBlock)))
            .map(|relative| event_index + relative + 1)
        else {
            continue;
        };
        let index = source_line_index(line_starts, range.start);
        let line = lines[index];
        let without_newline = line.trim_end_matches(['\r', '\n']);
        let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let Some(declaration) = read_declaration(
            without_newline,
            line_starts[index],
            line_number,
            masked,
            diagnostics,
        ) else {
            continue;
        };
        let source_span = declaration.source;
        let Some((Event::Start(Tag::List(None)), target_range)) = events.get(block_end_index + 1)
        else {
            semantic_diagnostic(
                diagnostics,
                source_span,
                "semantic-entry directive must immediately precede a complete bullet list"
                    .to_owned(),
            );
            continue;
        };
        let target_line = u32::try_from(source_line_index(line_starts, target_range.start) + 1)
            .unwrap_or(u32::MAX);
        if declarations
            .entries
            .insert(target_line, declaration)
            .is_some()
        {
            semantic_diagnostic(
                diagnostics,
                source_span,
                "more than one semantic-entry directive targets the same list".to_owned(),
            );
        }
    }
}

fn collect_domain_declarations(
    events: &[(Event<'_>, std::ops::Range<usize>)],
    lines: &[&str],
    line_starts: &[usize],
    masked: &mut [u8],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut item_offsets = Vec::new();
    for (event_index, (event, range)) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::Item) => {
                item_offsets.push(first_item_block_offset(events, event_index));
            }
            Event::End(TagEnd::Item) => {
                item_offsets.pop();
            }
            Event::Html(raw) if is_semantic_directive(raw, "mant:domain") => {
                let index = source_line_index(line_starts, range.start);
                let line = lines[index].trim_end_matches(['\r', '\n']);
                let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
                let Some(item_offset) = item_offsets.last().copied().flatten() else {
                    let source_span = directive_source(line, line_starts[index], line_number);
                    mask_directive(line, line_starts[index], masked);
                    domain_diagnostic(
                        diagnostics,
                        source_span,
                        "semantic value-domain directive must be inside a list item".to_owned(),
                    );
                    continue;
                };
                let Some(declaration) = read_domain_declaration(
                    line,
                    line_starts[index],
                    line_number,
                    masked,
                    diagnostics,
                ) else {
                    continue;
                };
                let source_span = declaration.source;
                if declarations
                    .domains
                    .insert(item_offset, declaration)
                    .is_some()
                {
                    domain_diagnostic(
                        diagnostics,
                        source_span,
                        "more than one semantic value-domain directive targets the same entry"
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
    }
}

fn first_item_block_offset(
    events: &[(Event<'_>, std::ops::Range<usize>)],
    item_index: usize,
) -> Option<usize> {
    for (event, range) in &events[item_index + 1..] {
        match event {
            Event::Start(_) => return Some(range.start),
            Event::End(TagEnd::Item) => break,
            _ => {}
        }
    }
    None
}

fn is_semantic_directive(raw: &str, name: &str) -> bool {
    raw.trim()
        .strip_prefix("<!--")
        .and_then(|value| value.strip_suffix("-->"))
        .map(str::trim)
        .and_then(|value| value.strip_prefix(name))
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(char::is_whitespace))
}

fn source_line_index(line_starts: &[usize], offset: usize) -> usize {
    line_starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1)
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

fn read_domain_declaration(
    line: &str,
    offset: usize,
    line_number: u32,
    masked: &mut [u8],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DomainDeclaration> {
    let source = directive_source(line, offset, line_number);
    let Some(comment_end) = mask_directive(line, offset, masked) else {
        domain_diagnostic(
            diagnostics,
            source,
            "unterminated semantic value-domain directive".to_owned(),
        );
        return None;
    };
    let comment_start = line
        .find("<!--")
        .expect("a recognized directive starts with an HTML comment");
    if !line[..comment_start].trim().is_empty() || !line[comment_end..].trim().is_empty() {
        domain_diagnostic(
            diagnostics,
            source,
            "semantic value-domain directive must be the only construct on its line".to_owned(),
        );
        return None;
    }
    match parse_domain_declaration(&line[comment_start..comment_end], source) {
        Ok(declaration) => Some(declaration),
        Err(message) => {
            domain_diagnostic(diagnostics, source, message);
            None
        }
    }
}

fn directive_source(line: &str, offset: usize, line_number: u32) -> SourceSpan {
    let comment_start = line
        .find("<!--")
        .expect("a recognized directive starts with an HTML comment");
    let comment_end = line[comment_start..]
        .find("-->")
        .map(|relative| comment_start + relative + 3);
    SourceSpan {
        byte_range: Some(mant_ir::TextRange::new(
            mant_ir::TextSize::from_usize_saturating(offset + comment_start),
            mant_ir::TextSize::from_usize_saturating(offset + comment_end.unwrap_or(line.len())),
        )),
        line: line_number,
        column: u32::try_from(comment_start + 1).unwrap_or(u32::MAX),
        end_line: Some(line_number),
        end_column: comment_end.map(|end| u32::try_from(end + 1).unwrap_or(u32::MAX)),
    }
}

fn mask_directive(line: &str, offset: usize, masked: &mut [u8]) -> Option<usize> {
    let comment_start = line
        .find("<!--")
        .expect("a recognized directive starts with an HTML comment");
    let comment_end = line[comment_start..]
        .find("-->")
        .map(|relative| comment_start + relative + 3);
    masked[offset + comment_start..offset + comment_end.unwrap_or(line.len())].fill(b' ');
    comment_end
}

fn parse_domain_declaration(value: &str, source: SourceSpan) -> Result<DomainDeclaration, String> {
    let Some(fields) = value
        .strip_prefix("<!--")
        .and_then(|value| value.strip_suffix("-->"))
        .map(str::trim)
        .and_then(|value| strip_directive_name(value, "mant:domain"))
    else {
        return Err("malformed semantic value-domain directive".to_owned());
    };
    let mut entries = None;
    let mut roles = None;
    for field in fields.split_whitespace() {
        let Some((key, value)) = field.split_once('=') else {
            return Err(format!("invalid semantic value-domain field '{field}'"));
        };
        match key {
            "entries" if entries.is_none() => entries = Some(parse_domain_reference(value)?),
            "roles" if roles.is_none() => roles = Some(parse_domain_roles(value)?),
            "entries" | "roles" => {
                return Err(format!("duplicate semantic value-domain field '{key}'"));
            }
            _ => return Err(format!("unknown semantic value-domain field '{key}'")),
        }
    }
    Ok(DomainDeclaration {
        value: ValueDomain::EntrySet {
            reference: entries
                .ok_or_else(|| "semantic value-domain directive requires entries=...".to_owned())?,
            entry_kinds: roles
                .ok_or_else(|| "semantic value-domain directive requires roles=...".to_owned())?,
            source: Some(source),
        },
        source,
    })
}

fn parse_domain_reference(value: &str) -> Result<SemanticDocumentReference, String> {
    if let Some(rest) = value.strip_prefix("manual/") {
        let Some((manual_section, name)) = rest.split_once('/') else {
            return Err("manual entry domains use manual/<section>/<name>".to_owned());
        };
        let reference = SemanticDocumentReference::Manual {
            name: name.to_owned(),
            manual_section: Some(manual_section.to_owned()),
        };
        if !reference.is_well_formed() {
            return Err("manual entry domains use manual/<section>/<name>".to_owned());
        }
        return Ok(reference);
    }
    let Some((name, fragment)) = super::inline::markdown_document_reference(value) else {
        return Err(
            "entry domains require a relative Markdown path or manual/<section>/<name>".to_owned(),
        );
    };
    if fragment.is_some() {
        return Err("entry domains must reference a complete document, not a fragment".to_owned());
    }
    let reference = SemanticDocumentReference::Document { name, fragment };
    reference
        .is_well_formed()
        .then_some(reference)
        .ok_or_else(|| {
            "entry domains require a relative Markdown path or manual/<section>/<name>".to_owned()
        })
}

fn parse_domain_roles(value: &str) -> Result<Vec<EntryKind>, String> {
    let mut roles = Vec::new();
    for role_name in value.split(',') {
        let role = match role_name {
            "option" => EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            "marker" => EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Marker,
            },
            "operand" => EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Operand,
            },
            "command" => EntryKind::Command,
            "configuration-key" => EntryKind::ConfigurationKey,
            "environment-variable" => EntryKind::EnvironmentVariable,
            "variable" => EntryKind::Variable,
            "value" => EntryKind::Value,
            "term" => EntryKind::Term,
            "" => return Err("semantic value-domain roles must not be empty".to_owned()),
            _ => return Err(format!("unknown semantic value-domain role '{role_name}'")),
        };
        if roles.contains(&role) {
            return Err(format!(
                "duplicate semantic value-domain role '{role_name}'"
            ));
        }
        roles.push(role);
    }
    Ok(roles)
}

fn parse_declaration(value: &str, source: SourceSpan) -> Result<EntryDeclaration, String> {
    let Some(fields) = value
        .strip_prefix("<!--")
        .and_then(|value| value.strip_suffix("-->"))
        .map(str::trim)
        .and_then(|value| strip_directive_name(value, "mant:entries"))
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
                    "marker" => DefinitionRole::Marker,
                    "operand" => DefinitionRole::Operand,
                    "command" => DefinitionRole::Command,
                    "configuration-key" => DefinitionRole::ConfigurationKey,
                    "environment-variable" => DefinitionRole::EnvironmentVariable,
                    "variable" => DefinitionRole::Variable,
                    "value" => DefinitionRole::Value,
                    "term" => DefinitionRole::Term,
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

fn strip_directive_name<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let suffix = value.strip_prefix(name)?;
    (suffix.is_empty() || suffix.starts_with(char::is_whitespace)).then(|| suffix.trim_start())
}

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
                let value_domain = item
                    .blocks
                    .first()
                    .and_then(block_source)
                    .and_then(|source| source.byte_range)
                    .and_then(|range| {
                        declarations
                            .domains
                            .remove(&usize::try_from(range.start.get()).unwrap_or(usize::MAX))
                    })
                    .map(|declaration| declaration.value);
                entry_definition(item, signature, role, case, value_domain)
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
    for (delimiter_inline, inline) in children.iter().enumerate() {
        match inline {
            Inline::Code { value } => {
                leading_term.get_or_insert(value.as_str());
                let parsed = entry_names(value, role, explicitly_declared, attached)
                    .map_err(|reason| EntryRejection::new(reason, Some(value), source))?;
                extend_unique(&mut names, parsed);
            }
            Inline::Link {
                target,
                children: linked,
                ..
            } if matches!(
                target,
                LinkTarget::Document { .. } | LinkTarget::Manual { .. }
            ) && matches!(linked.as_slice(), [Inline::Code { .. }]) =>
            {
                let [Inline::Code { value }] = linked.as_slice() else {
                    unreachable!("the match guard accepts exactly one code child");
                };
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
            id: String::new().into(),
            role,
            case,
            names: signature.names,
            value_domain,
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

fn semantic_diagnostic(diagnostics: &mut Vec<Diagnostic>, source: SourceSpan, message: String) {
    diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-entry-list".to_owned()),
        message,
        source: Some(source),
    });
}

fn domain_diagnostic(diagnostics: &mut Vec<Diagnostic>, source: SourceSpan, message: String) {
    diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-value-domain".to_owned()),
        message,
        source: Some(source),
    });
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
