//! Closed directive grammar and source-preserving comment masking.
use super::{
    AttachedValuePolicy, DomainDeclaration, EntryDeclaration, domain_diagnostic,
    semantic_diagnostic,
};
use mant_ir::{Diagnostic, DocumentReference, EntryKind, NameCase, SourceSpan, ValueDomain};
pub(super) fn is_semantic_directive(raw: &str, name: &str) -> bool {
    raw.trim()
        .strip_prefix("<!--")
        .and_then(|value| value.strip_suffix("-->"))
        .map(str::trim)
        .and_then(|value| value.strip_prefix(name))
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(char::is_whitespace))
}

pub(super) fn source_line_index(line_starts: &[usize], offset: usize) -> usize {
    line_starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1)
}

pub(super) fn read_declaration(
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

pub(super) fn read_domain_declaration(
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

pub(super) fn directive_source(line: &str, offset: usize, line_number: u32) -> SourceSpan {
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

pub(super) fn mask_directive(line: &str, offset: usize, masked: &mut [u8]) -> Option<usize> {
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

fn parse_domain_reference(value: &str) -> Result<DocumentReference, String> {
    if let Some(rest) = value.strip_prefix("manual/") {
        let Some((manual_section, name)) = rest.split_once('/') else {
            return Err("manual entry domains use manual/<section>/<name>".to_owned());
        };
        let reference = DocumentReference::Manual {
            name: name.to_owned(),
            manual_section: Some(manual_section.to_owned()),
        };
        if !reference.is_well_formed() {
            return Err("manual entry domains use manual/<section>/<name>".to_owned());
        }
        return Ok(reference);
    }
    let Some((name, fragment)) = super::super::inline::markdown_document_reference(value) else {
        return Err(
            "entry domains require a relative Markdown path or manual/<section>/<name>".to_owned(),
        );
    };
    if fragment.is_some() {
        return Err("entry domains must reference a complete document, not a fragment".to_owned());
    }
    let reference = DocumentReference::Document { name, fragment };
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
                    _ => return Err(format!("unknown semantic-entry role '{value}'")),
                });
            }
            "case" if case.is_none() => {
                case = Some(match value {
                    "sensitive" => NameCase::Sensitive,
                    "insensitive" => NameCase::Insensitive,
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
    if attached.is_some()
        && role
            != (EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            })
    {
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
