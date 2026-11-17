//! Source-neutral grammars for semantic definition names and lexical evidence.
//! Context selects a grammar; one recognition result carries both selectable
//! spellings and their original ranges to the binding mapper.
use super::{RecognizedName, context::DefinitionContext, evidence::HeadOperands};
use mant_ir::inline_plain_text as plain_text;
use mant_ir::{DefinitionItem, EntryKind, NameCase, ParameterKind};

mod commands;
mod decision;
mod declaration;
mod eligibility;
mod forms;
mod head;
mod named;
#[cfg(feature = "roff")]
mod native_subject;
mod options;
mod scan;
use eligibility::{NameSource, OperandUses};
pub(super) use head::is_inferred_head;
pub(super) use head::is_inferred_head_with_operands;
pub(super) use head::recognize_inferred_head_with_operands;
pub(crate) use named::{environment_variable_alias, environment_variable_body};
use named::{is_configuration_key, is_variable_term};
pub(super) use named::{is_ordinal_marker, is_value_name};
#[cfg(test)]
pub(super) use options::option_names;
pub(crate) use options::{
    option_names_from_terms, option_occurrences_from_terms, option_prefix, slash_option_forms,
};
pub(super) use scan::DeclarationLimit;

pub(super) struct InferredIdentity {
    pub(super) kind: EntryKind,
    pub(super) case: NameCase,
    pub(super) names: Vec<String>,
    pub(super) occurrences: Vec<Vec<RecognizedName>>,
    pub(super) limit: Option<DeclarationLimit>,
}

pub(super) fn infer_identity(
    item: &DefinitionItem,
    context: DefinitionContext,
    hint: Option<super::NativeHeadRole>,
    operands: HeadOperands<'_>,
) -> InferredIdentity {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    #[cfg(feature = "roff")]
    if let Some(identity) = native_subject::identify(item, &first, operands) {
        return identity;
    }
    let trimmed = first.trim();
    let leading_option = hint.is_none()
        && operands
            .for_term(0)
            .is_some_and(|ranges| scan::has_leading_explicit_option(&first, ranges));
    let hint = hint.or(leading_option.then_some(super::NativeHeadRole::Option));
    let (mut kind, mut case) = decision::select_kind(trimmed, context, hint);
    let (mut occurrences, mut limit) = head_occurrences(item, kind, hint, operands);
    constrain_names(&mut occurrences, operands);
    if hint == Some(super::NativeHeadRole::Literal) && occurrences.iter().all(Vec::is_empty) {
        occurrences = name_occurrences_with_operands(item, EntryKind::Command, operands);
        constrain_names(&mut occurrences, operands);
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
            constrain_names(&mut occurrences, operands);
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
        occurrences = name_occurrences_with_operands(item, EntryKind::Term, operands);
        constrain_names(&mut occurrences, operands);
        kind = EntryKind::Term;
        case = NameCase::Sensitive;
    }
    if (hint.is_none() || leading_option)
        && item.terms.len() > 1
        && operands.proves_option()
        && let Some(mixed_kind) = continued_occurrences(
            item,
            &first,
            operands,
            leading_option,
            &mut occurrences,
            &mut limit,
        )
    {
        kind = mixed_kind;
        case = NameCase::Sensitive;
    }
    let names = ordered_names(&occurrences, kind);
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
        limit,
    }
}

fn head_occurrences(
    item: &DefinitionItem,
    kind: EntryKind,
    hint: Option<super::NativeHeadRole>,
    operands: HeadOperands<'_>,
) -> (Vec<Vec<RecognizedName>>, Option<DeclarationLimit>) {
    if hint == Some(super::NativeHeadRole::LiteralTerm) {
        let occurrences = item
            .terms
            .iter()
            .map(|term| {
                let text = plain_text(term);
                vec![RecognizedName::contiguous(
                    text.trim(),
                    text.len() - text.trim_start().len(),
                )]
            })
            .collect();
        return (occurrences, None);
    }
    let occurrences = match hint {
        Some(super::NativeHeadRole::Option) => {
            let found = options::native_option_occurrences(&item.terms, operands);
            // A complete accepted native option receipt establishes known
            // selector loss; a weak candidate alone cannot prove coverage loss.
            let limit = if operands.proves_option() {
                found.limit
            } else {
                None
            };
            return (found.occurrences, limit);
        }
        Some(super::NativeHeadRole::Environment) => item
            .terms
            .iter()
            .enumerate()
            .map(|(index, term)| {
                operands.for_term(index).map_or_else(Vec::new, |ranges| {
                    native_environment_occurrences(term.as_ref(), OperandUses::new(ranges))
                })
            })
            .collect(),
        _ => name_occurrences_with_operands(item, kind, operands),
    };
    (occurrences, None)
}

