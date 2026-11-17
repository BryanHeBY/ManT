//! Recognize independent names in a native HEAD with protected operands.
use super::{Recognition, offset, operand_at, recognize_part, unwrapped};
use crate::definitions::syntax::{
    DeclarationLimit,
    eligibility::{NameSource, OperandUses},
    named, options,
    scan::{MAX_NAMES, MAX_NESTING},
};
use crate::definitions::{NativeOperand, NativeOperandRole, NativeSubject, RecognizedName};
use mant_ir::{EntryKind, ParameterKind};
use std::ops::Range;

const OPTION: EntryKind = EntryKind::Parameter {
    parameter_kind: ParameterKind::Option,
};

#[derive(Default)]
struct Candidates {
    names: Vec<Candidate>,
    unproved_part: bool,
    separated: bool,
    sequence: SubjectSequence,
}

#[derive(Default, Clone, Copy)]
enum SubjectSequence {
    #[default]
    Subjects,
    Constant(Option<u32>),
    ArgumentTail(Option<u32>),
}

struct Candidate {
    name: RecognizedName,
    kind: EntryKind,
    explicit: bool,
}

enum RejectedPart {
    Unproved,
    NameLimit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Admission {
    TypedSubjects,
    ProtectedSyntax,
}

pub(super) fn recognize(
    text: &str,
    operands: &[NativeOperand],
    admission: Admission,
) -> Option<(EntryKind, Recognition)> {
    if !text.contains([',', '|']) {
        return None;
    }
    let mut candidates = Candidates::default();
    let collected = collect(
        text,
        unwrap_search(text, text, operands),
        operands,
        0,
        admission,
        &mut candidates,
    );
    // A flag or search sign in the arguments of a single invocation does
    // not establish a separately declared name. Its existing grammar still
    // owns the invocation (for example read [-n n | -N n] [vname?prompt]).
    if !candidates.separated {
        return None;
    }
    Some(candidates.resolve(collected))
}

impl Candidates {
    fn leave_enclosure(&mut self, enclosure: u32, parent: Option<u32>) {
        self.sequence = match self.sequence {
            SubjectSequence::Constant(Some(owner)) if owner == enclosure => {
                SubjectSequence::Constant(parent)
            }
            SubjectSequence::ArgumentTail(Some(owner)) if owner == enclosure => {
                SubjectSequence::Subjects
            }
            sequence => sequence,
        };
    }

    fn resolve(self, result: Result<(), RejectedPart>) -> (EntryKind, Recognition) {
        let Err(rejected) = result else {
            let mut kind = None;
            let names = self
                .names
                .into_iter()
                .filter(|candidate| candidate.explicit || !self.unproved_part)
                .map(|candidate| {
                    kind = Some(match kind {
                        Some(previous) if previous != candidate.kind => EntryKind::Term,
                        _ => candidate.kind,
                    });
                    candidate.name
                })
                .collect();
            return (
                kind.unwrap_or(EntryKind::Term),
                Recognition { names, limit: None },
            );
        };
        let limit = match rejected {
            RejectedPart::NameLimit => Some(DeclarationLimit::Names),
            RejectedPart::Unproved => None,
        };
        (
            EntryKind::Term,
            Recognition {
                names: Vec::new(),
                limit,
            },
        )
    }
}

fn collect(
    text: &str,
    (head, enclosure): (&str, Option<u32>),
    operands: &[NativeOperand],
    depth: usize,
    admission: Admission,
    candidates: &mut Candidates,
) -> Result<(), RejectedPart> {
    if depth == MAX_NESTING {
        return Err(RejectedPart::Unproved);
    }
    let start = offset(text, head);
    let uses = OperandUses::new(operands);
    let groups =
        independent_groups(head, start, enclosure, operands).ok_or(RejectedPart::Unproved)?;
    candidates.separated |= groups.len() > 1;
    for group in groups {
        if matches!(candidates.sequence, SubjectSequence::ArgumentTail(_)) {
            break;
        }
        let group = group.trim();
        let (part, nested) = unwrap_search(text, group, operands);
        if part.len() != group.len() {
            let result = collect(
                text,
                (part, nested.or(enclosure)),
                operands,
                depth + 1,
                admission,
                candidates,
            );
            if let Some(nested) = nested {
                candidates.leave_enclosure(nested, enclosure);
            }
            match result {
                Ok(()) => {}
                Err(RejectedPart::Unproved) => candidates.unproved_part = true,
                Err(err) => return Err(err),
            }
            continue;
        }
        let start = offset(text, part);
        let span = start..start + part.len();
        if uses.covers(&span, search_argument) {
            continue;
        }
        let lead = operand_at(operands, start);
        let subject = lead
            .and_then(|operand| operand.role.subject())
            .map(|(subject, _)| subject);
        if subject == Some(NativeSubject::Field) {
            // A separately delimited Fa starts the argument tail of the
            // preceding constant. Matching native enclosures are transparent
            // to the role, but a tail started inside one ends with that scope.
            if let SubjectSequence::Constant(owner) = candidates.sequence {
                candidates.sequence = SubjectSequence::ArgumentTail(owner);
                candidates.unproved_part = true;
            }
            continue;
        }
        let explicit = lead.is_some_and(|operand| {
            operand.role == NativeOperandRole::ExplicitOption
                || operand.role == NativeOperandRole::Placeholder
                || operand.role.subject().is_some()
        });
        let Ok((kind, result)) = recognize_group(text, part, span, operands, uses, admission)
        else {
            candidates.unproved_part = true;
            continue;
        };
        if result.limit.is_some() || candidates.names.len() + result.names.len() > MAX_NAMES {
            return Err(RejectedPart::NameLimit);
        }
        if result.names.is_empty() {
            candidates.unproved_part = true;
        } else if subject == Some(NativeSubject::Constant) {
            candidates.sequence = SubjectSequence::Constant(enclosure);
        } else if explicit {
            candidates.sequence = SubjectSequence::Subjects;
        }
        candidates
            .names
            .extend(result.names.into_iter().map(|name| Candidate {
                name,
                kind,
                explicit,
            }));
    }
    Ok(())
}

fn independent_groups<'a>(
    head: &'a str,
    start: usize,
    enclosure: Option<u32>,
    operands: &[NativeOperand],
) -> Option<Vec<&'a str>> {
    // Protect accepted words in a nested enclosure even if its opening or
    // closing glyph was overwritten. Only the unwrapped BODY/ELEM supplies
    // separators at this level.
    let first = operands.partition_point(|operand| operand.bytes.end <= start);
    let mut receipts = operands[first..].iter().peekable();
    let mut last = None;
    named::split_named_groups(head, |index| {
        let offset = start + index;
        while let Some(operand) = receipts.next_if(|operand| operand.bytes.start <= offset) {
            last = Some(operand);
        }
        last.is_some_and(|operand| {
            operand.bytes.contains(&offset)
                && (operand.role.is_parameter()
                    || match operand.role {
                        NativeOperandRole::ExplicitOption
                        | NativeOperandRole::EnclosureStart(_)
                        | NativeOperandRole::EnclosureEnd(_) => true,
                        NativeOperandRole::EnclosureContent(scope) => enclosure != Some(scope),
                        _ => false,
                    })
        })
    })
}

