//! Recognize independent names in a native HEAD with protected operands.
use super::{
    Recognition, annotations, following_function,
    intervals::{local_operands, scoped_operands, split_inline_intervals},
    invocation, offset, operand_at, operand_cursor, recognize_part, unwrapped,
};
use crate::definitions::syntax::{
    DeclarationLimit, commands,
    eligibility::{NameSource, OperandUses, intersecting_operands},
    inference::infer_lexical_terms,
    named,
    scan::{MAX_NAMES, MAX_NESTING},
};
use crate::definitions::{
    NativeHeadRole, NativeOperand, NativeOperandRole, NativeSubject, RecognizedName,
    context::DefinitionContext, evidence::HeadOperands,
};
use mant_ir::{DefinitionTerm, EntryKind, Inline, NameCase};
use std::ops::Range;

#[derive(Default)]
struct Candidates {
    names: Vec<Candidate>,
    unproved_part: bool,
    unproved_form: bool,
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
    case: NameCase,
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
    text: &'a str,
    operands: &'a [NativeOperand],
    context: DefinitionContext,
    ranges: Vec<Range<usize>>,
    terms: Vec<DefinitionTerm>,
    argument_separators: Vec<usize>,
}

impl Head<'_> {
    fn argument_separators(&self, span: &Range<usize>) -> &[usize] {
        let first = self
            .argument_separators
            .partition_point(|at| *at < span.start);
        let count = self.argument_separators[first..].partition_point(|at| *at < span.end);
        &self.argument_separators[first..first + count]
    }
}

enum Group {
    Part(usize),
    Enclosed {
        scope: Option<u32>,
        groups: Vec<Group>,
    },
    Unproved,
}

#[derive(Default)]
struct Partition {
    ranges: Vec<Range<usize>>,
    separated: bool,
    argument_separators: Vec<usize>,
}

pub(super) fn recognize(
    nodes: &[Inline],
    text: &str,
    operands: &[NativeOperand],
    context: DefinitionContext,
    admission: Admission,
) -> Option<(EntryKind, Recognition)> {
    if !text.contains([',', '|']) {
        return None;
    }
    let mut partition = Partition::default();
    let (head, enclosure) = unwrap_search(text, text, operands);
    let groups = partition.groups(text, head, operands, enclosure, 0, admission)?;
    let protected_arguments = !partition.argument_separators.is_empty();
    if !partition.separated && !protected_arguments
        || admission == Admission::AfterSeparator
            && !operands
                .iter()
                .any(|operand| operand.role.subject().is_some())
            && !protected_arguments
            && !partition.has_mixed_roles(operands)
    {
        return None;
    }
    let terms = split_inline_intervals(nodes, partition.ranges.iter().cloned())
        .into_iter()
        .map(DefinitionTerm::from)
        .collect();
    partition.argument_separators.sort_unstable();
    partition.argument_separators.dedup();
    let source = Head {
        text,
        operands,
        context,
        ranges: partition.ranges,
        terms,
        argument_separators: partition.argument_separators,
    };
    let mut candidates = Candidates::default();
    let collected = collect(&source, &groups, enclosure, admission, &mut candidates);
    // A weak declaration cannot publish a partial lexical alias without
    // a proved native sibling. An observed coverage limit is still a result.
    if admission == Admission::AfterSeparator
        && !matches!(collected, Err(RejectedPart::NameLimit))
        && !candidates.names.iter().any(|name| name.explicit)
    {
        return None;
    }
    Some(candidates.resolve(collected))
}

impl Partition {
    fn has_mixed_roles(&self, operands: &[NativeOperand]) -> bool {
        let mut families =
            self.ranges
                .iter()
                .filter_map(|range| match operand_at(operands, range.start)?.role {
                    NativeOperandRole::ExplicitOption => Some(NativeHeadRole::Option),
                    NativeOperandRole::Environment { .. } => Some(NativeHeadRole::Environment),
                    NativeOperandRole::Command => Some(NativeHeadRole::Literal),
                    _ => None,
                });
        families
            .next()
            .is_some_and(|first| families.any(|next| next != first))
    }

