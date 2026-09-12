//! Filled flow preserves font, spacing and pending boundaries across scopes.
use super::{Font, ZeroAdvanceState, needs_boundary_space, push_text, updated_spacing};
use mant_ir::Inline;
use mant_ir::{first_visible_character, has_printable_character, last_visible_character};

pub(in crate::mandoc) struct InlineBuilder {
    nodes: Vec<Inline>,
    boundary: PendingBoundary,
    spacing: SpacingMode,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    empty_word: bool,
    pending_word_spaces: usize,
    pub(in crate::mandoc) font: FontState,
    pub(in crate::mandoc) zero_advance: ZeroAdvanceState,
    // A nested inline scope can resolve a `\\z` glyph that was armed by its
    // parent.  Preserve that boundary fact when the scope returns its nodes:
    // otherwise the parent would invent a word separator before the
    // overwriting glyph.
    zero_advance_joined: bool,
    source_cursor: Option<super::source_cursor::SourceCursor>,
    // This is deliberately separate from `PendingBoundary`: a formatter
    // request such as `Ns` can tighten a boundary without extending the
    // physical source line. ParagraphFlow needs the latter fact only after a
    // complete inline subtree has executed.
    source_line_continuation: Option<bool>,
}

/// A checkpoint for output that may be semantically annotated or discarded
/// after it has executed. Formatter state deliberately remains live: a
/// hidden operand can select a font or consume a `\\z` glyph even when its
/// projected characters are not retained in Mant's compact presentation.
#[derive(Clone)]
pub(in crate::mandoc) struct OutputCheckpoint {
    node_count: usize,
    boundary: PendingBoundary,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    empty_word: bool,
    pending_word_spaces: usize,
    source_cursor: Option<super::source_cursor::SourceCursor>,
}

/// Roff remembers the previous selection independently of the current font.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct FontState {
    pub(super) current: Font,
    pub(super) previous: Font,
}

