//! Filled flow preserves font, spacing and pending boundaries across scopes.
use super::{Font, needs_boundary_space, push_text, updated_spacing};
use crate::inline::{first_visible_character, has_printable_character, last_visible_character};
use mant_ir::Inline;

pub(in crate::mandoc) struct InlineBuilder {
    nodes: Vec<Inline>,
    boundary: PendingBoundary,
    spacing: SpacingMode,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    pub(in crate::mandoc) font: FontState,
}

/// Roff remembers the previous selection independently of the current font.
#[derive(Clone, Copy, Debug)]
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
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::Enabled,
            last_visible_character: None,
            has_printable_content: false,
            font: FontState::new(),
        }
    }

    pub(in crate::mandoc) const fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            nodes: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::from_enabled(spacing_enabled),
            last_visible_character: None,
            has_printable_content: false,
            font: FontState::new(),
        }
    }

    pub(in crate::mandoc) fn tighten_next_boundary(&mut self) {
        self.boundary = PendingBoundary::Tight;
    }

    /// Join a generated prefix only to its own operand scope. Empty text and
    /// zero-width anchors do not consume the join, so expire it on scope exit.
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

    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    pub(in crate::mandoc) fn hard_break(&mut self) {
        self.boundary = PendingBoundary::Ordinary;
        if self.has_printable_content && !matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.push(Inline::LineBreak);
            self.last_visible_character = Some('\n');
        }
    }

    pub(in crate::mandoc) fn append(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming);
    }

    /// Generated glyphs use the effective font just like authored text, but
    /// are not reparsed as roff source (names can contain literal escapes).
    pub(in crate::mandoc) fn append_text(&mut self, value: &str) {
        self.append(vec![super::font::styled_segment(
            value.into(),
            self.font.current,
        )]);
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

    /// Add physical blank rows without resetting font or spacing state.
    pub(in crate::mandoc) fn blank_rows(&mut self, rows: u16) {
        self.hard_break();
        if self.has_printable_content {
            self.nodes
                .extend(std::iter::repeat_n(Inline::LineBreak, usize::from(rows)));
        }
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
                self.append_at_boundary(&mut incoming);
            }
            FilledBoundary::LineBreak => {
                self.hard_break();
                self.append(incoming);
            }
        }
    }

    fn append_at_boundary(&mut self, incoming: &mut Vec<Inline>) {
        if incoming.is_empty() {
            return;
        }
        let incoming_first = first_visible_character(incoming);
        let incoming_last = last_visible_character(incoming);
        let incoming_has_printable = has_printable_character(incoming);
        if incoming_first.is_none() && !incoming_has_printable {
            self.nodes.append(incoming);
            return;
        }
        let add_space = needs_boundary_space(self.last_visible_character, incoming_first);
        let boundary = std::mem::replace(&mut self.boundary, PendingBoundary::Ordinary);
        if (self.spacing.enabled() || matches!(boundary, PendingBoundary::Preserved))
            && !boundary.is_tight()
            && add_space
        {
            push_text(&mut self.nodes, " ".to_owned());
            self.last_visible_character = Some(' ');
            self.has_printable_content = true;
        }
        self.nodes.append(incoming);
        if incoming_last.is_some() {
            self.last_visible_character = incoming_last;
        }
        self.has_printable_content |= incoming_has_printable;
    }

    pub(in crate::mandoc) fn finish(mut self) -> Vec<Inline> {
        while matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.pop();
        }
        self.nodes
    }
}
