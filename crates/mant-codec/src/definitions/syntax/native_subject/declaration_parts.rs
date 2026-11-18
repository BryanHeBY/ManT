//! Recognize independent names in a native HEAD with protected operands.
use super::{
    Recognition, annotations, environment, following_function, offset, operand_at, operand_cursor,
    recognize_part, unwrapped,
};
use crate::definitions::syntax::{
    DeclarationLimit, commands,
    eligibility::{NameSource, OperandUses, intersecting_operands},
    named, options,
    scan::{HeadView, MAX_NAMES, MAX_NESTING},
};
use crate::definitions::{NativeOperand, NativeOperandRole, NativeSubject, RecognizedName};
use mant_ir::{EntryKind, Inline, ParameterKind};
use std::{cell::OnceCell, ops::Range};

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
    Function(Option<u32>),
    ArgumentTail(Option<u32>),
}

impl SubjectSequence {
    fn from_subject(subject: NativeSubject, enclosure: Option<u32>) -> Self {
        match subject {
            NativeSubject::Constant => Self::Constant(enclosure),
            // Fo's generated arguments already carry the Argument role;
            // a following Fa outside its BODY can be independent.
            NativeSubject::Function => Self::Function(enclosure),
            _ => Self::Subjects,
        }
    }
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
    AfterSeparator,
}

struct Head<'a> {
    nodes: &'a [Inline],
    text: &'a str,
    operands: &'a [NativeOperand],
    view: OnceCell<HeadView<'a>>,
}

impl<'a> Head<'a> {
    fn view(&self) -> &HeadView<'a> {
        self.view
            .get_or_init(|| HeadView::new(self.nodes, self.operands))
    }
}

pub(super) fn recognize(
    nodes: &[Inline],
    text: &str,
    operands: &[NativeOperand],
    admission: Admission,
) -> Option<(EntryKind, Recognition)> {
    if !text.contains([',', '|']) {
        return None;
    }
    let source = Head {
        nodes,
        text,
        operands,
        view: OnceCell::new(),
    };
    let mut candidates = Candidates::default();
    let collected = collect(
        &source,
        unwrap_search(text, text, operands),
        0,
        admission,
        &mut candidates,
    );
    // A flag or search sign in the arguments of a single invocation does
    // not establish a separately declared name. Its existing grammar still
    // owns the invocation (for example read [-n n | -N n] [vname?prompt]).
    if !candidates.separated
        || admission == Admission::AfterSeparator
            && !candidates.names.iter().any(|name| name.explicit)
    {
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
            SubjectSequence::Function(Some(owner)) | SubjectSequence::ArgumentTail(Some(owner))
                if owner == enclosure =>
            {
                SubjectSequence::Subjects
            }
            sequence => sequence,
        };
    }

    fn admit_subject(&mut self, subject: Option<NativeSubject>, enclosure: Option<u32>) -> bool {
        let Some(subject) = subject else { return true };
        // An independently established subject changes the sequence even
        // when its name is unavailable. CVS termp_fa_pre distinguishes
        // ordinary Fa from its use as a function argument.
        if subject != NativeSubject::Field {
            self.sequence = SubjectSequence::from_subject(subject, enclosure);
            return true;
        }
        match self.sequence {
            SubjectSequence::Subjects => true,
            SubjectSequence::Constant(owner) => {
                self.sequence = SubjectSequence::ArgumentTail(owner);
                self.unproved_part = true;
                false
            }
            SubjectSequence::Function(_) | SubjectSequence::ArgumentTail(_) => false,
        }
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
                Recognition {
                    names,
                    ..Recognition::default()
                },
            );
        };
        let limit = match rejected {
            RejectedPart::NameLimit => Some(DeclarationLimit::Names),
            RejectedPart::Unproved => None,
        };
        (
            EntryKind::Term,
            Recognition {
                limit,
                ..Recognition::default()
            },
        )
    }
}

