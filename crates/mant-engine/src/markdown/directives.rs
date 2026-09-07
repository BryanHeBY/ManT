//! Collect declarations from original events without reparsing masked Markdown.
use mant_ir::{
    DefinitionCase, DefinitionRole, Diagnostic, DiagnosticLevel, EntryKind,
    SemanticDocumentReference, SourceSpan, ValueDomain,
};
use pulldown_cmark::{Event, Tag, TagEnd};
use std::collections::BTreeMap;

/// Events and declarations always describe this exact borrowed source.
pub(super) struct PreparedMarkdown<'a> {
    pub(super) source: &'a str,
    pub(super) events: Vec<super::SpannedEvent<'a>>,
    pub(super) declarations: SemanticDeclarations,
}
impl<'a> PreparedMarkdown<'a> {
    pub(super) fn new(source: &'a str, diagnostics: &mut Vec<Diagnostic>) -> Self {
        let (events, declarations) = extract_semantic_directives(source, diagnostics);
        Self {
            source,
            events,
            declarations,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EntryDeclaration {
    pub(super) role: DefinitionRole,
    pub(super) case: DefinitionCase,
    pub(super) attached: AttachedValuePolicy,
    pub(super) source: SourceSpan,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SemanticDeclarations {
    pub(super) entries: BTreeMap<u32, EntryDeclaration>,
    pub(super) domains: BTreeMap<usize, DomainDeclarationState>,
}

#[derive(Debug, Clone)]
pub(super) struct DomainDeclaration {
    pub(super) value: ValueDomain,
    pub(super) source: SourceSpan,
}

#[derive(Debug, Clone)]
pub(super) enum DomainDeclarationState {
    Unique(DomainDeclaration),
    Ambiguous,
}

impl DomainDeclarationState {
    pub(super) fn into_unique(self) -> Option<DomainDeclaration> {
        match self {
            Self::Unique(declaration) => Some(declaration),
            Self::Ambiguous => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) enum AttachedValuePolicy {
    #[default]
    Infer,
    Fixed,
}

/// Consume directives in the original event stream. Never reparse masked
/// Markdown: HTML blocks can be the only boundary between adjacent lists.
fn extract_semantic_directives<'a>(
    source: &'a str,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<super::SpannedEvent<'a>>, SemanticDeclarations) {
    let mut masked = source.as_bytes().to_vec();
    let mut declarations = SemanticDeclarations::default();
    let lines = super::source::physical_lines(source).collect::<Vec<_>>();
    let line_starts = lines
        .iter()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset = offset.saturating_add(line.len());
            Some(start)
        })
        .collect::<Vec<_>>();
    let events = super::source::parser_events(source);

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

    let mut output = Vec::with_capacity(events.len());
    let mut events = events.into_iter();
    while let Some((event, range)) = events.next() {
        if event != Event::Start(Tag::HtmlBlock) {
            output.push((event, range));
            continue;
        }
        let start = (event, range);
        let mut body = Vec::new();
        for (event, range) in events.by_ref() {
            if event == Event::End(TagEnd::HtmlBlock) {
                if !body.is_empty() {
                    output.push(start);
                    output.append(&mut body);
                    output.push((event, range));
                }
                break;
            }
            let retained =
                std::str::from_utf8(&masked[range.clone()]).expect("ASCII masking preserves UTF-8");
            if !retained.trim().is_empty() {
                body.push((Event::Html(retained.to_owned().into()), range));
            }
        }
    }
    (output, declarations)
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
        let Some((Event::Start(Tag::List(_)), target_range)) = events.get(block_end_index + 1)
        else {
            semantic_diagnostic(
                diagnostics,
                source_span,
                "semantic-entry directive must immediately precede a list".to_owned(),
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
                let duplicate = match declarations.domains.entry(item_offset) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(DomainDeclarationState::Unique(declaration));
                        false
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        entry.insert(DomainDeclarationState::Ambiguous);
                        true
                    }
                };
                if duplicate {
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
    let mut choices = None;
    for field in fields.split_whitespace() {
        let Some((key, value)) = field.split_once('=') else {
            return Err(format!("invalid semantic value-domain field '{field}'"));
        };
        match key {
            "entries" if entries.is_none() => entries = Some(parse_domain_reference(value)?),
            "roles" if roles.is_none() => roles = Some(parse_domain_roles(value)?),
            "choices" if choices.is_none() => {
                choices = Some(match value {
                    "exhaustive" => true,
                    "open" => false,
                    _ => return Err("choices must be exhaustive or open".to_owned()),
                });
            }
            "entries" | "roles" | "choices" => {
                return Err(format!("duplicate semantic value-domain field '{key}'"));
            }
            _ => return Err(format!("unknown semantic value-domain field '{key}'")),
        }
    }
    let value = if let Some(exhaustive) = choices {
        if entries.is_some() || roles.is_some() {
            return Err("choices cannot be combined with entries or roles".to_owned());
        }
        ValueDomain::Choices { exhaustive }
    } else {
        ValueDomain::EntrySet {
            reference: entries
                .ok_or_else(|| "semantic value-domain directive requires entries=...".to_owned())?,
            entry_kinds: roles
                .ok_or_else(|| "semantic value-domain directive requires roles=...".to_owned())?,
            source: Some(source),
        }
    };
    Ok(DomainDeclaration { value, source })
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

pub(super) fn semantic_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    source: SourceSpan,
    message: String,
) {
    diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-entry-list".to_owned()),
        message,
        source: Some(source),
    });
}

pub(super) fn domain_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    source: SourceSpan,
    message: String,
) {
    diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: Some("markdown.semantic-value-domain".to_owned()),
        message,
        source: Some(source),
    });
}
