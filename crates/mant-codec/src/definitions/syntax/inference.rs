//! Select an owner category after collecting names for the applicable grammars.
#[cfg(feature = "roff")]
use super::native_subject;
use super::{
    DeclarationLimit, decision, forms,
    occurrences::{
        HeadNames, constrain_term_names, lexical_term_occurrences, name_occurrences_with_operands,
        recognize_head_names,
    },
    options, scan,
};
use crate::definitions::{
    NativeHeadRole, RecognizedName, context::DefinitionContext, evidence::HeadOperands,
};
use mant_ir::{
    DefinitionItem, EntryKind, NameCase, ParameterKind, inline_plain_text as plain_text,
};

pub(in crate::definitions) struct InferredIdentity {
    pub(in crate::definitions) kind: EntryKind,
    pub(in crate::definitions) case: NameCase,
    pub(in crate::definitions) names: Vec<String>,
    pub(in crate::definitions) occurrences: Vec<Vec<RecognizedName>>,
    pub(in crate::definitions) excluded_forms: Vec<usize>,
    pub(in crate::definitions) limit: Option<DeclarationLimit>,
    pub(in crate::definitions) semantic: bool,
}

pub(in crate::definitions) fn infer_identity(
    item: &DefinitionItem,
    context: DefinitionContext,
    hint: Option<NativeHeadRole>,
    operands: HeadOperands<'_>,
) -> InferredIdentity {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    #[cfg(feature = "roff")]
    if let Some((kind, recognized)) = native_subject::recognize_head(item, &first, operands) {
        return InferredIdentity::classify(recognized, kind, NameCase::Sensitive, false);
    }
    let trimmed = first.trim();
    let leading_option = hint.is_none()
        && operands
            .for_term(0)
            .is_some_and(|ranges| scan::has_leading_explicit_option(&first, ranges));
    let hint = hint.or(leading_option.then_some(NativeHeadRole::Option));
    let (mut kind, mut case) = decision::select_kind(trimmed, context, hint);
    let keep_unnamed_kind = matches!(
        hint,
        Some(NativeHeadRole::Option | NativeHeadRole::Environment)
    );
    let mut recognized = recognize_head_names(item, kind, hint, operands);
    recognized.constrain(operands);
    if hint == Some(NativeHeadRole::Literal) && recognized.is_empty() {
        recognized.occurrences = name_occurrences_with_operands(item, EntryKind::Command, operands);
        recognized.constrain(operands);
        if recognized.is_empty() {
            recognized.occurrences = item
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
            recognized.constrain(operands);
        }
        kind = EntryKind::Term;
        case = NameCase::Sensitive;
    }
    if recognized.is_empty() && !keep_unnamed_kind {
        recognized.occurrences = name_occurrences_with_operands(item, EntryKind::Term, operands);
        recognized.constrain(operands);
        kind = EntryKind::Term;
        case = NameCase::Sensitive;
    }
    if (hint.is_none() || leading_option)
        && item.terms.len() > 1
        && operands.proves_option()
        && let Some(mixed_kind) =
            continued_occurrences(item, &first, operands, leading_option, &mut recognized)
    {
        kind = mixed_kind;
        case = NameCase::Sensitive;
    }
    InferredIdentity::classify(recognized, kind, case, keep_unnamed_kind)
}

impl InferredIdentity {
    fn classify(
        recognized: HeadNames,
        kind: EntryKind,
        case: NameCase,
        keep_unnamed_kind: bool,
    ) -> Self {
        let names = ordered_names(&recognized.occurrences, kind);
        let (kind, case) = if names.is_empty() && !keep_unnamed_kind {
            (EntryKind::Term, NameCase::Sensitive)
        } else {
            (kind, case)
        };
        Self {
            kind,
            case,
            names,
            occurrences: recognized.occurrences,
            excluded_forms: recognized.excluded_forms,
            limit: recognized.limit,
            semantic: recognized.semantic,
        }
    }
}

fn continued_occurrences(
    item: &DefinitionItem,
    first: &str,
    operands: HeadOperands<'_>,
    leading_option: bool,
    recognized: &mut HeadNames,
) -> Option<EntryKind> {
    let HeadNames {
        occurrences, limit, ..
    } = recognized;
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
