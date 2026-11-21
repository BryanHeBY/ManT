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
use crate::mandoc::roff_escape::{DecodedText, PresentationKind, RoffInlineEvent};
use libmandoc_rs::Node;
use mant_ir::Inline;

mod body_spans;

#[derive(Clone, Default)]
pub(in crate::mandoc) struct HeadOperandCapture {
    search_head: Option<SearchHead>,
    roles: HashMap<u64, NativeOperandRole>,
    spelling_receipts: Vec<(usize, u64)>,
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
            if buffer.projection_length(target, target + 1) != 0
                && let Some(owner) = word_at(target)
            {
                self.mark_clipped_operands(std::iter::once(owner), by.and_then(word_at));
            }
        });
    }

    pub(in crate::mandoc) fn take(&mut self) -> CapturedHeadOperands {
        let mut captured = std::mem::take(&mut self.captured);
        for (index, serial) in self.spelling_receipts.drain(..) {
            let operand = &mut captured.operands[index];
            let role = self.roles[&serial];
            // termp_fl_pre's generated dash is independent of these source
            // glyphs. Later writes can cover only part of a word during
            // term_field(), so validate the surviving source owner here.
            operand.role = if matches!(role, NativeOperandRole::FlWord(_))
                && self.fl_spellings.get(&serial).is_some_and(|spelling| {
                    !spelling.survived(captured.text[operand.bytes.clone()].trim())
                }) {
                NativeOperandRole::ClippedOption
            } else {
                role
            };
        }
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
        self.bodies.observe(start..end, serial, &self.captured.text);
        if let Some(previous) = self.captured.operands.last_mut()
            && previous.bytes.end == start
            && previous.role == role
            && self.last_owner == owner
        {
            previous.bytes.end = end;
        } else {
            if matches!(
                role,
                NativeOperandRole::Placeholder { .. }
                    | NativeOperandRole::FlDash(_)
                    | NativeOperandRole::FlWord(_)
                    | NativeOperandRole::Subject(..)
                    | NativeOperandRole::Environment { .. }
                    | NativeOperandRole::LiteralContinuation(_)
                    | NativeOperandRole::Command
                    | NativeOperandRole::ProgramName(_)
                    | NativeOperandRole::FunctionSignature { .. }
                    | NativeOperandRole::EnclosureStart { .. }
                    | NativeOperandRole::EnclosureEnd(_)
            ) {
                self.spelling_receipts
                    .push((self.captured.operands.len(), serial));
            }
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
                    let role = capture.operand_role?;
                    if matches!(role, NativeOperandRole::LiteralContinuation(_)) {
                        return text
                            .and_then(|(_, decoded)| literal_declaration_role(decoded))
                            .or(capture.has_environment.then_some(role));
                    }
                    Some(role)
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
                let role = if text.is_some_and(|(_, decoded)| !decoded.proves_native_glyphs()) {
                    match role {
                        NativeOperandRole::Placeholder { .. } => {
                            NativeOperandRole::IncompletePlaceholder
                        }
                        NativeOperandRole::SearchArgument(_) | NativeOperandRole::FlWord(_) => {
                            NativeOperandRole::Argument
                        }
                        _ => role.without_complete_spelling(),
                    }
                } else {
                    role
                };
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
        if let Some(capture) = &self.head_operand_capture
            && let Some(owners) = self.execution.zero_advance.take_overwritten_word_owners()
        {
            capture
                .borrow_mut()
                .mark_clipped_operands(owners, Some(writer));
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

fn declaration_separator(decoded: &DecodedText) -> bool {
    // CVS term_word() writes a zero-width cell for the ESCAPE_IGNORE
    // produced by roff_escape.c for color and point-size escapes. Those
    // cells and font controls do not change one No operand's spelling.
    let mut chars = decoded
        .events
        .iter()
        .flat_map(|event| match event {
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Presentation {
                kind: PresentationKind::Color | PresentationKind::PointSize,
                ..
            } => "".chars(),
            RoffInlineEvent::Text(word) | RoffInlineEvent::Glyph(word) => word.chars(),
            _ => "\0".chars(),
        })
        .skip_while(|glyph| glyph.is_whitespace());
    let separator = match chars.next() {
        Some(',' | '|') => true,
        Some('o') => chars.next() == Some('r'),
        _ => false,
    };
    separator && chars.all(char::is_whitespace)
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

fn literal_declaration_role(decoded: &DecodedText) -> Option<NativeOperandRole> {
    if declaration_separator(decoded) {
        Some(NativeOperandRole::LiteralContinuation(None))
    } else {
        decoded.events.iter().any(|event| matches!(
            event, RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value) if value.contains([',', '|'])
        )).then_some(NativeOperandRole::LiteralPunctuation)
    }
}

#[derive(Clone)]
enum FlSpelling {
    KeyBytes(usize),
    Assignment(String),
}

impl FlSpelling {
    fn from_events(events: &[RoffInlineEvent]) -> Option<Self> {
        if let Some(prefix) = assignment_prefix(events) {
            return Some(Self::Assignment(prefix));
        }
        // Ordinary Fl keys have one projected byte sequence even across
        // font changes. Accepting fewer bytes from this unique owner
        // cannot prove the original spelling. Cursor-editing operands use
        // the execution's existing native clipping receipts instead.
        let bytes = events.iter().try_fold(0, |bytes, event| match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value)
                if !value.contains(char::is_whitespace) =>
            {
                Some(bytes + value.len())
            }
            RoffInlineEvent::BreakableHyphen => Some(bytes + 1),
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroWidthGlyph => Some(bytes),
            _ => None,
        })?;
        (bytes > 0).then_some(Self::KeyBytes(bytes))
    }

    fn survived(&self, value: &str) -> bool {
        match self {
            Self::KeyBytes(bytes) => value.len() == *bytes,
            // A native overstrike in an attached value need not invalidate
            // the untouched key and assignment separator.
            Self::Assignment(prefix) => value.starts_with(prefix),
        }
    }
}

fn assignment_prefix(events: &[RoffInlineEvent]) -> Option<String> {
    if !events.iter().any(|event| matches!(event, RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value) if value.contains('='))) {
        return None;
    }
    let mut prefix = String::new();
    for event in events {
        match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value) => {
                if let Some(index) = value.find('=') {
                    prefix.push_str(&value[..=index]);
                    return (prefix.len() > 1).then_some(prefix);
                }
                prefix.push_str(value);
            }
            RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => {}
            _ => return None,
        }
    }
    None
}
