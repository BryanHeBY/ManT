//! Name grammars and accepted source ranges, independent of owner fallback policy.
use super::{
    commands,
    eligibility::{NameSource, OperandUses},
    forms, named, options,
    scan::DeclarationLimit,
};
use crate::definitions::{
    NativeHeadRole, NativeOperand, RecognizedName, context, evidence::HeadOperands,
};
use mant_ir::{DefinitionItem, EntryKind, Inline, ParameterKind, inline_plain_text as plain_text};

pub(super) struct HeadNames {
    pub(super) occurrences: Vec<Vec<RecognizedName>>,
    pub(super) limit: Option<DeclarationLimit>,
}

impl HeadNames {
    fn from_occurrences(occurrences: Vec<Vec<RecognizedName>>) -> Self {
        Self {
            occurrences,
            limit: None,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.occurrences.iter().all(Vec::is_empty)
    }

    pub(super) fn constrain(&mut self, operands: HeadOperands<'_>) {
        for (index, names) in self.occurrences.iter_mut().enumerate() {
            constrain_term_names(names, operands.for_term(index));
        }
    }
}

pub(super) fn recognize_head_names(
    item: &DefinitionItem,
    kind: EntryKind,
    hint: Option<NativeHeadRole>,
    operands: HeadOperands<'_>,
) -> HeadNames {
    if hint == Some(NativeHeadRole::LiteralTerm) {
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
        return HeadNames::from_occurrences(occurrences);
    }
    let occurrences = match hint {
        Some(NativeHeadRole::Option) => {
            let found = options::native_option_occurrences(&item.terms, operands);
            // A complete accepted native option receipt establishes known
            // selector loss; a weak candidate alone cannot prove coverage loss.
            let limit = if operands.proves_option() {
                found.limit
            } else {
                None
            };
            return HeadNames {
                occurrences: found.occurrences,
                limit,
            };
        }
        Some(NativeHeadRole::Environment) => item
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
    HeadNames::from_occurrences(occurrences)
}

fn native_environment_occurrences(term: &[Inline], uses: OperandUses<'_>) -> Vec<RecognizedName> {
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

pub(super) fn lexical_term_occurrences(
    text: &str,
    ranges: Option<&[NativeOperand]>,
) -> Vec<RecognizedName> {
    ranges.map_or_else(Vec::new, |ranges| {
        named::term_occurrences(text, OperandUses::new(ranges)).unwrap_or_default()
    })
}

pub(super) fn constrain_term_names(
    names: &mut Vec<RecognizedName>,
    operands: Option<&[NativeOperand]>,
) {
    let Some(ranges) = operands else {
        names.clear();
        return;
    };
    names.retain(|name| OperandUses::new(ranges).permits(name, NameSource::Lexical));
}

/// The same role-specific grammars produce both names and their lexical
/// evidence. Binding only maps these ranges to styled IR leaves; it does not
/// have another definition of punctuation or argument boundaries.
pub(in crate::definitions) fn name_occurrences(
    item: &DefinitionItem,
    kind: EntryKind,
) -> Vec<Vec<RecognizedName>> {
    name_occurrences_with_operands(item, kind, HeadOperands::Absent)
}

pub(super) fn name_occurrences_with_operands(
    item: &DefinitionItem,
    kind: EntryKind,
    operands: HeadOperands<'_>,
) -> Vec<Vec<RecognizedName>> {
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
                        EntryKind::Variable => named::is_variable_term,
                        EntryKind::ConfigurationKey => named::is_configuration_key,
                        _ => named::is_value_name,
                    };
                    named::named_occurrences_with_uses(&text, validate, uses).unwrap_or_default()
                }
                EntryKind::Command => {
                    if let Some((name, _)) = context::key_binding_command_form(&text) {
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
