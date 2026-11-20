//! Native roles and accepted byte ranges carried from text execution.
use std::{ops::Range, rc::Rc};

/// Actual `pre_alternate` operands and generated `Fl` words retain their
/// declaration roles through font escapes and accepted HEAD output only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeOperandRole {
    #[cfg(any(feature = "roff", test))]
    Literal,
    /// A visible No operand that can intentionally continue an Ev spelling.
    #[cfg(feature = "roff")]
    LiteralContinuation(Option<u32>),
    /// The mdoc parser emitted a bare middle delimiter between macro scopes.
    #[cfg(feature = "roff")]
    PipeDelimiter(Option<u32>),
    /// Bare punctuation that closes and reopens one executed mdoc Fl.
    #[cfg(feature = "roff")]
    FlReopening {
        pipe: bool,
        enclosure: Option<u32>,
    },
    /// A comma or pipe belongs to the complete text of a No operand.
    #[cfg(feature = "roff")]
    LiteralPunctuation,
    #[cfg(any(feature = "roff", test))]
    Argument,
    /// A tbl cell contributes readable content without declaring a name or form.
    #[cfg(feature = "roff")]
    DisplayOnly,
    /// An `Ar` spelling may name an authored operand definition directly.
    /// Its node ID also groups accepted text fragments of the same parameter.
    #[cfg(feature = "roff")]
    Placeholder(u32),
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
    /// A later native write replaced part of this Fl word.
    #[cfg(feature = "roff")]
    ClippedOption,
    /// A closing TEXT delimiter can be adjacent to an empty Fl's dash.
    #[cfg(feature = "roff")]
    OptionPunctuation,
    /// The first actual man OP operand still has to spell a complete flag.
    #[cfg(feature = "roff")]
    OptionalKey,
    /// Styled text executed by its own mdoc macro in a declaration HEAD.
    #[cfg(feature = "roff")]
    Description,
    /// An independently executed mdoc Sy word.
    #[cfg(feature = "roff")]
    SymbolicText,
    /// An executed Cm/Ic operand can open an independent command declaration.
    #[cfg(feature = "roff")]
    Command,
    /// One Nm scope can execute several words of an authored command name.
    #[cfg(feature = "roff")]
    ProgramName(u32),
    /// The surviving part of an invocation word cannot prove its name.
    #[cfg(feature = "roff")]
    ClippedInvocation,
    /// The device wrote this Fn/Fo signature delimiter.
    #[cfg(feature = "roff")]
    FunctionSignature {
        opening: bool,
    },
    /// Visible link operands, including the terminal's generated suffix.
    #[cfg(feature = "roff")]
    ReferenceText(Option<u32>),
    #[cfg(feature = "roff")]
    Subject(SubjectReceipt),
    /// Publication retained the native subject's role within the form,
    /// while clipping the evidence for its own exact name.
    #[cfg(feature = "roff")]
    ClippedSubject(SubjectReceipt),
    #[cfg(feature = "roff")]
    Environment {
        invocation: Option<u32>,
        enclosure: Option<u32>,
    },
    #[cfg(feature = "roff")]
    ClippedEnvironment {
        invocation: Option<u32>,
        enclosure: Option<u32>,
    },
    #[cfg(feature = "roff")]
    EnclosureStart {
        body: u32,
        parent: Option<u32>,
    },
    #[cfg(feature = "roff")]
    EnclosureEnd(u32),
    /// Accepted residue of a delimiter that cannot prove a paired boundary.
    #[cfg(feature = "roff")]
    ClippedEnclosure,
    #[cfg(feature = "roff")]
    EnclosureContent(u32),
    /// A standalone, executed mdoc parenthesis or bracket from a TEXT node.
    #[cfg(feature = "roff")]
    LiteralDelimiter {
        glyph: char,
        enclosure: Option<u32>,
    },
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
    pub(crate) invocation: Option<u32>,
    pub(crate) enclosure: Option<u32>,
}