fn native_environment_occurrences(
    term: &[mant_ir::Inline],
    uses: OperandUses<'_>,
) -> Vec<RecognizedName> {
    let mut names = forms::environment_prefix(term)
        .and_then(|prefix| named::environment_occurrences_with_uses(&prefix, uses))
        .unwrap_or_default();
    if uses.has_parameters()
        && let Some(complete) = named::environment_occurrences_with_uses(&plain_text(term), uses)
    {
        // A styled parameter may stop the legacy prefix scan. An authored
        // delimiter outside that parameter still starts another declaration.
        names.extend(complete);
        names.sort_by_key(|name| name.parts[0].start);
        names.dedup_by(|left, right| left.parts == right.parts);
    }
    names
}

fn continued_occurrences(
    item: &DefinitionItem,
    first: &str,
    operands: HeadOperands<'_>,
    leading_option: bool,
    occurrences: &mut [Vec<RecognizedName>],
    limit: &mut Option<DeclarationLimit>,
) -> Option<EntryKind> {
    if leading_option {
        let mut mixed = false;
        for (index, (term, names)) in item.terms.iter().zip(occurrences).enumerate().skip(1) {
            if names.is_empty() {
                *names = lexical_term_occurrences(&plain_text(term), operands.for_term(index));
                constrain_term_names(names, operands.for_term(index));
                mixed |= !names.is_empty();
            }
        }
        return mixed.then_some(EntryKind::Term);
    }
    if options::retain_continued_option_names(&item.terms, operands, occurrences, limit)
        && occurrences[0].is_empty()
    {
        occurrences[0] = lexical_term_occurrences(first, operands.for_term(0));
        constrain_term_names(&mut occurrences[0], operands.for_term(0));
        if occurrences[0].is_empty() {
            return Some(EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            });
        }
        return Some(EntryKind::Term);
    }
    None
}

fn lexical_term_occurrences(
    text: &str,
    ranges: Option<&[super::NativeOperand]>,
) -> Vec<RecognizedName> {
    ranges.map_or_else(Vec::new, |ranges| {
        named::term_occurrences(text, OperandUses::new(ranges)).unwrap_or_default()
    })
}

fn constrain_names(occurrences: &mut [Vec<RecognizedName>], operands: HeadOperands<'_>) {
    for (index, names) in occurrences.iter_mut().enumerate() {
        constrain_term_names(names, operands.for_term(index));
    }
}

fn constrain_term_names(
    names: &mut Vec<RecognizedName>,
    operands: Option<&[super::NativeOperand]>,
) {
    let Some(ranges) = operands else {
        names.clear();
        return;
    };
    names.retain(|name| OperandUses::new(ranges).permits(name, NameSource::Lexical));
}

fn ordered_names(occurrences: &[Vec<RecognizedName>], kind: EntryKind) -> Vec<String> {
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
    names
}

/// The same role-specific grammars produce both names and their lexical
/// evidence. Binding only maps these ranges to styled IR leaves; it does not
/// have another definition of punctuation or argument boundaries.
pub(super) fn name_occurrences(
    item: &DefinitionItem,
    kind: EntryKind,
) -> Vec<Vec<super::RecognizedName>> {
    name_occurrences_with_operands(item, kind, HeadOperands::Absent)
}

fn name_occurrences_with_operands(
    item: &DefinitionItem,
    kind: EntryKind,
    operands: HeadOperands<'_>,
) -> Vec<Vec<super::RecognizedName>> {
    if let EntryKind::Parameter { parameter_kind } = kind {
        if parameter_kind == ParameterKind::Option {
            return options::parameter_occurrences(&item.terms, operands);
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
        .enumerate()
        .map(|(index, term)| {
            let Some(ranges) = operands.for_term(index) else {
                return Vec::new();
            };
            let uses = OperandUses::new(ranges);
            let text = plain_text(term);
            match kind {
                EntryKind::EnvironmentVariable => {
                    named::environment_occurrences_with_uses(&text, uses).unwrap_or_default()
                }
                EntryKind::Variable | EntryKind::ConfigurationKey | EntryKind::Value => {
                    let validate = match kind {
                        EntryKind::Variable => is_variable_term,
                        EntryKind::ConfigurationKey => is_configuration_key,
                        _ => is_value_name,
                    };
                    named::named_occurrences_with_uses(&text, validate, uses).unwrap_or_default()
                }
                EntryKind::Command => {
                    if let Some((name, _)) = super::context::key_binding_command_form(&text) {
                        return commands::command_binding(&text, name, 0, uses)
                            .into_iter()
                            .collect();
                    }
                    let mut offset = 0;
                    forms::declaration_groups_with_uses(term, uses)
                        .into_iter()
                        .filter_map(|group| {
                            let text = plain_text(&group);
                            let start = offset;
                            offset += text.len() + 1;
                            let name = commands::inferred_command_name(&group).or_else(|| {
                                commands::command_name_from_authored_form(&text).map(str::to_owned)
                            })?;
                            commands::command_binding(&text, &name, start, uses)
                        })
                        .collect()
                }
                EntryKind::Term => named::term_occurrences(&text, uses).unwrap_or_default(),
                EntryKind::Parameter { .. } => Vec::new(),
            }
        })
        .collect()
}