fn collect(
    source: &Head<'_>,
    (head, enclosure): (&str, Option<u32>),
    depth: usize,
    admission: Admission,
    candidates: &mut Candidates,
) -> Result<(), RejectedPart> {
    if depth == MAX_NESTING {
        return Err(RejectedPart::Unproved);
    }
    let (text, operands) = (source.text, source.operands);
    let start = offset(text, head);
    let uses = OperandUses::new(operands);
    let groups = independent_groups(head, start, enclosure, operands, admission)
        .ok_or(RejectedPart::Unproved)?;
    candidates.separated |= groups.len() > 1;
    for group in groups {
        if matches!(candidates.sequence, SubjectSequence::ArgumentTail(_)) {
            break;
        }
        let group = group.trim();
        let (part, nested) = unwrap_search(text, group, operands);
        if part.len() != group.len() {
            let result = collect(
                source,
                (part, nested.or(enclosure)),
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
        let Ok(subject) = group_subject(text, &span, operands, enclosure) else {
            candidates.unproved_part = true;
            continue;
        };
        if !candidates.admit_subject(subject, enclosure) {
            continue;
        }
        let lead = operand_at(operands, start);
        let operand_label = lead.is_some_and(|operand| {
            operand.role == NativeOperandRole::ExplicitOption
                || operand.role == NativeOperandRole::Placeholder
        });
        let explicit = lead.is_some_and(|operand| {
            admission != Admission::ProtectedSyntax && independent_role(operand.role)
                || operand.role.subject().is_some()
        }) || operand_label;
        let Ok((kind, result)) = recognize_group(source, span, admission, enclosure) else {
            candidates.unproved_part = true;
            continue;
        };
        if result.limit.is_some() || candidates.names.len() + result.names.len() > MAX_NAMES {
            return Err(RejectedPart::NameLimit);
        }
        if result.names.is_empty() {
            candidates.unproved_part = true;
        } else if subject.is_none() && explicit {
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

fn group_subject(
    text: &str,
    span: &Range<usize>,
    operands: &[NativeOperand],
    enclosure: Option<u32>,
) -> Result<Option<NativeSubject>, RejectedPart> {
    let first = operand_cursor(operands, span.start);
    let lead = operands
        .get(first)
        .filter(|operand| operand.bytes.contains(&span.start));
    let Some(lead) = lead else { return Ok(None) };
    let Some(receipt) = lead.role.subject() else {
        // Authored parentheses do not introduce an AST BODY like Pq.
        // Their complete range can still prove a group of arguments.
        return Ok(
            (matches!(lead.role, NativeOperandRole::LiteralDelimiter { .. })
                && annotations::argument_group(text, span, operands, enclosure))
            .then_some(NativeSubject::Field),
        );
    };
    // A group can consist of multiple Fa nodes. Each must come from the
    // same proven enclosure, including the case of a direct It HEAD. A
    // missing delimiter can expose the text from a different scope.
    if receipt.kind == NativeSubject::Field {
        return if annotations::argument_group(text, span, operands, enclosure) {
            Ok(Some(receipt.kind))
        } else {
            Err(RejectedPart::Unproved)
        };
    }
    if receipt.enclosure != enclosure {
        return Ok(None);
    }
    if receipt.kind == NativeSubject::ReturnType
        && let Some((_, function)) = following_function(text, &operands[first..], span.clone())
    {
        return Ok(Some(function));
    }
    Ok(Some(receipt.kind))
}

fn independent_groups<'a>(
    head: &'a str,
    start: usize,
    enclosure: Option<u32>,
    operands: &[NativeOperand],
    admission: Admission,
) -> Option<Vec<&'a str>> {
    // Protect accepted words in a nested enclosure even if its opening or
    // closing glyph was overwritten. Only the unwrapped BODY/ELEM supplies
    // separators at this level.
    let first = operand_cursor(operands, start);
    let mut receipts = operands[first..].iter().peekable();
    let mut last = None;
    named::split_named_groups(head, |index, _| {
        let offset = start + index;
        while let Some(operand) = receipts.next_if(|operand| operand.bytes.start <= offset) {
            last = Some(operand);
        }
        let current = last.filter(|operand| operand.bytes.contains(&offset));
        if matches!(head.as_bytes()[index], b',' | b'|') && admission != Admission::ProtectedSyntax
        {
            // In a weak prefix, punctuation embedded in a prose operand
            // cannot authorize a later native subject as its own declaration.
            return current.is_none_or(|operand| {
                !(operand.role == NativeOperandRole::LiteralContinuation(enclosure)
                    || enclosure.is_some_and(|body| {
                        operand.role == NativeOperandRole::EnclosureContent(body)
                    }))
                    || operand.bytes.start < start
                    || head
                        .get(operand.bytes.start - start..operand.bytes.end - start)
                        .is_none_or(|value| !matches!(value.trim(), "," | "|"))
            });
        }
        current.is_some_and(|operand| {
            operand.role.is_parameter()
                || match operand.role {
                    NativeOperandRole::ExplicitOption
                    | NativeOperandRole::Description
                    | NativeOperandRole::Command
                    | NativeOperandRole::LiteralPunctuation
                    | NativeOperandRole::ReferenceText(_)
                    | NativeOperandRole::EnclosureStart { .. }
                    | NativeOperandRole::EnclosureEnd(_) => true,
                    NativeOperandRole::EnclosureContent(scope) => enclosure != Some(scope),
                    NativeOperandRole::LiteralDelimiter {
                        enclosure: owner, ..
                    } => enclosure != owner,
                    _ => false,
                }
        })
    })
}

fn recognize_group(
    source: &Head<'_>,
    span: Range<usize>,
    admission: Admission,
    enclosure: Option<u32>,
) -> Result<(EntryKind, Recognition), RejectedPart> {
    let (text, operands) = (source.text, source.operands);
    let part = &text[span.clone()];
    let uses = OperandUses::new(operands);
    if let Some(lead) = operand_at(operands, span.start) {
        match lead.role {
            NativeOperandRole::ExplicitOption => {
                return option_group(text, span, lead, operands, uses);
            }
            NativeOperandRole::Environment {
                enclosure: scope, ..
            } if admission != Admission::ProtectedSyntax && scope == enclosure => {
                let name = environment::native_atom(source.view(), &span, operands, lead.role);
                return Ok((EntryKind::EnvironmentVariable, Recognition::one(name)));
            }
            NativeOperandRole::Command if admission != Admission::ProtectedSyntax => {
                let name = command_group(text, span, lead, uses);
                return Ok((EntryKind::Command, Recognition::one(name)));
            }
            _ => {}
        }
    }
    if intersecting_operands(operands, span.clone())
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
    if let Some(result) = recognize_part(text, part, operands, enclosure) {
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
    Ok((
        EntryKind::Term,
        Recognition {
            names,
            ..Recognition::default()
        },
    ))
}

fn independent_role(role: NativeOperandRole) -> bool {
    matches!(
        role,
        NativeOperandRole::Environment { .. } | NativeOperandRole::Command
    )
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
    // termp_fl_pre writes a separate dash followed by the accepted Fl word.
    // Both receipts must cover the contiguous key; a later operand starts
    // only after the ordinary word boundary.
    let mut end = option.bytes.end;
    let mut saw_operand = !text[option.bytes.clone()].trim().bytes().all(|b| b == b'-');
    for next in intersecting_operands(operands, span.clone()).skip(1) {
        if saw_operand || next.role != NativeOperandRole::ExplicitOption || next.bytes.start != end
        {
            break;
        }
        end = next.bytes.end;
        saw_operand = !text[next.bytes.clone()].trim().bytes().all(|b| b == b'-');
    }
    if !text[span.end.min(end)..end].trim().is_empty() {
        return Err(RejectedPart::Unproved);
    }
    let end = end.min(span.end);
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

fn command_group(
    text: &str,
    span: Range<usize>,
    lead: &NativeOperand,
    uses: OperandUses<'_>,
) -> Option<RecognizedName> {
    let name = text
        .get(span.start..lead.bytes.end.min(span.end))?
        .trim_end();
    let tail = text.get(span.start + name.len()..span.end)?;
    if commands::is_command_name(name) && (tail.is_empty() || tail.starts_with(char::is_whitespace))
    {
        commands::command_binding(&text[span.clone()], name, span.start, uses)
    } else {
        None
    }
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