    fn groups(
        &mut self,
        text: &str,
        head: &str,
        operands: &[NativeOperand],
        enclosure: Option<u32>,
        depth: usize,
        admission: Admission,
    ) -> Option<Vec<Group>> {
        if depth == MAX_NESTING {
            return None;
        }
        let parts = independent_groups(
            text,
            head,
            enclosure,
            operands,
            admission,
            &mut self.argument_separators,
        )?;
        self.separated |= parts.len() > 1;
        Some(
            parts
                .into_iter()
                .map(|part| {
                    let part = part.trim();
                    let (inner, nested) = unwrap_search(text, part, operands);
                    if inner.len() == part.len() {
                        let index = self.ranges.len();
                        let start = offset(text, part);
                        self.ranges.push(start..start + part.len());
                        Group::Part(index)
                    } else {
                        self.groups(
                            text,
                            inner,
                            operands,
                            nested.or(enclosure),
                            depth + 1,
                            admission,
                        )
                        .map_or(Group::Unproved, |groups| {
                            Group::Enclosed {
                                scope: nested,
                                groups,
                            }
                        })
                    }
                })
                .collect(),
        )
    }
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
                    let category = (candidate.kind, candidate.case);
                    kind = Some(match kind {
                        Some(previous) if previous != category => {
                            (EntryKind::Term, NameCase::Sensitive)
                        }
                        _ => category,
                    });
                    candidate.name
                })
                .collect();
            let (kind, case) = kind.unwrap_or((EntryKind::Term, NameCase::Sensitive));
            return (
                kind,
                Recognition {
                    names,
                    case: Some(case),
                    unproved_form: self.unproved_form,
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
    groups: &[Group],
    enclosure: Option<u32>,
    admission: Admission,
    candidates: &mut Candidates,
) -> Result<(), RejectedPart> {
    for group in groups {
        if matches!(candidates.sequence, SubjectSequence::ArgumentTail(_)) {
            break;
        }
        match group {
            Group::Part(index) => collect_group(source, *index, enclosure, admission, candidates)?,
            Group::Enclosed { scope, groups } => {
                collect(source, groups, scope.or(enclosure), admission, candidates)?;
                if let Some(nested) = scope {
                    candidates.leave_enclosure(*nested, enclosure);
                }
            }
            Group::Unproved => candidates.unproved_part = true,
        }
    }
    Ok(())
}

fn collect_group(
    source: &Head<'_>,
    index: usize,
    enclosure: Option<u32>,
    admission: Admission,
    candidates: &mut Candidates,
) -> Result<(), RejectedPart> {
    let (text, operands) = (source.text, source.operands);
    let span = &source.ranges[index];
    if OperandUses::new(operands).covers(span, search_argument) {
        return Ok(());
    }
    let Ok(subject) = group_subject(text, span, operands, enclosure) else {
        candidates.unproved_part = true;
        return Ok(());
    };
    if !candidates.admit_subject(subject, enclosure) {
        return Ok(());
    }
    let lead = operand_at(operands, span.start);
    let operand_label = lead.is_some_and(|operand| {
        operand.role == NativeOperandRole::ExplicitOption
            || operand.role == NativeOperandRole::Placeholder
    });
    let hint = group_hint(source, index, lead, admission, enclosure);
    let explicit = hint.is_some()
        || operand_label
        || lead.is_some_and(|operand| operand.role.subject().is_some());
    let Ok((kind, result)) = recognize_group(source, index, hint, admission, enclosure) else {
        candidates.unproved_part = true;
        return Ok(());
    };
    if result.limit.is_some() || candidates.names.len() + result.names.len() > MAX_NAMES {
        return Err(RejectedPart::NameLimit);
    }
    candidates.unproved_form |= result.unproved_form;
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
            case: result.case.unwrap_or(NameCase::Sensitive),
            explicit,
        }));
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
    text: &str,
    head: &'a str,
    enclosure: Option<u32>,
    operands: &[NativeOperand],
    admission: Admission,
    argument_separators: &mut Vec<usize>,
) -> Option<Vec<&'a str>> {
    // Protect accepted words in a nested enclosure even if its opening or
    // closing glyph was overwritten. Only the unwrapped BODY/ELEM supplies
    // separators at this level.
    let start = offset(text, head);
    let first = operand_cursor(operands, start);
    let mut receipts = operands[first..].iter().peekable();
    let mut last = None;
    let mut syntax = invocation::Syntax::new(text, operands);
    let mut group_start = start;
    named::split_named_groups(head, |index, top_level| {
        let offset = start + index;
        while let Some(operand) = receipts.next_if(|operand| operand.bytes.start <= offset) {
            last = Some(operand);
        }
        let current = last.filter(|operand| operand.bytes.contains(&offset));
        let separator = matches!(head.as_bytes()[index], b',' | b'|');
        if top_level && separator && syntax.protects(group_start, offset) {
            argument_separators.push(offset);
            return true;
        }
        let protected = if separator && admission != Admission::ProtectedSyntax {
            // In a weak prefix, punctuation embedded in a prose operand
            // cannot authorize a later native subject as its own declaration.
            current.is_none_or(|operand| {
                !(operand.role == NativeOperandRole::LiteralContinuation(enclosure)
                    || operand.role == NativeOperandRole::PipeDelimiter(enclosure)
                    || enclosure.is_some_and(|body| {
                        operand.role == NativeOperandRole::EnclosureContent(body)
                    }))
                    || operand.bytes.start < start
                    || head
                        .get(operand.bytes.start - start..operand.bytes.end - start)
                        .is_none_or(|value| !matches!(value.trim(), "," | "|"))
            })
        } else {
            current.is_some_and(|operand| {
                operand.role.is_parameter()
                    || match operand.role {
                        NativeOperandRole::ExplicitOption
                        | NativeOperandRole::Description
                        | NativeOperandRole::SymbolicText
                        | NativeOperandRole::Command
                        | NativeOperandRole::LiteralPunctuation
                        | NativeOperandRole::ReferenceText(_)
                        | NativeOperandRole::EnclosureStart { .. }
                        | NativeOperandRole::EnclosureEnd(_) => true,
                        NativeOperandRole::EnclosureContent(scope) => enclosure != Some(scope),
                        NativeOperandRole::PipeDelimiter(scope) => enclosure != scope,
                        NativeOperandRole::LiteralDelimiter {
                            enclosure: owner, ..
                        } => enclosure != owner,
                        _ => false,
                    }
            })
        };
        if top_level && separator && !protected {
            group_start = offset + 1;
        }
        protected
    })
}

