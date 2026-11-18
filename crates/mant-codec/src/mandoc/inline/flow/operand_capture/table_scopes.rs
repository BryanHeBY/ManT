//! Display rows inherit the active native BODY even when its marks are invisible.
use crate::definitions::{NativeOperand, NativeOperandRole};
use std::{collections::HashMap, ops::Range};

#[derive(Clone, Default)]
pub(super) struct AcceptedTableScopes {
    word_bodies: HashMap<u64, u32>,
    runs: Vec<ScopeRun>,
}

#[derive(Clone)]
struct ScopeRun {
    body: u32,
    bytes: Range<usize>,
    has_table: bool,
}

impl AcceptedTableScopes {
    pub(super) fn record(&mut self, serial: u64, body: u32) {
        self.word_bodies.insert(serial, body);
    }

    pub(super) fn observe(&mut self, bytes: Range<usize>, serial: u64, display: bool, text: &str) {
        let Some(&body) = self.word_bodies.get(&serial) else {
            return;
        };
        if let Some(last) = self.runs.last_mut()
            && last.body == body
            && text[last.bytes.end..bytes.start].trim().is_empty()
        {
            last.bytes.end = bytes.end;
            last.has_table |= display;
        } else {
            self.runs.push(ScopeRun {
                body,
                bytes,
                has_table: display,
            });
        }
    }

    pub(super) fn restrict(&mut self, operands: &mut Vec<NativeOperand>) {
        let mut scopes = self.runs.drain(..).filter(|run| run.has_table).peekable();
        if scopes.peek().is_none() {
            return;
        }
        let mut recorded = std::mem::take(operands).into_iter().peekable();
        for scope in scopes {
            while let Some(operand) =
                recorded.next_if(|operand| operand.bytes.end <= scope.bytes.start)
            {
                operands.push(operand);
            }
            while recorded
                .next_if(|operand| operand.bytes.start < scope.bytes.end)
                .is_some()
            {}
            operands.push(NativeOperand {
                bytes: scope.bytes,
                role: NativeOperandRole::DisplayEnclosure,
            });
        }
        operands.extend(recorded);
    }
}