impl NativeOperandRole {
    pub(in crate::definitions) fn is_parameter(self) -> bool {
        match self {
            #[cfg(any(feature = "roff", test))]
            Self::Argument => true,
            #[cfg(feature = "roff")]
            Self::DisplayOnly
            | Self::ClippedOption
            | Self::Placeholder(_)
            | Self::IncompletePlaceholder
            | Self::SearchArgument(_)
            | Self::OptionalKey
            | Self::Subject(..)
            | Self::ClippedSubject(..)
            | Self::ClippedEnvironment { .. }
            | Self::ClippedEnclosure => true,
            _ => false,
        }
    }

    #[cfg(feature = "roff")]
    pub(in crate::definitions) fn display_only(self) -> bool {
        self == Self::DisplayOnly
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
        matches!(self, Self::Placeholder(_) | Self::IncompletePlaceholder)
    }

    #[cfg(feature = "roff")]
    pub(crate) fn is_description(self) -> bool {
        matches!(
            self,
            Self::Description
                | Self::SymbolicText
                | Self::Command
                | Self::ProgramName(_)
                | Self::ClippedInvocation
        )
    }

    #[cfg(feature = "roff")]
    pub(in crate::definitions) fn is_invocation_name(self) -> bool {
        matches!(self, Self::Command | Self::ProgramName(_))
    }

    #[cfg(feature = "roff")]
    pub(in crate::definitions) fn declaration_delimiter(self) -> Self {
        match self {
            Self::FlReopening {
                pipe: true,
                enclosure,
            } => Self::PipeDelimiter(enclosure),
            Self::FlReopening {
                pipe: false,
                enclosure: Some(scope),
            } => Self::EnclosureContent(scope),
            Self::FlReopening {
                pipe: false,
                enclosure: None,
            } => Self::LiteralContinuation(None),
            role => role,
        }
    }