fn group_hint(
    source: &Head<'_>,
    index: usize,
    lead: Option<&NativeOperand>,
    admission: Admission,
    enclosure: Option<u32>,
) -> Option<NativeHeadRole> {
    match lead.map(|operand| operand.role) {
        Some(NativeOperandRole::ExplicitOption) => Some(NativeHeadRole::Option),
        Some(NativeOperandRole::Environment {
            enclosure: scope, ..
        }) if admission != Admission::ProtectedSyntax && scope == enclosure => {
            Some(NativeHeadRole::Environment)
        }
        Some(NativeOperandRole::Command)
            if admission != Admission::ProtectedSyntax
                || !source.argument_separators(&source.ranges[index]).is_empty() =>
        {
            Some(NativeHeadRole::Literal)
        }
        // A styled Nm followed by Cm can prove one compound command.
        // An Ar operand or typed subject preceding Ic still supplies its
        // own role even when the whole HEAD appears under COMMANDS.
        _ if admission != Admission::ProtectedSyntax
            && source.context == DefinitionContext::Commands
            && lead.is_none_or(|operand| {
                operand.role.subject().is_none() && !operand.role.is_operand_label()
            })
            && commands::leading_styled_command_name(&source.terms[index]).is_some()
            && intersecting_operands(source.operands, source.ranges[index].clone())
                .any(|operand| operand.role == NativeOperandRole::Command) =>
        {
            Some(NativeHeadRole::Literal)
        }
        _ => None,
    }
}

