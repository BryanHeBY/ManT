//! Preserve actual native operand identity from accepted word receipts.
//!
//! CVS `man_term.c::pre_alternate()` calls `term_word()` once for each TEXT
//! child. Font escapes inside that child never create another operand.
//! `mdoc_term.c::termp_fl_pre()` emits its own dash word before its children;
//! this explicit option role applies only inside that actual macro scope.
//! Existing private word owners already follow field clipping and delayed
//! glyph retirement; this HEAD-only sidecar reads those accepted owners.

use std::collections::{HashMap, HashSet};

use crate::definitions::{CapturedHeadOperands, NativeOperand, NativeOperandRole};
use crate::mandoc::declaration_groups::search::SearchHead;
use crate::mandoc::roff_escape::{DecodedText, RoffInlineEvent};
use libmandoc_rs::Node;
use mant_ir::Inline;

mod body_spans;
mod spelling;

use spelling::{
    FlSpelling, literal_declaration_role, native_spelling_role, standalone_field_operator,
};

#[derive(Clone, Default)]
pub(in crate::mandoc) struct HeadOperandCapture {
    search_head: Option<SearchHead>,
    roles: HashMap<u64, NativeOperandRole>,
    replaces_native_ink: HashSet<u64>,
    projected_operands: Vec<ProjectedOperand>,
    unsplit_receipts: HashMap<u64, usize>,
    fl_spellings: HashMap<u64, FlSpelling>,
    captured: CapturedHeadOperands,
    last_owner: Option<u64>,
    option_dash: Option<u64>,
    bodies: body_spans::AcceptedBodies,
    operand_role: Option<NativeOperandRole>,
    enclosures: Vec<u32>,
    has_environment: bool,
    has_option: bool,
}

/// Accepted output can be drained before a later word's paint pass is
/// finalized. Retain its native owner independently of its grammar role.
#[derive(Clone)]
struct ProjectedOperand {
    owner: u64,
    tracks_spelling: bool,
}

impl HeadOperandCapture {
    pub(in crate::mandoc) fn new(search_head: Option<SearchHead>, has_environment: bool) -> Self {
        Self {
            search_head,
            has_environment,
            ..Self::default()
        }
    }

    pub(super) fn record(&mut self, serial: u64, role: NativeOperandRole) {
        self.has_environment |= matches!(role, NativeOperandRole::Environment { .. });
        self.has_option |= matches!(
            role,
            NativeOperandRole::ExplicitOption
                | NativeOperandRole::FlDash(_)
                | NativeOperandRole::FlWord(_)
        );
        self.roles.insert(serial, role);
    }

    fn mark_clipped_operands(&mut self, owners: impl Iterator<Item = u64>, writer: Option<u64>) {
        for owner in owners {
            // The execution receipt proves displaced ink, even if the
            // erased word had no recognized role (e.g. ordinary No text).
            // A subsequent overwrite keeps the same proof on its writer.
            if let Some(writer) = writer.filter(|&writer| writer != owner) {
                self.replaces_native_ink.insert(writer);
            }
            if let Some(role) = self.roles.get_mut(&owner) {
                *role = match *role {
                    NativeOperandRole::Placeholder { .. } => {
                        NativeOperandRole::IncompletePlaceholder
                    }
                    NativeOperandRole::FlDash(_) => NativeOperandRole::Argument,
                    NativeOperandRole::FlWord(_)
                        if !matches!(
                            self.fl_spellings.get(&owner),
                            Some(FlSpelling::Assignment(_))
                        ) =>
                    {
                        NativeOperandRole::ClippedOption
                    }
                    // CVS term_word() can intentionally write the displayed
                    // subject spelling through an internal overstrike. A
                    // different word overwriting its glyph clips its operand.
                    NativeOperandRole::Subject(..) | NativeOperandRole::Environment { .. }
                        if writer == Some(owner) =>
                    {
                        *role
                    }
                    role => role.without_complete_spelling(),
                };
            }
        }
    }

