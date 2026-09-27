//! Source-neutral grammars for semantic definition names and lexical evidence.
//! Context selects a grammar; one recognition result carries both selectable
//! spellings and their original ranges to the binding mapper.
#![allow(clippy::similar_names)] // ContentContext and DefinitionContext are distinct inputs.
use super::{RecognizedName, context::DefinitionContext};
use mant_ir::inline_plain_text as plain_text;
use mant_ir::{ContentContext, DefinitionItem, EntryKind, Inline, NameCase, ParameterKind};

mod commands;
mod decision;
mod declaration;
mod forms;
mod head;
mod named;
mod native_components;
mod options;
pub(super) use head::is_inferred_head;
pub(crate) use mant_ir::option_prefix;
pub(super) use named::is_value_name;
pub(crate) use named::{environment_variable_alias, environment_variable_body};
use named::{is_configuration_key, is_variable_term};
#[cfg(test)]
pub(super) use options::option_names;
#[cfg(test)]
pub(crate) use options::option_names_from_terms;
pub(crate) use options::{
    option_names_from_literal, option_occurrences_from_literal, slash_option_forms,
};

pub(super) struct InferredIdentity {
    pub(super) kind: EntryKind,
    pub(super) case: NameCase,
    pub(super) names: Vec<String>,
    pub(super) occurrences: Vec<Vec<RecognizedName>>,
    /// The complete owner exceeded the bounded declaration-occurrence budget.
    pub(super) over_limit: bool,
}

/// Recheck a final Flow owner for a budget omission after native role hints
/// have been discarded. Either complete visible or native-prefix grammar may
/// prove the same over-limit owner; neither publishes a partial name group.
#[cfg_attr(not(feature = "roff"), allow(dead_code))]
pub(super) fn environment_owner_over_limit(
    content: ContentContext<'_>,
    item: &DefinitionItem,
) -> bool {
    named::environment_owner_occurrences(content, &item.terms, false).is_err()
        || named::environment_owner_occurrences(content, &item.terms, true).is_err()
}