fn recognize_group(
    source: &Head<'_>,
    index: usize,
    hint: Option<NativeHeadRole>,
    admission: Admission,
    enclosure: Option<u32>,
) -> Result<(EntryKind, Recognition), RejectedPart> {
    let (text, operands) = (source.text, source.operands);
    let span = &source.ranges[index];
    let part = &text[span.clone()];
    if let Some(hint) = hint {
        return recognize_lexical_group(source, index, hint, admission);
    }
    if intersecting_operands(operands, span.clone())
        .any(|operand| operand.role == NativeOperandRole::ExplicitOption)
    {
        // Joined text from another operand is neither a complete option
        // nor a new lexical spelling containing both operands.
        return Err(RejectedPart::Unproved);
    }
    if let Some(result) = recognize_part(text, part, operands, enclosure) {
        return Ok(result);
    }
    if admission == Admission::TypedSubjects {
        return Err(RejectedPart::Unproved);
    }
    let uses = OperandUses::new(operands).shifted(span.start);
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

fn recognize_lexical_group(
    source: &Head<'_>,
    index: usize,
    hint: NativeHeadRole,
    admission: Admission,
) -> Result<(EntryKind, Recognition), RejectedPart> {
    let original = &source.ranges[index];
    let term = &source.terms[index];
    let protected = hint == NativeHeadRole::Option && admission == Admission::ProtectedSyntax;
    let (range, narrowed) = if protected {
        let range = protected_option_span(source, original)?;
        let content = split_inline_intervals(term, std::iter::once(0..range.len()))
            .pop()
            .expect("one interval");
        (range, Some(DefinitionTerm::from(content)))
    } else {
        (original.clone(), None)
    };
    let scoped = [local_operands(invocation::protect_arguments(
        scoped_operands(source.operands, range.clone()),
        source.argument_separators(&range),
        range.start,
    ))];
    let found = infer_lexical_terms(
        std::slice::from_ref(narrowed.as_ref().unwrap_or(term)),
        &source.text[range.clone()],
        source.context,
        Some(hint),
        HeadOperands::Accepted(&scoped),
    );
    let mut unproved_form = !protected && !found.excluded_forms.is_empty();
    let names = found
        .occurrences
        .into_iter()
        .flatten()
        .map(|name| rebase(name, range.start))
        .filter(|name| {
            let proved = hint != NativeHeadRole::Option
                || name
                    .parts
                    .iter()
                    .all(|span| option_key_is_contiguous(source, span));
            unproved_form |= !protected && !proved;
            proved
        })
        .collect();
    Ok((
        found.kind,
        Recognition {
            names,
            case: Some(found.case),
            limit: found.limit,
            unproved_form,
            ..Recognition::default()
        },
    ))
}

fn option_key_is_contiguous(source: &Head<'_>, span: &Range<usize>) -> bool {
    let mut operands = intersecting_operands(source.operands, span.clone())
        .filter(|operand| operand.role == NativeOperandRole::ExplicitOption);
    let Some(first) = operands.next() else {
        return true;
    };
    // A separator in the same Fl word cannot name another flag; Ns
    // cannot glue two flags into one key. Nested Fl may supply the leading
    // dashes before the operand containing the actual name.
    let text = source.text;
    text[first.bytes.start..span.start.max(first.bytes.start)]
        .trim()
        .is_empty()
        && std::iter::once(first)
            .chain(operands)
            .take_while(|operand| operand.bytes.end < span.end)
            .all(|operand| {
                text[operand.bytes.clone()]
                    .trim()
                    .bytes()
                    .all(|byte| byte == b'-')
            })
}

fn protected_option_span(
    source: &Head<'_>,
    span: &Range<usize>,
) -> Result<Range<usize>, RejectedPart> {
    let text = source.text;
    let operands = source.operands;
    let mut receipts = intersecting_operands(operands, span.clone());
    let option = receipts.next().ok_or(RejectedPart::Unproved)?;
    let mut end = option.bytes.end;
    let mut has_key = !text[option.bytes.clone()]
        .trim()
        .bytes()
        .all(|byte| byte == b'-');
    for next in receipts {
        if has_key || next.role != NativeOperandRole::ExplicitOption || next.bytes.start != end {
            break;
        }
        end = next.bytes.end;
        has_key = !text[next.bytes.clone()]
            .trim()
            .bytes()
            .all(|byte| byte == b'-');
    }
    if !text[span.end.min(end)..end].trim().is_empty() {
        return Err(RejectedPart::Unproved);
    }
    let end = end.min(span.end);
    let (suffix, _) = unwrapped(text, &text[end..span.end], operands);
    let at = offset(text, suffix);
    if !suffix.is_empty()
        && !OperandUses::new(operands).covers(&(at..at + suffix.len()), |role| {
            role.is_parameter() || role.is_search()
        })
    {
        return Err(RejectedPart::Unproved);
    }
    Ok(span.start..end)
}

pub(super) fn independent_role(role: NativeOperandRole) -> bool {
    matches!(
        role,
        NativeOperandRole::Environment { .. }
            | NativeOperandRole::Command
            | NativeOperandRole::ExplicitOption
    )
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
