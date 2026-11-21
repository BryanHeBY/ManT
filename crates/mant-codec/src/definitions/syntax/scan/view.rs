//! Accepted visible bytes, effective fonts and actual native operand receipts.
use super::{Inline, NativeOperand, NativeOperandRole, Range};
use crate::definitions::syntax::eligibility::operand_at;

#[cfg(feature = "roff")]
mod assignment_angles;

#[derive(Clone, Copy, Default)]
pub(super) struct Style {
    pub(super) strong: bool,
    pub(super) emphasis: bool,
}

struct Run {
    bytes: Range<usize>,
    style: Style,
}

pub(in crate::definitions::syntax) struct HeadView<'a> {
    pub(in crate::definitions::syntax) text: String,
    runs: Vec<Run>,
    operands: &'a [NativeOperand],
    literal_starts: Vec<usize>,
    #[cfg(feature = "roff")]
    assignment_angles: std::cell::OnceCell<Vec<usize>>,
}

impl<'a> HeadView<'a> {
    pub(in crate::definitions::syntax) fn new(
        nodes: &[Inline],
        operands: &'a [NativeOperand],
    ) -> Self {
        let mut view = Self {
            text: String::new(),
            runs: Vec::new(),
            operands,
            literal_starts: Vec::new(),
            #[cfg(feature = "roff")]
            assignment_angles: std::cell::OnceCell::new(),
        };
        view.append(nodes, Style::default());
        // An authored leading blank belongs to the operand; its first name
        // starts later. Compute that point once instead of rescanning prefixes.
        view.literal_starts = operands
            .iter()
            .filter_map(|operand| {
                if operand.role.is_parameter() {
                    return None;
                }
                let text = view.text.get(operand.bytes.clone())?;
                let name = text.trim_start();
                (!name.is_empty()).then_some(operand.bytes.start + text.len() - name.len())
            })
            .collect();
        view
    }

    fn append(&mut self, nodes: &[Inline], style: Style) {
        for node in nodes {
            match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let start = self.text.len();
                    self.text.push_str(value);
                    self.runs.push(Run {
                        bytes: start..self.text.len(),
                        style,
                    });
                }
                Inline::Strong { children } => self.append(
                    children,
                    Style {
                        strong: true,
                        ..style
                    },
                ),
                Inline::Emphasis { children } => {
                    self.append(
                        children,
                        Style {
                            emphasis: true,
                            ..style
                        },
                    );
                }
                Inline::Link { children, .. } => self.append(children, style),
                Inline::LineBreak { .. } => {
                    let start = self.text.len();
                    self.text.push('\n');
                    self.runs.push(Run {
                        bytes: start..start + 1,
                        style,
                    });
                }
                Inline::Anchor { .. } => {}
            }
        }
    }

    pub(super) fn style(&self, offset: usize) -> Style {
        let index = self.runs.partition_point(|run| run.bytes.end <= offset);
        self.runs
            .get(index)
            .map_or(Style::default(), |run| run.style)
    }

    #[cfg(feature = "roff")]
    pub(in crate::definitions::syntax) fn emphasized_operand_start(
        &self,
        range: Range<usize>,
    ) -> Option<usize> {
        let first = self
            .runs
            .partition_point(|run| run.bytes.end <= range.start);
        self.runs[first..]
            .iter()
            .take_while(|run| run.bytes.start < range.end)
            .filter(|run| run.style.emphasis)
            .find_map(|run| {
                let start = run.bytes.start.max(range.start);
                let value = &self.text[start..run.bytes.end.min(range.end)];
                let trimmed = value.trim_start();
                (!trimmed.is_empty()).then_some(start + value.len() - trimmed.len())
            })
    }

    pub(super) fn operand(&self, offset: usize) -> Option<&NativeOperand> {
        operand_at(self.operands, offset)
    }

    pub(super) fn native_option_starts(&self, offset: usize) -> bool {
        self.explicit_option(offset) && self.literal_operand_starts(offset)
    }

    pub(super) fn skip_space(&self, offset: usize) -> usize {
        let tail = &self.text[offset..];
        offset + tail.len() - tail.trim_start().len()
    }

    #[cfg(feature = "roff")]
    pub(super) fn paired_assignment_angle(&self, offset: usize) -> bool {
        self.assignment_angles
            .get_or_init(|| assignment_angles::paired_offsets(self))
            .binary_search(&offset)
            .is_ok()
    }

    #[cfg(feature = "roff")]
    pub(super) fn literal_assignment_angle(
        &self,
        argument: &super::Argument,
        offset: usize,
        character: char,
    ) -> bool {
        matches!(character, '<' | '>' | '⟨' | '⟩')
            && self
                .operand(offset)
                .is_some_and(|operand| match operand.role {
                    NativeOperandRole::Placeholder { .. }
                    | NativeOperandRole::PathOperand(_)
                    | NativeOperandRole::LiteralValue
                    | NativeOperandRole::Command => true,
                    NativeOperandRole::ExplicitOption => {
                        let rhs_start = argument.bytes.start + 1;
                        argument.attached_to_name
                            && self.text[argument.bytes.start..].starts_with('=')
                            && offset >= rhs_start
                    }
                    _ => false,
                })
            && !self.paired_assignment_angle(offset)
    }

    pub(super) fn is_parameter(&self, offset: usize) -> bool {
        if let Some(operand) = self.operand(offset) {
            if operand.role.is_parameter() {
                return true;
            }
            if operand.role == NativeOperandRole::ExplicitOption {
                return false;
            }
        }
        let style = self.style(offset);
        style.emphasis && !style.strong
    }

    pub(super) fn literal_operand_starts(&self, offset: usize) -> bool {
        self.literal_starts.binary_search(&offset).is_ok()
    }

    pub(super) fn explicit_argument(&self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| operand.role.is_parameter())
    }

    pub(super) fn explicit_option(&self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| operand.role == NativeOperandRole::ExplicitOption)
    }

    pub(super) fn literal_punctuation(&self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| match operand.role {
                #[cfg(feature = "roff")]
                NativeOperandRole::LiteralPunctuation => true,
                _ => false,
            })
    }

    pub(super) fn permits_option_spelling(
        &self,
        span: Range<usize>,
        allow_literal_suffix: bool,
    ) -> bool {
        super::super::eligibility::permits_option_spelling(
            &self.text,
            self.operands,
            span,
            allow_literal_suffix,
        )
    }

    pub(super) fn reference_or_environment(&self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| match operand.role {
                #[cfg(feature = "roff")]
                NativeOperandRole::ReferenceText(_)
                | NativeOperandRole::PathOperand(_)
                | NativeOperandRole::Environment { .. }
                | NativeOperandRole::ClippedEnvironment { .. } => true,
                _ => false,
            })
    }

    pub(super) fn independent_option_suffix(&self, offset: usize) -> bool {
        self.reference_or_environment(offset)
            || self
                .operand(offset)
                .is_some_and(|operand| match operand.role {
                    #[cfg(feature = "roff")]
                    role if role.is_description() => {
                        !(self.style(offset).emphasis
                            || matches!(
                                role,
                                NativeOperandRole::Description | NativeOperandRole::LiteralValue
                            ) && self.text.as_bytes()[offset] == b'='
                                && self.text[operand.bytes.clone()].trim() == "=")
                    }
                    _ => false,
                })
    }

    #[cfg(feature = "roff")]
    pub(super) fn operands_in(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = &NativeOperand> + Clone {
        let start = self
            .operands
            .partition_point(|item| item.bytes.end <= range.start);
        self.operands[start..]
            .iter()
            .take_while(move |item| item.bytes.start < range.end)
    }
}