fn recognize_group(
    text: &str,
    part: &str,
    span: Range<usize>,
    operands: &[NativeOperand],
    uses: OperandUses<'_>,
    admission: Admission,
) -> Result<(EntryKind, Recognition), RejectedPart> {
    if let Some(option) = operand_at(operands, span.start)
        && option.role == NativeOperandRole::ExplicitOption
    {
        return option_group(text, span, option, operands, uses);
    }
    if operands[operands.partition_point(|operand| operand.bytes.end <= span.start)..]
        .iter()
        .take_while(|operand| operand.bytes.start < span.end)
        .any(|operand| operand.role == NativeOperandRole::ExplicitOption)
    {
        // Joined text from another operand is neither a complete option
        // nor a new lexical spelling containing both operands.
        return Err(RejectedPart::Unproved);
    }
    if admission == Admission::TypedSubjects
        && operand_at(operands, span.start).is_none_or(|operand| operand.role.subject().is_none())
    {
        return Err(RejectedPart::Unproved);
    }
    if let Some(result) = recognize_part(text, part, operands) {
        return Ok(result);
    }
    if admission == Admission::TypedSubjects {
        return Err(RejectedPart::Unproved);
    }
    let uses = uses.shifted(span.start);
    let names = named::term_occurrences(part, uses)
        .ok_or(RejectedPart::Unproved)?
        .into_iter()
        .filter(|name| uses.permits(name, NameSource::Lexical))
        .map(|name| rebase(name, span.start))
        .collect();
    Ok((EntryKind::Term, Recognition { names, limit: None }))
}

fn option_group(
    text: &str,
    span: Range<usize>,
    option: &NativeOperand,
    operands: &[NativeOperand],
    uses: OperandUses<'_>,
) -> Result<(EntryKind, Recognition), RejectedPart> {
    if !text[option.bytes.start..span.start].trim().is_empty() {
        return Err(RejectedPart::Unproved);
    }
    let end = option.bytes.end.min(span.end);
    if !text[end..option.bytes.end].trim().is_empty() {
        return Err(RejectedPart::Unproved);
    }
    let (suffix, _) = unwrapped(text, &text[end..span.end], operands);
    let suffix_start = offset(text, suffix);
    if !suffix.is_empty()
        && !uses.covers(&(suffix_start..suffix_start + suffix.len()), |role| {
            role.is_parameter() || role.is_search()
        })
    {
        return Err(RejectedPart::Unproved);
    }
    let name =
        options::native_option_key(&text[span.start..end]).map(|name| rebase(name, span.start));
    Ok((OPTION, Recognition::one(name)))
}

fn unwrap_search<'a>(
    text: &'a str,
    mut head: &'a str,
    operands: &[NativeOperand],
) -> (&'a str, Option<u32>) {
    let mut enclosure = None;
    loop {
        let (inner, scope) = unwrapped(text, head, operands);
        head = inner;
        enclosure = scope.or(enclosure);
        let Some(rest) = head.strip_prefix(['/', '?']) else {
            return (head, enclosure);
        };
        let start = offset(text, head);
        if !operand_at(operands, start).is_some_and(|operand| {
            matches!(
                operand.role,
                NativeOperandRole::SearchSign | NativeOperandRole::SearchOpen { .. }
            )
        }) {
            return (head, enclosure);
        }
        head = rest;
    }
}

fn search_argument(role: NativeOperandRole) -> bool {
    role.is_search()
        || matches!(
            role,
            NativeOperandRole::Argument | NativeOperandRole::IncompletePlaceholder
        )
}

fn rebase(mut name: RecognizedName, start: usize) -> RecognizedName {
    for part in &mut name.parts {
        part.start += start;
        part.end += start;
    }
    name
}