#[expect(
    clippy::too_many_lines,
    reason = "keep declaration role precedence and its one shared recognition result together"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "the frozen native HEAD witness is passed intact"
)]
pub(super) fn infer_identity(
    content: ContentContext<'_>,
    item: &DefinitionItem,
    context: DefinitionContext,
    hint: Option<super::NativeHeadRole>,
    complete_term_witness: bool,
    option_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
    operand_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
    argument_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
    components: Option<&[Vec<super::NativeHeadComponent>]>,
) -> InferredIdentity {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(content, term));
    let trimmed = first.trim();
    let leading = first.len() - first.trim_start().len();
    let numeric_start = first
        .get(leading..leading.saturating_add(2))
        .filter(|token| {
            let bytes = token.as_bytes();
            bytes.len() == 2 && bytes[0] == b'-' && bytes[1].is_ascii_digit()
        });
    // Numeric spelling is not a source-neutral option rule: only the shared
    // scan, supplied with the native operand and final-style evidence, may
    // prove a leading short digit. Reuse that result below instead of making
    // a second role-specific name decision after selecting EntryKind.
    let native_numeric_occurrences = (hint.is_none() && numeric_start.is_some())
        .then(|| options::parameter_occurrences(content, &item.terms, operand_ranges));
    let native_numeric_occurrences = native_numeric_occurrences.filter(|occurrences| {
        let Some(spelling) = numeric_start else {
            return false;
        };
        occurrences.first().is_some_and(|names| {
            names.iter().any(|name| {
                name.name == spelling
                    && name.parts.len() == 1
                    && name.parts[0] == (leading..leading + 2)
            })
        })
    });
    let configuration_scan = matches!(
        context,
        DefinitionContext::ConfigurationKeys | DefinitionContext::RootConfigurationKeys
    )
    .then(|| {
        item.terms
            .iter()
            .enumerate()
            .map(|(index, term)| {
                let text = plain_text(content, term);
                if context == DefinitionContext::RootConfigurationKeys && hint.is_none() {
                    mant_ir::root_configuration_assignment_range(&text).and_then(|range| {
                        text.get(range.clone())
                            .map(|name| vec![RecognizedName::contiguous(name, range.start)])
                    })
                } else {
                    native_components::configuration_names(
                        &text,
                        components.and_then(|ranges| ranges.get(index).map(Vec::as_slice)),
                        argument_ranges.and_then(|ranges| ranges.get(index).map(Vec::as_slice)),
                    )
                }
            })
            .collect::<Vec<_>>()
    });
    let first_configuration_name = configuration_scan
        .as_ref()
        .and_then(|terms| terms.first())
        .is_some_and(|names| names.as_ref().is_some_and(|names| !names.is_empty()));
    let (mut kind, mut case) = decision::select_kind(
        trimmed,
        context,
        hint,
        native_numeric_occurrences.is_some(),
        first_configuration_name,
    );
    let mut proven_command_occurrences = None;
    if kind == EntryKind::Command
        && context == DefinitionContext::Commands
        && hint.is_none()
        && complete_term_witness
    {
        let candidates = name_occurrences(content, item, EntryKind::Command, operand_ranges);
        let complete = item.terms.iter().enumerate().all(|(index, term)| {
            let visible = plain_text(content, term);
            mant_ir::command_declaration_name_range(&visible).is_some()
                || head::is_styled_command_head(content, term)
                || components
                    .and_then(|terms| terms.get(index))
                    .is_some_and(|members| {
                        members
                            .iter()
                            .any(|member| member.role == super::NativeHeadRole::Literal)
                            && candidates.get(index).is_some_and(|names| !names.is_empty())
                    })
                || forms::declaration_groups(content, term).len() > 1
                    && candidates.get(index).is_some_and(|names| names.len() > 1)
        });
        // CVS man_macro.c::blk_imp/man_term.c::pre_TP establish a physical
        // owner and man_term.c::pre_B only changes font. Neither turns a bare
        // bold subject into a command. A complete call or a bounded group of
        // separately recognized declaration forms can do so; otherwise keep
        // the TP label as a named Term. Authored Ic/Cm and explicit IR facts
        // take other paths.
        // A failed Command role does not erase a checked styled name prefix
        // such as `start` in `start option|other`; it remains a named Term.
        proven_command_occurrences = Some(candidates);
        if !complete {
            kind = EntryKind::Term;
            case = NameCase::Sensitive;
        }
    }
    let mut over_limit = matches!(
        hint,
        Some(super::NativeHeadRole::Literal | super::NativeHeadRole::Variable)
    ) && (components
        .is_some_and(|terms| terms.iter().any(|members| members.len() > 64))
        || argument_ranges.is_some_and(|terms| terms.iter().any(|members| members.len() > 64)));
    let mut occurrences = if hint == Some(super::NativeHeadRole::LiteralTerm) {
        item.terms
            .iter()
            .map(|term| {
                let text = plain_text(content, term);
                vec![super::RecognizedName::contiguous(
                    text.trim(),
                    text.len() - text.trim_start().len(),
                )]
            })
            .collect()
    } else if hint == Some(super::NativeHeadRole::Option) {
        options::native_option_occurrences(content, &item.terms, option_ranges, operand_ranges)
    } else if hint == Some(super::NativeHeadRole::Environment) {
        if let Ok(occurrences) = named::environment_owner_occurrences(content, &item.terms, true) {
            occurrences
        } else {
            over_limit = true;
            Vec::new()
        }
    } else if let Some(occurrences) = native_numeric_occurrences {
        occurrences
    } else if let Some(occurrences) = proven_command_occurrences {
        occurrences
    } else if kind == EntryKind::ConfigurationKey {
        configuration_scan
            .unwrap_or_default()
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect()
    } else if hint == Some(super::NativeHeadRole::Variable) && components.is_some() {
        item.terms
            .iter()
            .enumerate()
            .map(|(index, term)| {
                let text = plain_text(content, term);
                components
                    .and_then(|ranges| ranges.get(index))
                    .and_then(|members| native_components::variable_names(&text, members))
                    .unwrap_or_default()
            })
            .collect()
    } else if context == DefinitionContext::Variables && hint.is_none() && !complete_term_witness {
        item.terms
            .iter()
            .map(|term| {
                let text = plain_text(content, term);
                mant_ir::variable_assignment_declaration_range(&text)
                    .and_then(|range| {
                        text.get(range.clone())
                            .map(|name| vec![RecognizedName::contiguous(name, range.start)])
                    })
                    .unwrap_or_default()
            })
            .collect()
    } else if kind == EntryKind::Variable && hint.is_none() {
        item.terms
            .iter()
            .map(|term| {
                let text = plain_text(content, term);
                mant_ir::variable_declaration_name_range(&text)
                    .and_then(|range| {
                        text.get(range.clone())
                            .map(|name| vec![RecognizedName::contiguous(name, range.start)])
                    })
                    .unwrap_or_default()
            })
            .collect()
    } else if kind == EntryKind::EnvironmentVariable {
        if let Ok(occurrences) = named::environment_owner_occurrences(content, &item.terms, false) {
            occurrences
        } else {
            over_limit = true;
            Vec::new()
        }
    } else {
        name_occurrences(content, item, kind, operand_ranges)
    };
    if kind == EntryKind::Term
        && context == DefinitionContext::Values
        && hint.is_none()
        && occurrences.iter().all(Vec::is_empty)
        && item.terms.len() <= 64
    {
        // CVS man_macro.c::blk_imp/man_term.c::pre_IP retain an isolated
        // unsigned numeric HEAD as its own definition below an option. It is
        // an exact Term selector, not proof of a Value kind or a valueDomain.
        // Numbered sequences are converted to List before this pass.
        occurrences = item
            .terms
            .iter()
            .map(|term| {
                let text = plain_text(content, term);
                let name = text.trim();
                if name.len() <= 128
                    && !name.is_empty()
                    && name.bytes().all(|byte| byte.is_ascii_digit())
                {
                    vec![RecognizedName::contiguous(
                        name,
                        text.len() - text.trim_start().len(),
                    )]
                } else {
                    Vec::new()
                }
            })
            .collect();
    }
    if hint == Some(super::NativeHeadRole::Literal) && occurrences.iter().all(Vec::is_empty) {
        occurrences = name_occurrences(content, item, EntryKind::Command, operand_ranges);
        if occurrences.iter().all(Vec::is_empty) {
            occurrences = item
                .terms
                .iter()
                .map(|term| {
                    let prefix = forms::literal_prefix(content, term);
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
    // An unrecognized variable label is still a real native TP/It owner, but
    // the full-label Term fallback cannot turn a malformed variable syntax
    // into a selectable name. A plain label like `real-name` can still be a
    // conservative named Term when the native role is not proven;
    // an explicit Va component takes the native-role path above instead.
    let rejected_variable_head = context == DefinitionContext::Variables
        && hint.is_none()
        && kind == EntryKind::Variable
        && occurrences.iter().all(Vec::is_empty)
        && item.terms.iter().any(|term| {
            let text = plain_text(content, term);
            mant_ir::rejected_variable_declaration_head(&text)
        });
    if !over_limit
        && !rejected_variable_head
        && occurrences.iter().all(Vec::is_empty)
        && kind != EntryKind::EnvironmentVariable
        && !matches!(
            hint,
            Some(super::NativeHeadRole::Option | super::NativeHeadRole::Environment)
        )
    {
        occurrences = name_occurrences(content, item, EntryKind::Term, operand_ranges);
        kind = EntryKind::Term;
        case = NameCase::Sensitive;
    }
    if complete_term_witness && kind == EntryKind::Term && !rejected_variable_head {
        // man_term.c::pre_TP and mdoc_term.c::termp_it_pre establish the
        // physical owner independently. Only then can punctuation-bearing
        // full labels fall back to one Term rather than delimiter-made aliases.
        for (term, names) in item.terms.iter().zip(&mut occurrences) {
            if names.is_empty() {
                *names = complete_native_term_occurrences(content, term);
            }
        }
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
        && !over_limit
        && kind != EntryKind::EnvironmentVariable
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
        over_limit,
    }
}

/// The same role-specific grammars produce both names and their lexical
/// evidence. Binding only maps these ranges to styled IR leaves; it does not
/// have another definition of punctuation or argument boundaries.
pub(super) fn name_occurrences(
    content: ContentContext<'_>,
    item: &DefinitionItem,
    kind: EntryKind,
    operand_ranges: Option<&[Vec<std::ops::Range<usize>>]>,
) -> Vec<Vec<super::RecognizedName>> {
    if let EntryKind::Parameter { parameter_kind } = kind {
        if parameter_kind == ParameterKind::Option {
            return options::parameter_occurrences(content, &item.terms, operand_ranges);
        }
        // Marker/operand inference requires an exact complete first term,
        // not a prefix of a longer invocation. Repeated matching terms may
        // contribute evidence without promoting other terms into names.
        let first = item
            .terms
            .first()
            .map_or_else(String::new, |term| plain_text(content, term));
        return item
            .terms
            .iter()
            .map(|term| {
                let text = plain_text(content, term);
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
            let text = plain_text(content, term);
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
                    if let Some(range) = mant_ir::command_declaration_name_range(&text) {
                        return vec![super::RecognizedName::contiguous(
                            &text[range.clone()],
                            range.start,
                        )];
                    }
                    let mut offset = 0;
                    forms::declaration_groups(content, term)
                        .into_iter()
                        .filter_map(|group| {
                            let text = plain_text(content, &group);
                            let start = offset;
                            offset += text.len() + 1;
                            let name = commands::leading_styled_command_name(content, &group)
                                .or_else(|| {
                                    commands::command_name_from_authored_form(&text)
                                        .map(str::to_owned)
                                })?;
                            let start =
                                start + text.find(&name).expect("grammar returns the command head");
                            Some(super::RecognizedName::contiguous(&name, start))
                        })
                        .collect()
                }
                EntryKind::Term => named::term_occurrences(&text)
                    .or_else(|| styled_complete_term_occurrences(content, term, &text))
                    .unwrap_or_default(),
                EntryKind::Parameter { .. } => Vec::new(),
            }
        })
        .collect()
}

pub(super) fn styled_complete_term_occurrences(
    content: ContentContext<'_>,
    term: &[Inline],
    text: &str,
) -> Option<Vec<RecognizedName>> {
    // A real definition owner plus one complete authored bold label can bind
    // a multiword subject. Unstyled prose has no equivalent evidence.
    let styled = term
        .iter()
        .any(|inline| matches!(inline, Inline::Strong { .. }))
        && term.iter().all(|inline| {
            matches!(inline, Inline::Strong { .. })
                || plain_text(content, std::slice::from_ref(inline))
                    .trim()
                    .is_empty()
        });
    if !styled {
        return None;
    }
    let range = mant_ir::complete_term_label_range(text)?;
    Some(vec![RecognizedName::contiguous(
        &text[range.clone()],
        range.start,
    )])
}

fn complete_native_term_occurrences(
    content: ContentContext<'_>,
    term: &[Inline],
) -> Vec<RecognizedName> {
    let text = plain_text(content, term);
    let Some(range) = mant_ir::complete_term_label_range(&text) else {
        return Vec::new();
    };
    // A failed lexical option is not renamed as a generic Term. The native
    // owner witness proves a definition boundary, not an option spelling.
    if option_prefix(&text[range.clone()]).is_some() {
        return Vec::new();
    }
    vec![RecognizedName::contiguous(
        &text[range.clone()],
        range.start,
    )]
}