    pub(super) fn record_rewound_ink<'a>(
        &mut self,
        clipped: impl Iterator<Item = &'a str>,
        writer: &str,
    ) {
        self.mark_clipped_operands(
            clipped
                .filter_map(|owner| owner.strip_prefix(super::INTERNAL_FIELD_WORD)?.parse().ok()),
            writer
                .strip_prefix(super::INTERNAL_FIELD_WORD)
                .and_then(|serial| serial.parse().ok()),
        );
    }

    pub(super) fn record_accepted_covers(
        &mut self,
        buffer: &super::field_buffer::FieldBuffer,
        anchors: &[super::NativeWordAnchor],
        passes: &[super::field_buffer::FillPass],
    ) {
        let word_at = |cell| {
            let index = anchors
                .partition_point(|anchor| anchor.content <= cell)
                .checked_sub(1)?;
            anchors[index]
                .owner
                .strip_prefix(super::INTERNAL_FIELD_WORD)?
                .parse()
                .ok()
        };
        // Unlike immediate BACKBEFORE projection, these glyphs remained
        // available for later writes. Only the accepted pass proves clipping.
        buffer.visit_accepted_covers(passes, |target, by| {
            if buffer.has_projected_ink_at(target)
                && let Some(owner) = word_at(target)
            {
                let writer = by
                    .map(|cell| buffer.native_ink_owner_cell(cell))
                    .and_then(word_at);
                self.mark_clipped_operands(std::iter::once(owner), writer);
            }
        });
    }

    pub(in crate::mandoc) fn take(&mut self) -> CapturedHeadOperands {
        let mut captured = std::mem::take(&mut self.captured);
        let mut receipts = self.projected_operands.drain(..);
        captured.operands.retain_mut(|operand| {
            let receipt = receipts.next().expect("projected operand owner");
            let owner = receipt.owner;
            operand.replaces_native_ink = self.replaces_native_ink.contains(&owner);
            let Some(&role) = self.roles.get(&owner) else {
                // Unclassified words only supply a receipt if they are
                // the visible writer of covered author's ink.
                return operand.replaces_native_ink;
            };
            if receipt.tracks_spelling {
                // termp_fl_pre's generated dash is independent of these
                // glyphs. Validate the surviving spelling after all covers.
                operand.role = if matches!(role, NativeOperandRole::FlWord(_))
                    && self.fl_spellings.get(&owner).is_some_and(|spelling| {
                        !spelling.survived(captured.text[operand.bytes.clone()].trim())
                    }) {
                    NativeOperandRole::ClippedOption
                } else {
                    role
                };
            }
            true
        });
        debug_assert!(receipts.next().is_none());
        self.unsplit_receipts.clear();
        captured.qualify_optional_keys();
        captured.bodies = self.bodies.take();
        captured.qualify_fl_keys(self.search_head.is_some());
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
        let Some(serial) = owner else {
            self.last_owner = owner;
            return;
        };
        let mut role = self
            .roles
            .get(&serial)
            .copied()
            .unwrap_or(NativeOperandRole::Argument);
        if text.is_empty() {
            return;
        }
        let end = self.captured.text.len();
        self.bodies.observe(start..end, serial, &self.captured.text);
        if let Some(previous) = self.captured.operands.last_mut()
            && previous.bytes.end == start
            && previous.role == role
            && self.last_owner == owner
        {
            previous.bytes.end = end;
        } else {
            self.projected_operands.push(ProjectedOperand {
                owner: serial,
                tracks_spelling: matches!(
                    role,
                    NativeOperandRole::Placeholder { .. }
                        | NativeOperandRole::FlDash(_)
                        | NativeOperandRole::FlWord(_)
                        | NativeOperandRole::Subject(..)
                        | NativeOperandRole::Environment { .. }
                        | NativeOperandRole::LiteralValue
                        | NativeOperandRole::SymbolicText
                        | NativeOperandRole::PathOperand(_)
                        | NativeOperandRole::LiteralContinuation(_)
                        | NativeOperandRole::Command
                        | NativeOperandRole::ProgramName(_)
                        | NativeOperandRole::FunctionSignature { .. }
                        | NativeOperandRole::EnclosureStart { .. }
                        | NativeOperandRole::EnclosureEnd(_)
                ),
            });
            // A split projected word cannot prove a complete option key or
            // enclosure delimiter, even when all its glyphs survived.
            if matches!(
                role,
                NativeOperandRole::OptionalKey
                    | NativeOperandRole::FlDash(_)
                    | NativeOperandRole::FlWord(_)
                    | NativeOperandRole::EnclosureStart { .. }
                    | NativeOperandRole::EnclosureEnd(_)
            ) && let Some(previous) = self
                .unsplit_receipts
                .insert(serial, self.captured.operands.len())
            {
                role = match role {
                    NativeOperandRole::EnclosureStart { .. }
                    | NativeOperandRole::EnclosureEnd(_) => role.without_complete_spelling(),
                    _ => NativeOperandRole::Argument,
                };
                self.captured.operands[previous].role = role;
                self.roles.insert(serial, role);
            }
            self.captured.operands.push(NativeOperand {
                replaces_native_ink: false,
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
            NativeOperandRole::Placeholder {
                node,
                enclosure: None,
            }
        }
    }

    /// Register an actual word before parsing or accepted-output drains.
    pub(in crate::mandoc) fn record_current_native_operand(
        &mut self,
        text: Option<(&Node, &DecodedText)>,
    ) {
        if let Some(capture) = &self.head_operand_capture {
            let mut capture = capture.borrow_mut();
            let serial = self.execution.native_owner_serial;
            let enclosure = || {
                capture
                    .enclosures
                    .iter()
                    .rev()
                    .find(|&&scope| !self.scope_posts.ended(scope))
                    .copied()
            };
            let role = capture
                .operand_role
                .filter(|role| {
                    !role.is_description()
                        && !matches!(role, NativeOperandRole::LiteralContinuation(_))
                })
                .or_else(|| capture.search_head.as_ref()?.role(text?.0.id))
                .or_else(|| {
                    let glyph = literal_delimiter(text?.0)?;
                    Some(NativeOperandRole::LiteralDelimiter {
                        glyph,
                        enclosure: enclosure(),
                    })
                })
                .or_else(|| {
                    // mdoc_macro.c::in_line can allocate an empty Fl when
                    // the argument is closing punctuation, then execute
                    // its following TEXT as a separate native word.
                    let node = text?.0;
                    (capture
                        .option_dash
                        .is_some_and(|dash| dash.wrapping_add(1) == serial)
                        && node.flags.delimiter_close
                        && matches!(node.decoder_text(), Some("," | "." | ";" | ":" | "?" | "!")))
                    .then_some(NativeOperandRole::OptionPunctuation)
                })
                .or_else(|| {
                    // in_line() closes Fl before these sibling delimiters.
                    // They also belong to the lexical option alphabet.
                    let node = text?.0;
                    (capture.has_option
                        && node.flags.delimiter_close
                        && matches!(node.decoder_text(), Some("?" | "." | r"\.")))
                    .then_some(NativeOperandRole::OptionClosingPunctuation)
                })
                .or_else(|| {
                    let role = capture.operand_role?;
                    if matches!(role, NativeOperandRole::LiteralContinuation(_)) {
                        return text
                            .and_then(|(_, decoded)| literal_declaration_role(decoded))
                            .or(capture.has_environment.then_some(role));
                    }
                    Some(role)
                })
                .or_else(|| {
                    (capture.has_option
                        && capture.operand_role.is_none()
                        && standalone_field_operator(text?.1))
                    .then_some(NativeOperandRole::LiteralContinuation(None))
                })
                .or_else(|| {
                    captured_bare_delimiter(
                        text.and_then(|(node, _)| node.decoder_text()),
                        enclosure(),
                        !matches!(
                            capture.operand_role,
                            Some(NativeOperandRole::LiteralContinuation(_))
                        ),
                    )
                })
                .or_else(|| {
                    text?
                        .1
                        .events
                        .iter()
                        .any(|event| matches!(event, RoffInlineEvent::Font(_)))
                        .then_some(NativeOperandRole::Literal)
                });
            if let Some(role) = role {
                let role = role.record_enclosure(enclosure);
                let outermost = capture
                    .enclosures
                    .iter()
                    .find(|&&scope| !self.scope_posts.ended(scope))
                    .copied();
                if let Some(scope) = outermost {
                    capture.bodies.record(serial, scope);
                }
                if matches!(role, NativeOperandRole::FlDash(_)) {
                    capture.option_dash = Some(serial);
                }
                let role = native_spelling_role(
                    role,
                    text.map(|(_, decoded)| decoded),
                    capture.has_option,
                    || self.execution.font.clone(),
                );
                if matches!(role, NativeOperandRole::FlWord(_))
                    && let Some((_, decoded)) = text
                    && let Some(spelling) = FlSpelling::from_events(&decoded.events)
                {
                    capture.fl_spellings.insert(serial, spelling);
                }
                capture.record(serial, role);
            }
        }
    }

    pub(super) fn record_clipped_operands(&mut self) {
        let writer = self.execution.native_owner_serial;
        if let Some(capture) = &self.head_operand_capture {
            let mut capture = capture.borrow_mut();
            if let Some(owner) = self.execution.zero_advance.take_revised_authored_ink() {
                capture.replaces_native_ink.insert(owner);
            }
            if let Some(owners) = self.execution.zero_advance.take_overwritten_word_owners() {
                capture.mark_clipped_operands(owners, Some(writer));
            }
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
                .mark_clipped_operands(std::iter::once(owner), None);
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

    pub(in crate::mandoc) fn with_declaration_operand(
        &mut self,
        role: NativeOperandRole,
        operation: impl FnOnce(&mut Self),
    ) {
        let has_declaration = self.head_operand_capture.as_ref().is_some_and(|capture| {
            let capture = capture.borrow();
            capture.has_environment
                || capture.has_option && role == NativeOperandRole::Description
                || role == NativeOperandRole::Command
        });
        if has_declaration {
            self.with_native_operand_role(role, operation);
        } else {
            operation(self);
        }
    }

    pub(in crate::mandoc) fn with_native_enclosure(
        &mut self,
        scope: u32,
        operation: impl FnOnce(&mut Self),
    ) {
        let capture = self.head_operand_capture.clone();
        if let Some(capture) = &capture {
            capture.borrow_mut().enclosures.push(scope);
        }
        operation(self);
        if let Some(capture) = capture {
            capture.borrow_mut().enclosures.pop();
        }
    }
}

fn literal_delimiter(node: &Node) -> Option<char> {
    // mdoc_macro.c::dword sets NODE_DELIMO/C for bare delimiters.
    // A glyph within a styled word is not such a native boundary.
    match node.decoder_text()? {
        "(" if node.flags.delimiter_open => Some('('),
        "[" if node.flags.delimiter_open => Some('['),
        ")" if node.flags.delimiter_close => Some(')'),
        "]" if node.flags.delimiter_close => Some(']'),
        _ => None,
    }
}

fn captured_bare_delimiter(
    text: Option<&str>,
    enclosure: Option<u32>,
    bare: bool,
) -> Option<NativeOperandRole> {
    // in_line() rewinds its element at a middle delimiter and emits a
    // sibling TEXT. In particular that pipe was not an explicit No operand.
    // mdoc.c::mdoc_isdelim recognizes both spellings as DELIM_MIDDLE.
    if matches!(text, Some("|" | r"\fR|\fP")) {
        Some(NativeOperandRole::PipeDelimiter(enclosure))
    } else {
        enclosure
            .map(|body| NativeOperandRole::EnclosureContent { body, bare })
            .or((text == Some(",")).then_some(NativeOperandRole::LiteralContinuation(None)))
    }
}
