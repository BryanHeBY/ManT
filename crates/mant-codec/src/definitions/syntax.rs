//! Source-neutral grammars for semantic definition names and lexical evidence.
//! Context selects a grammar; one recognition result carries both selectable
//! spellings and their original ranges to the binding mapper.
use super::{RecognizedName, context::DefinitionContext};
use mant_ir::inline_plain_text as plain_text;
use mant_ir::{DefinitionItem, EntryKind, NameCase, ParameterKind};

mod commands;
mod decision;
mod declaration;
mod forms;
mod head;
mod named;
mod options;
pub(super) use head::is_inferred_head;
pub(super) use named::is_value_name;
pub(crate) use named::{environment_variable_alias, environment_variable_body};
use named::{is_configuration_key, is_variable_term};
#[cfg(test)]
pub(super) use options::option_names;
pub(crate) use options::{
    option_names_from_terms, option_occurrences_from_terms, option_prefix, slash_option_forms,
};

pub(super) struct InferredIdentity {
    pub(super) kind: EntryKind,
    pub(super) case: NameCase,
    pub(super) names: Vec<String>,
    pub(super) occurrences: Vec<Vec<RecognizedName>>,
}

pub(super) fn infer_identity(
    item: &DefinitionItem,
    context: DefinitionContext,
    hint: Option<super::NativeHeadRole>,
) -> InferredIdentity {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    let trimmed = first.trim();
    let (mut kind, mut case) = decision::select_kind(trimmed, context, hint);
    let mut occurrences = if hint == Some(super::NativeHeadRole::Option) {
        options::native_option_occurrences(&item.terms)
    } else if hint == Some(super::NativeHeadRole::Environment) {
        item.terms
            .iter()
            .map(|term| {
                forms::environment_prefix(term)
                    .and_then(|prefix| named::environment_occurrences(&prefix))
                    .unwrap_or_default()
            })
            .collect()
    } else {
        name_occurrences(item, kind)
    };
    if hint == Some(super::NativeHeadRole::Literal) && occurrences.iter().all(Vec::is_empty) {
        occurrences = name_occurrences(item, EntryKind::Command);
        if occurrences.iter().all(Vec::is_empty) {
            occurrences = item
                .terms
                .iter()
                .map(|term| {
                    let prefix = forms::literal_prefix(term);
                    let name = prefix.trim();
                    if matches!(name, "-" | "--") {
                        vec![RecognizedName::contiguous(
                            name,
                            prefix.len() - prefix.trim_start().len(),
                        )]
                    } else {
                        Vec::new()
                    }
                })
                .collect();
        }
        kind = EntryKind::Term;
        case = NameCase::Sensitive;
    }
    if occurrences.iter().all(Vec::is_empty)
        && !matches!(
            hint,
            Some(super::NativeHeadRole::Option | super::NativeHeadRole::Environment)
        )
    {
        occurrences = name_occurrences(item, EntryKind::Term);
        kind = EntryKind::Term;
        case = NameCase::Sensitive;
    }
    let mut names = Vec::new();
    let all = || occurrences.iter().flatten();
    // Preserve the established native order: ordinary dash options first,
    // followed by finite sign alternations and plus-prefixed forms.
    let is_extra = |found: &&RecognizedName| {
        matches!(kind, EntryKind::Parameter { .. })
            && (found.name.starts_with('+') || found.parts.len() > 1)
    };
    for found in all()
        .filter(|found| !is_extra(found))
        .chain(all().filter(is_extra))
    {
        if !names.contains(&found.name) {
            names.push(found.name.clone());
        }
    }
    let (kind, case) = if names.is_empty()
        && !matches!(
            hint,
            Some(super::NativeHeadRole::Option | super::NativeHeadRole::Environment)
        ) {
        (EntryKind::Term, NameCase::Sensitive)
    } else {
        (kind, case)
    };
    InferredIdentity {
        kind,
        case,
        names,
        occurrences,
    }
}

/// The same role-specific grammars produce both names and their lexical
/// evidence. Binding only maps these ranges to styled IR leaves; it does not
/// have another definition of punctuation or argument boundaries.
pub(super) fn name_occurrences(
    item: &DefinitionItem,
    kind: EntryKind,
) -> Vec<Vec<super::RecognizedName>> {
    if let EntryKind::Parameter { parameter_kind } = kind {
        if parameter_kind == ParameterKind::Option {
            return options::parameter_occurrences(&item.terms);
        }
        // Marker/operand inference requires an exact complete first term,
        // not a prefix of a longer invocation. Repeated matching terms may
        // contribute evidence without promoting other terms into names.
        let first = item
            .terms
            .first()
            .map_or_else(String::new, |term| plain_text(term));
        return item
            .terms
            .iter()
            .map(|term| {
                let text = plain_text(term);
                let trimmed = text.trim();
                if trimmed == first.trim() && !trimmed.is_empty() {
                    vec![RecognizedName::contiguous(
                        trimmed,
                        text.len() - text.trim_start().len(),
                    )]
                } else {
                    Vec::new()
                }
            })
            .collect();
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
                    named::environment_occurrences(&text).unwrap_or_default()
                }
                EntryKind::Variable | EntryKind::ConfigurationKey | EntryKind::Value => {
                    let validate = match kind {
                        EntryKind::Variable => is_variable_term,
                        EntryKind::ConfigurationKey => is_configuration_key,
                        _ => is_value_name,
                    };
                    named::named_occurrences(&text, validate).unwrap_or_default()
                }
                EntryKind::Command => {
                    if let Some((name, _)) = super::context::key_binding_command_form(&text) {
                        return vec![locate(&text, name)];
                    }
                    let mut offset = 0;
                    forms::declaration_groups(term)
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
                EntryKind::Term => {
                    named::named_occurrences(&text, is_variable_term).unwrap_or_default()
                }
                EntryKind::Parameter { .. } => Vec::new(),
            }
        })
        .collect()
}
