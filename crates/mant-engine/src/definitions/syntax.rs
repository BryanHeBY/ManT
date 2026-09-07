//! Source-neutral grammars for semantic definition names.
//!
//! These recognizers deliberately consume complete authored forms. Section
//! context decides which grammar to try; this module only decides whether a
//! spelling is trustworthy enough to expose as an addressable entry.

use mant_ir::{DefinitionItem, EntryKind, NameCase};

use crate::inline::plain_text;

use super::context::DefinitionContext;

mod commands;
mod forms;
mod named;
mod options;
use commands::command_names;
pub(super) use named::environment_names_from_terms;
pub(super) use named::is_value_name;
use named::{environment_names, is_configuration_key, is_variable_term, named_term};
pub(crate) use named::{environment_variable_alias, environment_variable_body};
#[cfg(test)]
pub(super) use options::option_names;
use options::parameter_identity;
pub(crate) use options::{
    option_names_from_terms, option_occurrences_from_terms, option_prefix, slash_option_forms,
};

pub(super) fn infer_identity(
    item: &DefinitionItem,
    context: DefinitionContext,
) -> (EntryKind, NameCase, Vec<String>) {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    let trimmed = first.trim();
    match context {
        DefinitionContext::Commands
            if trimmed.starts_with(['-', '+']) || trimmed.starts_with("[-+]") =>
        {
            parameter_identity(item, trimmed)
        }
        DefinitionContext::Commands => {
            let names = command_names(item);
            if names.is_empty() {
                (EntryKind::Term, NameCase::Sensitive, Vec::new())
            } else {
                (EntryKind::Command, NameCase::Sensitive, names)
            }
        }
        DefinitionContext::EnvironmentVariables => named_identity(
            EntryKind::EnvironmentVariable,
            NameCase::Sensitive,
            environment_names(item),
        ),
        DefinitionContext::Variables => named_identity(
            EntryKind::Variable,
            NameCase::Sensitive,
            named_term(item, is_variable_term),
        ),
        DefinitionContext::ConfigurationKeys => named_identity(
            EntryKind::ConfigurationKey,
            NameCase::Insensitive,
            named_term(item, is_configuration_key),
        ),
        DefinitionContext::Values => named_identity(
            EntryKind::Value,
            NameCase::Sensitive,
            named_term(item, is_value_name),
        ),
        DefinitionContext::Parameters => parameter_identity(item, trimmed),
        DefinitionContext::Generic if trimmed.starts_with('-') => parameter_identity(item, trimmed),
        DefinitionContext::Generic => (EntryKind::Term, NameCase::Sensitive, Vec::new()),
    }
}

fn named_identity(
    role: EntryKind,
    case: NameCase,
    names: Vec<String>,
) -> (EntryKind, NameCase, Vec<String>) {
    if names.is_empty() {
        (EntryKind::Term, NameCase::Sensitive, names)
    } else {
        (role, case, names)
    }
}

/// The same role-specific grammars produce both names and their lexical
/// evidence. Binding only maps these ranges to styled IR leaves; it does not
/// have another definition of punctuation or argument boundaries.
pub(super) fn name_occurrences(
    item: &DefinitionItem,
    kind: EntryKind,
) -> Vec<Vec<super::RecognizedName>> {
    if matches!(kind, EntryKind::Parameter { .. }) {
        return options::parameter_occurrences(&item.terms);
    }
    item.terms
        .iter()
        .map(|term| {
            let text = plain_text(term);
            let locate = |part: &str, name: &str| {
                super::RecognizedName::contiguous(
                    name,
                    part.as_ptr() as usize - text.as_ptr() as usize
                        + part.find(name).expect("grammar returns a visible name"),
                )
            };
            match kind {
                EntryKind::EnvironmentVariable => {
                    let parts = if text.contains('=') {
                        vec![text.as_str()]
                    } else {
                        text.split([',', '|']).collect()
                    };
                    parts
                        .into_iter()
                        .filter_map(|part| {
                            environment_variable_alias(part).map(|name| locate(part, &name))
                        })
                        .collect()
                }
                EntryKind::Variable | EntryKind::ConfigurationKey | EntryKind::Value => {
                    let validate = match kind {
                        EntryKind::Variable => is_variable_term,
                        EntryKind::ConfigurationKey => is_configuration_key,
                        _ => is_value_name,
                    };
                    text.split(',')
                        .filter_map(|part| {
                            named::named_term_name(part, validate).map(|name| locate(part, name))
                        })
                        .collect()
                }
                EntryKind::Command => {
                    if let Some((name, _)) = super::context::key_binding_command_form(&text) {
                        return vec![locate(&text, name)];
                    }
                    let mut offset = 0;
                    forms::alias_groups(term)
                        .into_iter()
                        .filter_map(|group| {
                            let text = plain_text(&group);
                            let start = offset;
                            offset += text.len() + 1;
                            let name =
                                commands::leading_styled_command_name(&group).or_else(|| {
                                    commands::command_name_from_authored_form(&text)
                                        .map(str::to_owned)
                                })?;
                            let start =
                                start + text.find(&name).expect("grammar returns the command head");
                            Some(super::RecognizedName::contiguous(&name, start))
                        })
                        .collect()
                }
                _ => Vec::new(),
            }
        })
        .collect()
}
