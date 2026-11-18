//! Native roles and accepted byte ranges carried from text execution.
use std::{ops::Range, rc::Rc};

/// Actual `pre_alternate` operands and generated `Fl` words retain their
/// declaration roles through font escapes and accepted HEAD output only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeOperandRole {
    #[cfg(any(feature = "roff", test))]
    Literal,
    #[cfg(any(feature = "roff", test))]
    Argument,
    /// An `Ar` spelling may name an authored operand definition directly.
    #[cfg(feature = "roff")]
    Placeholder,
    /// Recovery or clipping retained part of an Ar without proving its name.
    #[cfg(feature = "roff")]
    IncompletePlaceholder,
    /// The native Ar belongs to a source-proven search form even if native
    /// execution subsequently overwrites the opening sign.
    #[cfg(feature = "roff")]
    SearchArgument(u32),
    #[cfg(feature = "roff")]
    SearchOpen {
        sign: char,
        argument: u32,
        closed: bool,
    },
    #[cfg(feature = "roff")]
    SearchClose,
    /// A sign in a compound search expression. Only `SearchOpen` can prove a bare template.
    #[cfg(feature = "roff")]
    SearchSign,
    /// Provisional receipts from one mdoc Fl. Acceptance joins the dash
    /// and the surviving word from the same macro into an `ExplicitOption`.
    #[cfg(feature = "roff")]
    FlDash(u32),
    #[cfg(feature = "roff")]
    FlWord(u32),
    /// The first actual man OP operand still has to spell a complete flag.
    #[cfg(feature = "roff")]
    OptionalKey,
    /// Styled text executed by its own mdoc macro in a declaration HEAD.
    #[cfg(feature = "roff")]
    Description,
    #[cfg(feature = "roff")]
    Subject(SubjectReceipt),
    /// Publication retained the native subject's role within the form,
    /// while clipping the evidence for its own exact name.
    #[cfg(feature = "roff")]
    ClippedSubject(SubjectReceipt),
    #[cfg(feature = "roff")]
    EnclosureStart(u32),
    #[cfg(feature = "roff")]
    EnclosureEnd(u32),
    /// Accepted residue of a delimiter that cannot prove a paired boundary.
    #[cfg(feature = "roff")]
    ClippedEnclosure,
    #[cfg(feature = "roff")]
    EnclosureContent(u32),
    ExplicitOption,
}

#[cfg(feature = "roff")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeSubject {
    Constant,
    Error,
    Variable,
    Type,
    ReturnType,
    Field,
    Function,
    FunctionBlock,
}

#[cfg(feature = "roff")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubjectReceipt {
    pub(crate) kind: NativeSubject,
    pub(crate) node: u32,
    pub(crate) enclosure: Option<u32>,
}

impl NativeOperandRole {
    pub(in crate::definitions) fn is_parameter(self) -> bool {
        match self {
            #[cfg(any(feature = "roff", test))]
            Self::Argument => true,
            #[cfg(feature = "roff")]
            Self::Placeholder
            | Self::IncompletePlaceholder
            | Self::SearchArgument(_)
            | Self::OptionalKey
            | Self::Subject(..)
            | Self::ClippedSubject(..)
            | Self::ClippedEnclosure => true,
            _ => false,
        }
    }

    #[cfg(feature = "roff")]
    pub(in crate::definitions) fn is_search(self) -> bool {
        matches!(
            self,
            Self::SearchArgument(_)
                | Self::SearchOpen { .. }
                | Self::SearchClose
                | Self::SearchSign
        )
    }

    #[cfg(feature = "roff")]
    pub(crate) fn is_operand_label(self) -> bool {
        matches!(self, Self::Placeholder | Self::IncompletePlaceholder)
    }

    #[cfg(feature = "roff")]
    pub(crate) fn subject_word(kind: NativeSubject, node: u32) -> Self {
        Self::Subject(SubjectReceipt {
            kind,
            node,
            enclosure: None,
        })
    }

    #[cfg(feature = "roff")]
    pub(crate) fn record_enclosure(self, enclosure: impl FnOnce() -> Option<u32>) -> Self {
        match self {
            Self::Subject(mut subject) => {
                subject.enclosure = enclosure();
                Self::Subject(subject)
            }
            role => role,
        }
    }

    #[cfg(feature = "roff")]
    pub(in crate::definitions) fn subject(self) -> Option<SubjectReceipt> {
        match self {
            Self::Subject(subject) | Self::ClippedSubject(subject) => Some(subject),
            _ => None,
        }
    }

