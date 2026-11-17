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
use crate::mandoc::declaration_groups::search::SearchHead;
use crate::mandoc::roff_escape::{DecodedText, RoffInlineEvent};
use mant_ir::Inline;

#[derive(Clone, Default)]
pub(in crate::mandoc) struct HeadOperandCapture {
    search_head: Option<SearchHead>,
    roles: HashMap<u64, NativeOperandRole>,
    placeholder_receipts: Vec<(usize, u64)>,
    optional_keys: HashMap<u64, usize>,
    captured: CapturedHeadOperands,
    last_owner: Option<u64>,
    operand_role: Option<NativeOperandRole>,
}

impl HeadOperandCapture {
    pub(in crate::mandoc) fn new(search_head: Option<SearchHead>) -> Self {
        Self {
            search_head,
            ..Self::default()
        }
    }

    pub(super) fn record(&mut self, serial: u64, role: NativeOperandRole) {
        self.roles.insert(serial, role);
    }

    pub(super) fn mark_clipped_arguments(&mut self, owners: impl Iterator<Item = u64>) {
        for owner in owners {
            if let Some(role) = self.roles.get_mut(&owner)
                && *role == NativeOperandRole::Placeholder
            {
                *role = NativeOperandRole::IncompletePlaceholder;
            }
        }
    }

    pub(in crate::mandoc) fn take(&mut self) -> CapturedHeadOperands {
        let mut captured = std::mem::take(&mut self.captured);
        for (index, serial) in self.placeholder_receipts.drain(..) {
            captured.operands[index].role = self.roles[&serial];
        }
        self.optional_keys.clear();
        captured.qualify_optional_keys();
        captured
    }

    pub(in crate::mandoc) fn observe_accepted_output(&mut self, nodes: &[Inline]) {
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
        let Some((serial, mut role)) =
            owner.and_then(|serial| self.roles.get(&serial).map(|&role| (serial, role)))
        else {
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
            if role == NativeOperandRole::Placeholder {
                self.placeholder_receipts
                    .push((self.captured.operands.len(), serial));
            }
            // A single OP key can split at a formatter boundary. Its
            // surviving fragments cannot each introduce an independent key.
            if role == NativeOperandRole::OptionalKey
                && let Some(previous) = self
                    .optional_keys
                    .insert(serial, self.captured.operands.len())
            {
                role = NativeOperandRole::Argument;
                self.captured.operands[previous].role = role;
                self.roles.insert(serial, role);
            }
            self.captured.operands.push(NativeOperand {
                bytes: start..end,
                role,
            });
        }
        self.last_owner = owner;
    }
}

impl super::InlineBuilder {
    pub(in crate::mandoc) fn mdoc_argument_role(&self, node: u32) -> NativeOperandRole {
        if self.head_operand_capture.as_ref().is_some_and(|capture| {
            capture
                .borrow()
                .search_head
                .as_ref()
                .is_some_and(|form| form.has_argument(node))
        }) {
            NativeOperandRole::SearchArgument(node)
        } else {
            NativeOperandRole::Placeholder
        }
    }

    /// Register an actual word before parsing or accepted-output drains.
    pub(in crate::mandoc) fn record_current_native_operand(
        &mut self,
        text: Option<(u32, &DecodedText)>,
    ) {
        if let Some(capture) = &self.head_operand_capture {
            let mut capture = capture.borrow_mut();
            let role = capture
                .operand_role
                .or_else(|| capture.search_head.as_ref()?.role(text?.0))
                .or_else(|| {
                    text?
                        .1
                        .events
                        .iter()
                        .any(|event| matches!(event, RoffInlineEvent::Font(_)))
                        .then_some(NativeOperandRole::Literal)
                });
            if let Some(role) = role {
                let role = if text.is_some_and(|(_, decoded)| !decoded.proves_native_glyphs()) {
                    match role {
                        NativeOperandRole::Placeholder => NativeOperandRole::IncompletePlaceholder,
                        NativeOperandRole::SearchArgument(_) => NativeOperandRole::Argument,
                        _ => role,
                    }
                } else {
                    role
                };
                capture.record(self.execution.native_owner_serial, role);
            }
        }
    }

    pub(super) fn record_clipped_operands(&mut self) {
        if let Some(capture) = &self.head_operand_capture
            && let Some(owners) = self.execution.zero_advance.take_overwritten_word_owners()
        {
            capture.borrow_mut().mark_clipped_arguments(owners);
        }
    }

    pub(super) fn detach_pending_operand(&self) {
        if let Some(capture) = &self.head_operand_capture
            && let Some(owner) = self.execution.zero_advance.pending_word_owner()
        {
            // The buffered glyph follows the live execution. The detached
            // HEAD's accepted residue cannot certify the complete label.
            capture
                .borrow_mut()
                .mark_clipped_arguments(std::iter::once(owner));
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