impl FontState {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            current: Font::Regular,
            previous: Font::Regular,
        }
    }

    pub(super) fn select(&mut self, font: Font) {
        self.previous = self.current;
        self.current = font;
    }

    pub(super) fn restore(&mut self) {
        std::mem::swap(&mut self.current, &mut self.previous);
    }

    /// mdoc font scopes push a selection. Popping restores the saved current
    /// font, not the previous-selection register used by `\\fP` and `.ft P`.
    pub(in crate::mandoc) fn push_scope(&mut self, font: Font) -> Font {
        let saved = self.current;
        self.select(font);
        saved
    }

    pub(in crate::mandoc) fn pop_scope(&mut self, saved: Font) {
        self.current = saved;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingBoundary {
    Ordinary,
    Tight,
    PrefixJoin,
    Preserved,
}

impl PendingBoundary {
    const fn is_tight(self) -> bool {
        matches!(self, Self::Tight | Self::PrefixJoin)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpacingMode {
    Enabled,
    Disabled,
}

impl SpacingMode {
    const fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    const fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

impl From<bool> for SpacingMode {
    fn from(enabled: bool) -> Self {
        Self::from_enabled(enabled)
    }
}

/// Semantic boundary between two inline fragments in filled roff mode.
///
/// Roff distinguishes ordinary source wrapping from an input line whose first
/// text character is whitespace.  The former fills as a word boundary; the
/// latter starts a new output line.  Keeping that distinction here prevents
/// renderers from having to rediscover formatter semantics from flattened
/// text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum FilledBoundary {
    SameLine,
    Word,
    LineBreak,
}

impl InlineBuilder {
    #[cfg(test)]
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::Enabled,
            last_visible_character: None,
            has_printable_content: false,
            empty_word: false,
            pending_word_spaces: 0,
            font: FontState::new(),
            zero_advance: ZeroAdvanceState::new(),
            zero_advance_joined: false,
            source_cursor: None,
            source_line_continuation: None,
        }
    }

    pub(in crate::mandoc) const fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            nodes: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::from_enabled(spacing_enabled),
            last_visible_character: None,
            has_printable_content: false,
            empty_word: false,
            pending_word_spaces: 0,
            font: FontState::new(),
            zero_advance: ZeroAdvanceState::new(),
            zero_advance_joined: false,
            source_cursor: None,
            source_line_continuation: None,
        }
    }

    pub(in crate::mandoc) fn tighten_next_boundary(&mut self) {
        self.boundary = PendingBoundary::Tight;
    }

    pub(in crate::mandoc) fn note_zero_advance_join(&mut self) {
        self.zero_advance_joined = true;
    }

    pub(in crate::mandoc) fn track_executed_lines(&mut self) {
        self.source_cursor = Some(super::source_cursor::SourceCursor::new());
    }

    pub(in crate::mandoc) fn begin_executed_node(&mut self, node: &libmandoc_rs::Node) {
        if node.flags.line_start
            && let Some(cursor) = &mut self.source_cursor
        {
            cursor.begin();
        }
    }

    /// Start a top-level source subtree. Its final formatter word, rather
    /// than the AST's source order, determines whether the next physical line
    /// is joined. This matters for mdoc `.Lk`, whose renderer executes the
    /// descriptive label before the URI stored as its first child.
    pub(in crate::mandoc) fn begin_source_fragment(&mut self) {
        self.source_line_continuation = None;
    }

    pub(in crate::mandoc) fn source_line_continues_or(&self, fallback: bool) -> bool {
        self.source_line_continuation.unwrap_or(fallback)
    }

    pub(in crate::mandoc) fn continue_source_line(&mut self, continued: bool) {
        if let Some(cursor) = &mut self.source_cursor {
            cursor.continue_line(continued);
        }
        self.source_line_continuation = Some(continued);
    }

    pub(in crate::mandoc) fn transfer_source_cursor(&mut self, next: &mut Self) {
        next.source_cursor = self.source_cursor.take();
    }

    pub(in crate::mandoc) fn reset_source_cursor(&mut self) {
        if let Some(cursor) = &mut self.source_cursor {
            cursor.reset();
        }
    }

    pub(in crate::mandoc) fn release_next_boundary(&mut self) {
        self.boundary = PendingBoundary::Ordinary;
    }

    /// Source wrapping is a word boundary even while macro auto-spacing is
    /// disabled. Explicit joins still take precedence over ordinary wrapping.
    pub(in crate::mandoc) fn preserve_source_word_boundary(&mut self) {
        if !self.boundary.is_tight() {
            self.boundary = PendingBoundary::Preserved;
        }
    }

    /// Join a generated prefix only to its own operand scope. Word events
    /// consume it even without glyphs; anchors do not. Expire unused joins.
    /// An explicit control replaces `PrefixJoin` with `Tight` and must survive.
    pub(super) fn with_prefix_join(&mut self, append: impl FnOnce(&mut Self)) {
        self.boundary = PendingBoundary::PrefixJoin;
        append(self);
        if self.boundary == PendingBoundary::PrefixJoin {
            self.boundary = PendingBoundary::Ordinary;
        }
    }

    pub(in crate::mandoc) const fn has_tight_boundary(&self) -> bool {
        self.boundary.is_tight()
    }

    pub(in crate::mandoc) const fn spacing_enabled(&self) -> bool {
        self.spacing.enabled()
    }

    pub(in crate::mandoc) fn set_spacing(&mut self, setting: &str) {
        let updated = updated_spacing(self.spacing.enabled(), setting);
        if updated == self.spacing.enabled() {
            return;
        }
        // `.Sm off` changes spacing *after* the request. If printable
        // content precedes the transition, retain its ordinary boundary to
        // the first following fragment, then concatenate subsequent macro
        // arguments until spacing is enabled again.
        self.boundary = match (updated, !self.has_printable_content, self.boundary) {
            (_, _, boundary) if boundary.is_tight() => boundary,
            (false, false, _) => PendingBoundary::Preserved,
            _ => PendingBoundary::Ordinary,
        };
        self.spacing = SpacingMode::from(updated);
    }

    /// Carry formatter state out of a nested structural wrapper.
    ///
    /// The nested builder has already applied the transition at its exact
    /// source position. The parent therefore inherits only the final state;
    /// replaying `set_spacing` here would invent a preserved boundary after a
    /// nested `Sm off` request.
    pub(in crate::mandoc) fn inherit_spacing(&mut self, spacing_enabled: bool) {
        self.spacing = SpacingMode::from(spacing_enabled);
        if matches!(self.boundary, PendingBoundary::Preserved) {
            self.boundary = PendingBoundary::Ordinary;
        }
    }

    pub(in crate::mandoc) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub(in crate::mandoc) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Save presentation-only state before executing an operand whose
    /// rendered spelling may be replaced. Font, spacing, and zero-advance
    /// state are intentionally not part of this snapshot: they are execution
    /// effects and must survive an eventual compact-output fallback.
    pub(in crate::mandoc) fn output_checkpoint(&self) -> OutputCheckpoint {
        OutputCheckpoint {
            node_count: self.nodes.len(),
            boundary: self.boundary,
            last_visible_character: self.last_visible_character,
            has_printable_content: self.has_printable_content,
            empty_word: self.empty_word,
            pending_word_spaces: self.pending_word_spaces,
            source_cursor: self.source_cursor.clone(),
        }
    }

    /// Whether output since `checkpoint` contains a glyph a reader can use
    /// as a semantic link label. Whitespace-only output is not a label: link
    /// identity must fall back to its visible target instead of wrapping an
    /// invisible click region.
    pub(in crate::mandoc) fn output_since_has_non_whitespace_glyph(
        &self,
        checkpoint: &OutputCheckpoint,
    ) -> bool {
        fn contains_glyph(nodes: &[Inline]) -> bool {
            nodes.iter().any(|node| match node {
                Inline::Text { value } | Inline::Code { value } => {
                    value.chars().any(|character| !character.is_whitespace())
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => contains_glyph(children),
                Inline::Anchor { .. } | Inline::LineBreak => false,
            })
        }
        contains_glyph(&self.nodes[checkpoint.node_count..])
    }

    /// Drop only the projected representation emitted since `checkpoint`.
    /// This is used by semantic macros that compactly replace a source
    /// operand. Its formatter execution state remains in the builder.
    pub(in crate::mandoc) fn discard_output_since(&mut self, checkpoint: OutputCheckpoint) {
        self.nodes.truncate(checkpoint.node_count);
        self.boundary = checkpoint.boundary;
        self.last_visible_character = checkpoint.last_visible_character;
        self.has_printable_content = checkpoint.has_printable_content;
        self.empty_word = checkpoint.empty_word;
        self.pending_word_spaces = checkpoint.pending_word_spaces;
        self.source_cursor = checkpoint.source_cursor;
    }

    /// Drop a compactly hidden operand's output while preserving the
    /// formatter transitions it performed.  In particular, `\\c` changes the
    /// next word's boundary and a literal row advances its source cursor;
    /// those are execution facts even though a semantic macro may replace the
    /// operand's visible spelling.
    pub(in crate::mandoc) fn discard_output_preserving_execution(
        &mut self,
        checkpoint: OutputCheckpoint,
    ) {
        let retained_line_breaks = line_break_count(&self.nodes[checkpoint.node_count..]);
        let boundary = self.boundary;
        let source_cursor = self.source_cursor.clone();
        let zero_advance_joined = self.zero_advance_joined;
        self.discard_output_since(checkpoint);
        self.boundary = boundary;
        self.source_cursor = source_cursor;
        self.zero_advance_joined = zero_advance_joined;
        // Semantic compaction may replace an operand's glyphs, never its
        // layout. Native `term_word()` writes an explicit break immediately;
        // retain that event outside a subsequently visible fallback link so a
        // cursor that already consumed the source row cannot erase it.
        self.retain_line_breaks(retained_line_breaks);
    }

    /// Wrap the output emitted since `checkpoint` without replaying its
    /// formatter execution. Links and other semantic wrappers are IR
    /// annotations over an already-executed source stream.
    pub(in crate::mandoc) fn wrap_output_since(
        &mut self,
        checkpoint: &OutputCheckpoint,
        wrap: impl FnOnce(Vec<Inline>) -> Vec<Inline>,
    ) {
        let output = self.nodes.split_off(checkpoint.node_count);
        self.nodes.extend(wrap(output));
    }

    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    pub(in crate::mandoc) fn hard_break(&mut self) {
        self.flush_zero_advance();
        self.boundary = PendingBoundary::Ordinary;
        self.empty_word = false;
        self.pending_word_spaces = 0;
        if self.has_printable_content && !matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.push(Inline::LineBreak);
            self.last_visible_character = Some('\n');
        }
        self.source_line_continuation = Some(false);
    }

    pub(in crate::mandoc) fn append(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming, false, true);
    }

    /// A native text node is a word event even if decoding yields no glyphs.
    /// Unlike a target/control-only append, it consumes the pending boundary.
    pub(in crate::mandoc) fn append_word(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming, true, true);
    }

    /// A decoded empty word can be a real literal row, an empty macro
    /// argument, or a pure formatter transition. Keep those execution facts
    /// separate without changing filled-flow inter-word spacing.
    pub(in crate::mandoc) fn append_word_with_literal_row(
        &mut self,
        mut incoming: Vec<Inline>,
        occupies_row: bool,
    ) {
        self.append_at_boundary(&mut incoming, true, occupies_row);
    }

    /// Generated glyphs use the effective font just like authored text, but
    /// are not reparsed as roff source (names can contain literal escapes).
    pub(in crate::mandoc) fn append_text(&mut self, value: &str) {
        let mut projected = Vec::new();
        self.zero_advance
            .append_generated_text(value, &mut projected, self.font.current);
        self.append(projected);
        // Generated formatter words (for example Lk's colon or enclosure
        // delimiters) cannot themselves carry source `\\c`; they consume a
        // preceding continuation before the next source operand runs.
        if !value.is_empty() {
            self.source_line_continuation = Some(false);
        }
    }

    /// Start an ordinary formatter word after a source text node. If a prior
    /// `\z` glyph is pending, model term.c's implicit blank before decoding
    /// the next glyph: preserve the pending glyph but suppress this reader's
    /// own inter-word space. Tight joins (for example alternating `.BR`
    /// operands) deliberately bypass this transition and overstrike instead.
    pub(in crate::mandoc) fn begin_word_projection(&mut self, next_is_visible: bool) {
        if !next_is_visible
            || self.boundary.is_tight()
            || !(self.has_printable_content || self.zero_advance.has_pending_glyph())
            || !(self.spacing.enabled() || matches!(self.boundary, PendingBoundary::Preserved))
        {
            return;
        }
        let Some(glyph) = self.zero_advance.resolve_at_word_boundary() else {
            return;
        };
        self.append_projected(vec![glyph]);
        self.boundary = PendingBoundary::Tight;
    }

    /// Only macros that select a font establish this scope. Transparent
    /// macros and text operands must not invent a push/pop of their own.
    pub(in crate::mandoc) fn with_font_scope(
        &mut self,
        font: Font,
        append: impl FnOnce(&mut Self),
    ) {
        let saved = self.font.push_scope(font);
        append(self);
        self.font.pop_scope(saved);
    }

    /// Style newly appended content without creating a new formatter state.
    /// The transform must preserve visible characters and line boundaries.
    /// In particular, a wrapper ending after `\\c` must not consume its
    /// pending join merely because its content is represented as a subtree.
    pub(in crate::mandoc) fn append_scope(
        &mut self,
        append: impl FnOnce(&mut Self),
        style: impl FnOnce(Vec<Inline>) -> Vec<Inline>,
    ) {
        // Appending must still see the prefix, especially a preceding hard
        // break used for deduplication. Style only the new suffix afterwards.
        let start = self.nodes.len();
        append(self);
        let inner = self.nodes.split_off(start);
        self.nodes.extend(style(inner));
    }

    /// Append content using the formatter-level boundary selected by the
    /// block lowering pass.
    pub(in crate::mandoc) fn append_filled(
        &mut self,
        incoming: Vec<Inline>,
        boundary: FilledBoundary,
    ) {
        match boundary {
            FilledBoundary::SameLine => self.append(incoming),
            FilledBoundary::Word => {
                let mut incoming = incoming;
                self.append_at_boundary(&mut incoming, false, true);
            }
            FilledBoundary::LineBreak => {
                self.hard_break();
                self.append(incoming);
            }
        }
    }

    fn append_at_boundary(&mut self, incoming: &mut Vec<Inline>, word: bool, occupies_row: bool) {
        if incoming.is_empty() && !word {
            return;
        }
        let incoming_first = first_visible_character(incoming);
        let incoming_last = last_visible_character(incoming);
        let incoming_has_printable = has_printable_character(incoming);
        if incoming_first.is_none() && !incoming_has_printable && !word {
            self.nodes.append(incoming);
            return;
        }
        if let Some(cursor) = &mut self.source_cursor {
            let pending = cursor.pending();
            let empty_row = pending && incoming.is_empty() && word && occupies_row;
            if cursor.word(occupies_row) {
                self.nodes.push(Inline::LineBreak);
                self.last_visible_character = Some('\n');
                self.boundary = PendingBoundary::Ordinary;
                self.pending_word_spaces = 0;
                self.empty_word = false;
            }
            if empty_row {
                // Empty executed rows are content, including at a display's
                // start/end. Do not turn them into inter-word padding.
                self.nodes.push(Inline::Text {
                    value: String::new(),
                });
                return;
            }
            if pending && !occupies_row && incoming.is_empty() {
                return;
            }
        }
        let empty_word = word && incoming_first.is_none() && !incoming_has_printable;
        let add_space = if empty_word || self.empty_word {
            self.has_printable_content
        } else {
            needs_boundary_space(self.last_visible_character, incoming_first)
        };
        let boundary = std::mem::replace(&mut self.boundary, PendingBoundary::Ordinary);
        if !empty_word && self.pending_word_spaces > 0 {
            push_text(&mut self.nodes, " ".repeat(self.pending_word_spaces));
            self.pending_word_spaces = 0;
        }
        if (self.spacing.enabled() || matches!(boundary, PendingBoundary::Preserved))
            && !boundary.is_tight()
            && add_space
        {
            if empty_word {
                // A word boundary is real, but trailing formatter padding
                // is not authored term content. Materialize at the next glyph.
                self.pending_word_spaces = self.pending_word_spaces.saturating_add(1);
            } else {
                push_text(&mut self.nodes, " ".to_owned());
                self.last_visible_character = Some(' ');
                self.has_printable_content = true;
            }
        }
        self.nodes.append(incoming);
        if incoming_last.is_some() {
            self.last_visible_character = incoming_last;
        }
        self.has_printable_content |= incoming_has_printable;
        self.empty_word = empty_word;
    }

    pub(in crate::mandoc) fn finish(mut self) -> Vec<Inline> {
        self.flush_zero_advance();
        self.finish_nodes()
    }

    /// Return an inner scope without forcing a pending `\\z` glyph to become
    /// visible.  CVS mandoc carries its backtracking flags through nested
    /// `term_word()` calls, so the caller must continue the state in the
    /// surrounding inline stream before committing it at a real boundary.
    pub(in crate::mandoc) fn finish_preserving_zero_advance(
        mut self,
    ) -> (Vec<Inline>, ZeroAdvanceState, bool) {
        let zero_advance = std::mem::take(&mut self.zero_advance);
        let joined = self.zero_advance_joined;
        (self.finish_nodes(), zero_advance, joined)
    }

    fn finish_nodes(&mut self) -> Vec<Inline> {
        if let Some(cursor) = &self.source_cursor {
            if !cursor.row_occupied()
                && let Some(last) = self
                    .nodes
                    .iter()
                    .rposition(|node| !matches!(node, Inline::Anchor { .. }))
                && matches!(self.nodes[last], Inline::LineBreak)
            {
                self.nodes.remove(last);
            }
            return std::mem::take(&mut self.nodes);
        }
        while matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.pop();
        }
        std::mem::take(&mut self.nodes)
    }

    fn flush_zero_advance(&mut self) {
        let mut pending = Vec::new();
        self.zero_advance.finish_into(&mut pending);
        self.append_projected(pending);
    }

    fn append_projected(&mut self, mut incoming: Vec<Inline>) {
        if incoming.is_empty() {
            return;
        }
        let last = last_visible_character(&incoming);
        let printable = has_printable_character(&incoming);
        self.nodes.append(&mut incoming);
        if last.is_some() {
            self.last_visible_character = last;
        }
        self.has_printable_content |= printable;
    }

    fn retain_line_breaks(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.nodes
            .extend(std::iter::repeat_n(Inline::LineBreak, count));
        self.last_visible_character = Some('\n');
        self.empty_word = false;
        self.pending_word_spaces = 0;
    }
}

/// Count layout instructions recursively before discarding a compact operand.
/// Styling and link wrappers are presentation-only, but their explicit line
/// breaks are formatter output and must survive without retaining the hidden
/// operand's glyphs.
fn line_break_count(nodes: &[Inline]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Inline::LineBreak => 1,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => line_break_count(children),
            Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => 0,
        })
        .sum()
}