    #[cfg(feature = "roff")]
    pub(crate) fn without_complete_spelling(self) -> Self {
        match self {
            Self::Subject(subject) => Self::ClippedSubject(subject),
            Self::EnclosureStart(_) | Self::EnclosureEnd(_) => Self::ClippedEnclosure,
            role => role,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeOperand {
    pub(crate) bytes: Range<usize>,
    pub(crate) role: NativeOperandRole,
}

#[derive(Clone, Default)]
#[cfg(feature = "roff")]
pub(crate) struct CapturedHeadOperands {
    pub(crate) text: String,
    pub(crate) operands: Vec<NativeOperand>,
}

#[cfg(feature = "roff")]
impl CapturedHeadOperands {
    pub(crate) fn qualify_fl_keys(&mut self, search_head: bool) {
        let check_scopes = search_head
            || self
                .operands
                .iter()
                .find(|operand| {
                    !matches!(
                        operand.role,
                        NativeOperandRole::EnclosureStart(_)
                            | NativeOperandRole::EnclosureEnd(_)
                            | NativeOperandRole::Description
                    )
                })
                .is_some_and(|operand| operand.role.is_operand_label());
        if !check_scopes {
            for operand in &mut self.operands {
                if matches!(
                    operand.role,
                    NativeOperandRole::FlDash(_) | NativeOperandRole::FlWord(_)
                ) {
                    operand.role = NativeOperandRole::ExplicitOption;
                }
            }
            return;
        }
        if !self.operands.iter().any(|operand| {
            matches!(
                operand.role,
                NativeOperandRole::FlDash(_) | NativeOperandRole::FlWord(_)
            )
        }) {
            return;
        }
        let mut raw = std::mem::take(&mut self.operands).into_iter().peekable();
        while let Some(mut operand) = raw.next() {
            match operand.role {
                NativeOperandRole::FlDash(scope) => {
                    if let Some(word) = raw.peek()
                        && word.role == NativeOperandRole::FlWord(scope)
                        && operand.bytes.end == word.bytes.start
                        && self.text[operand.bytes.clone()].trim_start() == "-"
                    {
                        operand.bytes.end = word.bytes.end;
                        operand.role = NativeOperandRole::ExplicitOption;
                        raw.next();
                        self.operands.push(operand);
                        continue;
                    }
                    operand.role = NativeOperandRole::Argument;
                }
                NativeOperandRole::FlWord(_) => operand.role = NativeOperandRole::Argument,
                _ => {}
            }
            self.operands.push(operand);
        }
    }

    pub(crate) fn qualify_optional_keys(&mut self) {
        for operand in &mut self.operands {
            if operand.role != NativeOperandRole::OptionalKey {
                continue;
            }
            let key = self.text.get(operand.bytes.clone()).map(str::trim);
            operand.role =
                if key.is_some_and(|key| crate::definitions::option_prefix(key) == Some(key)) {
                    NativeOperandRole::ExplicitOption
                } else {
                    NativeOperandRole::Argument
                };
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) enum OperandEvidence<T> {
    #[default]
    Absent,
    Accepted(T),
    Unavailable,
}

pub(crate) type TermOperands = OperandEvidence<Rc<Vec<NativeOperand>>>;
pub(in crate::definitions) type HeadOperands<'a> = OperandEvidence<&'a [TermOperands]>;

impl<'a> HeadOperands<'a> {
    /// A missing receipt permits ordinary lexical recognition. A rejected
    /// receipt must not resurrect an argument as a font-derived name.
    pub(in crate::definitions) fn for_term(self, index: usize) -> Option<&'a [NativeOperand]> {
        match self {
            Self::Absent => Some(&[]),
            Self::Accepted(terms) => match terms.get(index)? {
                TermOperands::Absent => Some(&[]),
                TermOperands::Accepted(operands) => Some(operands),
                TermOperands::Unavailable => None,
            },
            Self::Unavailable => None,
        }
    }

    pub(in crate::definitions) fn proves_option(self) -> bool {
        let Self::Accepted(terms) = self else {
            return false;
        };
        (0..terms.len()).any(|index| {
            self.for_term(index).is_some_and(|operands| {
                operands
                    .iter()
                    .any(|operand| operand.role == NativeOperandRole::ExplicitOption)
            })
        })
    }
}
