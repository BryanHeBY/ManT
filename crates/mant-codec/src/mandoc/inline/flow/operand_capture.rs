//! Preserve actual native operand identity from accepted word receipts.
//!
//! CVS `man_term.c::pre_alternate()` calls `term_word()` once for each TEXT
//! child. Font escapes inside that child never create another operand.
//! `mdoc_term.c::termp_fl_pre()` emits its own dash word before its children;
//! this explicit option role applies only inside that actual macro scope.
//! Existing private word owners already follow field clipping and delayed
//! glyph retirement; this HEAD-only sidecar reads those accepted owners.

use std::collections::HashMap;

use crate::definitions::{CapturedHeadOperands, NativeOperand, NativeOperandRole};
use mant_ir::Inline;

#[derive(Default)]
pub(in crate::mandoc) struct HeadOperandCapture {
    roles: HashMap<u64, NativeOperandRole>,
    captured: CapturedHeadOperands,
    last_owner: Option<u64>,
    operand_role: Option<NativeOperandRole>,
}

impl HeadOperandCapture {
    pub(super) fn record(&mut self, serial: u64, role: NativeOperandRole) {
        self.roles.insert(serial, role);
    }

    pub(in crate::mandoc) fn take(&mut self) -> CapturedHeadOperands {
        std::mem::take(&mut self.captured)
    }

    pub(super) fn observe_accepted_output(&mut self, nodes: &[Inline]) {
        if self.roles.is_empty() {
            return;
        }
        let mut owner = None;
        self.visit(nodes, &mut owner);
    }

    fn visit(&mut self, nodes: &[Inline], owner: &mut Option<u64>) {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } => {
                    if let Some(serial) = id.as_str().strip_prefix(super::INTERNAL_FIELD_WORD) {
                        *owner = serial.parse().ok();
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.visit(children, owner),
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => self.append(value, *owner),
                Inline::LineBreak { .. } => self.append("\n", *owner),
            }
        }
    }

    fn append(&mut self, text: &str, owner: Option<u64>) {
        let start = self.captured.text.len();
        self.captured.text.push_str(text);
        let Some(role) = owner.and_then(|serial| self.roles.get(&serial)).copied() else {
            self.last_owner = owner;
            return;
        };
        if text.is_empty() {
            return;
        }
        let end = self.captured.text.len();
        if let Some(previous) = self.captured.operands.last_mut()
            && previous.bytes.end == start
            && previous.role == role
            && self.last_owner == owner
        {
            previous.bytes.end = end;
        } else {
            self.captured.operands.push(NativeOperand {
                bytes: start..end,
                role,
            });
        }
        self.last_owner = owner;
    }
}

impl super::InlineBuilder {
    /// Register an actual word before parsing or accepted-output drains.
    pub(in crate::mandoc) fn record_current_native_operand(&mut self, has_font_escape: bool) {
        if let Some(capture) = &self.head_operand_capture {
            let mut capture = capture.borrow_mut();
            let role = capture
                .operand_role
                .or_else(|| has_font_escape.then_some(NativeOperandRole::Literal));
            if let Some(role) = role {
                capture.record(self.execution.native_owner_serial, role);
            }
        }
    }

    pub(in crate::mandoc) fn with_native_operand_role(
        &mut self,
        role: NativeOperandRole,
        operation: impl FnOnce(&mut Self),
    ) {
        let capture = self.head_operand_capture.clone();
        let previous = capture
            .as_ref()
            .and_then(|capture| capture.borrow_mut().operand_role.replace(role));
        operation(self);
        if let Some(capture) = capture {
            capture.borrow_mut().operand_role = previous;
        }
    }
}
