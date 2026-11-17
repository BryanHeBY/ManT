//! A name grammar must preserve the uses of executed native operands.
use crate::definitions::{NativeOperand, RecognizedName};
use std::ops::Range;

#[cfg(feature = "roff")]
use crate::definitions::NativeOperandRole;

#[derive(Clone, Copy)]
pub(super) enum NameSource {
    Lexical,
    #[cfg(feature = "roff")]
    Operand(NativeOperandRole),
}

#[derive(Clone, Copy, Default)]
pub(super) struct OperandUses<'a> {
    operands: &'a [NativeOperand],
    offset: usize,
}

impl<'a> OperandUses<'a> {
    pub(super) fn new(operands: &'a [NativeOperand]) -> Self {
        Self {
            operands,
            offset: 0,
        }
    }

    #[cfg(feature = "roff")]
    pub(super) fn shifted(self, offset: usize) -> Self {
        Self {
            offset: self.offset + offset,
            ..self
        }
    }

    fn intersecting(self, part: &Range<usize>) -> impl Iterator<Item = &'a NativeOperand> {
        let end = self.offset + part.end;
        let start = self
            .operands
            .partition_point(|operand| operand.bytes.end <= self.offset + part.start);
        self.operands[start..]
            .iter()
            .take_while(move |operand| operand.bytes.start < end)
    }

    pub(super) fn permits(self, name: &RecognizedName, source: NameSource) -> bool {
        name.parts.iter().all(|part| match source {
            NameSource::Lexical => !self.protects(part),
            #[cfg(feature = "roff")]
            NameSource::Operand(role) => {
                if matches!(role, NativeOperandRole::ClippedSubject(..)) {
                    return false;
                }
                // An Ar label must fit one executed operand. Subject scopes
                // carry their own identity and can cover contiguous parts.
                let mut end = self.offset + part.start;
                for operand in self.intersecting(part) {
                    if operand.role != role || operand.bytes.start > end {
                        return false;
                    }
                    end = operand.bytes.end;
                    if end >= self.offset + part.end {
                        return true;
                    }
                    if role == NativeOperandRole::Placeholder {
                        return false;
                    }
                }
                false
            }
        })
    }

    pub(super) fn protects(self, part: &Range<usize>) -> bool {
        self.intersecting(part)
            .any(|operand| operand.role.is_parameter())
    }

    pub(super) fn protects_syntax(self, offset: usize) -> bool {
        self.protects(&(offset..offset + 1))
    }

    #[cfg(feature = "roff")]
    pub(super) fn is_parameter_only(self, part: &Range<usize>) -> bool {
        let mut cursor = self.offset + part.start;
        let end = self.offset + part.end;
        for operand in self.intersecting(part) {
            if operand.bytes.start > cursor
                || !matches!(
                    operand.role,
                    NativeOperandRole::SearchArgument(_) | NativeOperandRole::IncompletePlaceholder
                )
            {
                return false;
            }
            cursor = operand.bytes.end;
        }
        !part.is_empty() && cursor >= end
    }

    pub(super) fn has_parameters(self) -> bool {
        self.operands
            .iter()
            .any(|operand| operand.role.is_parameter())
    }
}
