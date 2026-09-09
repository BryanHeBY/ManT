//! Explicit Markdown name grammar, distinct from native inference policy.
use super::{AttachedValuePolicy, diagnostics::EntryRejectionReason};
use crate::definitions::RecognizedName;
use crate::definitions::{environment_variable_alias, option_names_from_terms, option_prefix};
use mant_ir::{EntryKind, Inline};
pub(super) fn is_option_code(value: &str) -> bool {
    let terms = vec![vec![Inline::Code {
        value: value.to_owned(),
    }]];
    !option_names_from_terms(&terms).is_empty() && value.trim_start().starts_with('-')
}

pub(super) fn entry_names(
    value: &str,
    role: EntryKind,
    explicitly_declared: bool,
    attached: AttachedValuePolicy,
) -> Result<Vec<RecognizedName>, EntryRejectionReason> {
    let names = match role {
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        } if explicitly_declared => return option_entry_names(value, attached),
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        } => {
            return value
                .trim_start()
                .starts_with('-')
                .then(|| {
                    let terms = vec![vec![Inline::Code {
                        value: value.to_owned(),
                    }]];
                    crate::definitions::option_occurrences_from_terms(&terms)
                        .pop()
                        .unwrap_or_default()
                })
                .filter(|names| !names.is_empty())
                .ok_or(EntryRejectionReason::InvalidOptionName);
        }
        EntryKind::Command => plain_entry_name(value, is_command_name),
        EntryKind::EnvironmentVariable => environment_variable_alias(value)
            .map(|name| vec![name])
            .ok_or(EntryRejectionReason::InvalidEntryName),
        EntryKind::Variable => plain_entry_name(value, is_variable_name),
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Marker | mant_ir::ParameterKind::Operand,
        }
        | EntryKind::ConfigurationKey
        | EntryKind::Value
        | EntryKind::Term => plain_entry_name(value, |name| {
            !name.is_empty() && !name.contains(['\r', '\n'])
        }),
    }?;
    let start = value.len() - value.trim_start().len();
    Ok(names
        .into_iter()
        .map(|name| RecognizedName::contiguous(&name, start))
        .collect())
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
) -> Result<Vec<RecognizedName>, EntryRejectionReason> {
    let mut names = Vec::new();
    for alias in value.split([',', '|']).map(str::trim) {
        if let Some(parts) = crate::definitions::slash_option_forms(alias) {
            for part in parts {
                let name = dash_option_name(part, attached)?;
                names.push(RecognizedName::contiguous(
                    &name,
                    part.as_ptr() as usize - value.as_ptr() as usize,
                ));
            }
            continue;
        }
        let name = option_entry_name(alias, attached)?;
        names.push(RecognizedName::contiguous(
            &name,
            alias.as_ptr() as usize - value.as_ptr() as usize,
        ));
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
    if is_explicit_placeholder(value) {
        return true;
    }
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
        .is_some_and(|name| {
            !name.is_empty()
                && name.bytes().any(|byte| byte.is_ascii_alphabetic())
                && is_safe_segment(name)
        })
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