    #[cfg(feature = "roff")]
    pub(crate) fn subject_word(kind: NativeSubject, node: u32, invocation: Option<u32>) -> Self {
        Self::Subject(SubjectReceipt {
            kind,
            node,
            invocation,
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
            Self::Environment { invocation, .. } => Self::Environment {
                invocation,
                enclosure: enclosure(),
            },
            Self::EnclosureStart { body, .. } => Self::EnclosureStart {
                body,
                parent: enclosure(),
            },
            Self::ReferenceText(_) => Self::ReferenceText(enclosure()),
            Self::LiteralContinuation(_) => Self::LiteralContinuation(enclosure()),
            Self::PipeDelimiter(_) => Self::PipeDelimiter(enclosure()),
            Self::FlReopening { pipe, .. } => Self::FlReopening {
                pipe,
                enclosure: enclosure(),
            },
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
            Self::Environment {
                invocation,
                enclosure,
            } => Self::ClippedEnvironment {
                invocation,
                enclosure,
            },
            Self::LiteralContinuation(_)
            | Self::PipeDelimiter(_)
            | Self::FlReopening { .. }
            | Self::FunctionSignature { .. } => Self::Argument,
            Self::Command | Self::ProgramName(_) => Self::ClippedInvocation,
            Self::EnclosureStart { .. } | Self::EnclosureEnd(_) => Self::ClippedEnclosure,
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
    pub(crate) bodies: Vec<Range<usize>>,
}

#[derive(Clone, Default)]
pub(crate) struct AcceptedOperands {
    pub(in crate::definitions) operands: Vec<NativeOperand>,
    #[cfg(feature = "roff")]
    pub(in crate::definitions) bodies: Vec<Range<usize>>,
}

#[cfg(feature = "roff")]
pub(super) fn split_accepted_term_operands(
    mut captured: CapturedHeadOperands,
    terms: &[mant_ir::DefinitionTerm],
    retired_break: bool,
) -> Option<Vec<TermOperands>> {
    let mut offset = 0;
    let mut bounds = Vec::new();
    for (index, term) in terms.iter().enumerate() {
        let actual = mant_ir::inline_plain_text(term);
        let end = offset + actual.len();
        if captured.text.get(offset..end)? != actual {
            return None;
        }
        let next = if index + 1 < terms.len() {
            // split_definition_terms consumes the alt-marker's single
            // executed line break. It keeps earlier device rows as text.
            (captured.text.as_bytes().get(end) == Some(&b'\n')).then_some(end + 1)?
        } else {
            end
        };
        if terms.len() > 1 {
            bounds.push((offset..end, next));
        }
        offset = next;
    }
    match &captured.text[offset..] {
        "" => {}
        "\n" if retired_break => {
            // Definition layout retired the actual final HEAD endline.
            // No other dropped or rearranged text can prove name ranges.
            captured.operands.retain_mut(|operand| {
                operand.bytes.end = operand.bytes.end.min(offset);
                operand.bytes.start < operand.bytes.end
            });
        }
        _ => return None,
    }
    if let [_] = terms {
        let mut accepted = AcceptedOperands {
            operands: captured.operands,
            bodies: captured.bodies,
        };
        accepted.bodies.retain_mut(|span| {
            span.end = span.end.min(offset);
            span.start < span.end
        });
        return Some(vec![TermOperands::Accepted(Rc::new(accepted))]);
    }
    let mut native = captured.operands.into_iter().peekable();
    let mut bodies = captured.bodies.into_iter().peekable();
    let mut result = Vec::with_capacity(bounds.len());
    for (bound, next) in bounds {
        let mut local = Vec::new();
        while let Some(mut operand) = native.next_if(|operand| operand.bytes.start < next) {
            if operand.bytes.start < bound.start || operand.bytes.end > next {
                // Only the structural separator may be removed, never a
                // word spanning visible glyphs from two different terms.
                return None;
            }
            if operand.bytes.start == bound.end {
                // The discarded separator may have its own receipt.
                continue;
            }
            operand.bytes.end = operand.bytes.end.min(bound.end);
            operand.bytes.start -= bound.start;
            operand.bytes.end -= bound.start;
            local.push(operand);
        }
        let mut local_bodies = Vec::new();
        while let Some(span) = bodies.peek().filter(|span| span.start < bound.end) {
            let start = span.start.max(bound.start).min(bound.end);
            let end = span.end.min(bound.end);
            if start < end {
                local_bodies.push(start - bound.start..end - bound.start);
            }
            if span.end > next {
                break;
            }
            bodies.next();
        }
        result.push(TermOperands::Accepted(Rc::new(AcceptedOperands {
            operands: local,
            bodies: local_bodies,
        })));
    }
    native.next().is_none().then_some(result)
}

#[cfg(feature = "roff")]
impl CapturedHeadOperands {
    pub(crate) fn qualify_fl_keys(&mut self, search_head: bool) {
        let check_scopes = search_head
            || self
                .operands
                .iter()
                .find(|operand| {
                    !operand.role.is_description()
                        && !matches!(
                            operand.role,
                            NativeOperandRole::EnclosureStart { .. }
                                | NativeOperandRole::EnclosureEnd(_)
                                | NativeOperandRole::LiteralDelimiter { .. }
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

pub(crate) type TermOperands = OperandEvidence<Rc<AcceptedOperands>>;
pub(in crate::definitions) type HeadOperands<'a> = OperandEvidence<&'a [TermOperands]>;

impl<'a> HeadOperands<'a> {
    /// A missing receipt permits ordinary lexical recognition. A rejected
    /// receipt must not resurrect an argument as a font-derived name.
    pub(in crate::definitions) fn for_term(self, index: usize) -> Option<&'a [NativeOperand]> {
        match self {
            Self::Absent => Some(&[]),
            Self::Accepted(terms) => match terms.get(index)? {
                TermOperands::Absent => Some(&[]),
                TermOperands::Accepted(accepted) => Some(&accepted.operands),
                TermOperands::Unavailable => None,
            },
            Self::Unavailable => None,
        }
    }

    #[cfg(feature = "roff")]
    pub(in crate::definitions) fn body_ranges(self, index: usize) -> &'a [Range<usize>] {
        if let Self::Accepted(terms) = self
            && let Some(TermOperands::Accepted(accepted)) = terms.get(index)
        {
            return &accepted.bodies;
        }
        &[]
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
